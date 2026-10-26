//! The YAD background service and the browser native-messaging host.
//!
//! Both run from the standalone `yadd` binary and from the app binary
//! (`yad --daemon`, `yad --native-host`), so a bundled app needs no extra executable.

use std::{
    fs::OpenOptions,
    io::{Read, Write},
    path::Path,
    sync::{Arc, Mutex},
};

use serde_json::{json, Value};
use yad_core::{
    config::Paths,
    ipc::{self, Client, ServeError},
    manager::Manager,
    service::{self, LaunchCommand},
    storage::Storage,
};

/// True when Chrome started us as a native-messaging host: it passes the caller's origin
/// (`chrome-extension://…/`) as the first argument.
pub fn is_native_host_invocation(args: &[String]) -> bool {
    args.iter().skip(1).any(|a| a == "--native-host" || a == "native-host" || a.starts_with("chrome-extension://"))
}

fn init_logging(paths: &Paths, to_file: bool) {
    let mut builder = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"));
    if to_file {
        let log = paths.log_path();
        if std::fs::metadata(&log).map(|m| m.len() > 2 * 1024 * 1024).unwrap_or(false) {
            let _ = std::fs::remove_file(&log);
        }
        if let Ok(f) = OpenOptions::new().create(true).append(true).open(&log) {
            builder.target(env_logger::Target::Pipe(Box::new(f)));
        }
    }
    let _ = builder.try_init();
}

/// Runs the download service until `shutdown` or a termination signal.
/// `launch` is how this daemon is started again (for "start at login").
pub fn run_daemon(launch: LaunchCommand) -> i32 {
    let paths = Paths::from_env();
    if let Err(e) = paths.ensure() {
        eprintln!("Couldn't create {}: {e}", paths.config_dir.display());
        return 1;
    }
    init_logging(&paths, !atty_stderr());
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("runtime");
    rt.block_on(async move {
        let storage = match Storage::open(&paths.db_path()) {
            Ok(s) => s,
            Err(e) => {
                log::error!("Couldn't open the database: {e}");
                return 1;
            }
        };
        let host_exe = launch.exe.clone();
        let manager = Manager::new(paths.clone(), storage, launch);
        // Keep the browser extension's native host pointing at this binary.
        // YAD_SKIP_NATIVE_HOST=1 leaves browser settings alone (tests, development).
        if std::env::var_os("YAD_SKIP_NATIVE_HOST").is_none() {
            match service::register_native_host(&host_exe, &paths) {
                Ok(done) => log::info!("native host registered for {} browser(s)", done.len()),
                Err(e) => log::warn!("couldn't register the native host: {e}"),
            }
        }
        let server = tokio::spawn(ipc::serve(manager.clone()));
        // Bind first, then start downloading, so a second instance exits without touching anything.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        if server.is_finished() {
            return match server.await {
                Ok(Err(ServeError::AlreadyRunning)) => {
                    log::info!("already running");
                    0
                }
                Ok(Err(e)) => {
                    log::error!("couldn't listen: {e}");
                    1
                }
                _ => 1,
            };
        }
        manager.start();
        log::info!("YAD service {} started (pid {})", yad_core::config::VERSION, std::process::id());
        let token = manager.shutdown_token();
        tokio::select! {
            _ = token.cancelled() => {}
            _ = tokio::signal::ctrl_c() => manager.shutdown().await,
            _ = terminate() => manager.shutdown().await,
        }
        let _ = server.await;
        log::info!("YAD service stopped");
        0
    })
}

#[cfg(unix)]
async fn terminate() {
    if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
        s.recv().await;
    } else {
        std::future::pending::<()>().await;
    }
}

#[cfg(not(unix))]
async fn terminate() {
    std::future::pending::<()>().await;
}

fn atty_stderr() -> bool {
    use std::io::IsTerminal;
    std::io::stderr().is_terminal()
}

/// Chrome native messaging: 4-byte little-endian length, then JSON. Each request is
/// forwarded to the daemon (started on demand) and answered with the same `id`.
pub fn run_native_host(launch: LaunchCommand) -> i32 {
    let paths = Paths::from_env();
    let _ = paths.ensure();
    init_logging(&paths, true);
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("runtime");
    rt.block_on(async move {
        let (in_tx, mut in_rx) = tokio::sync::mpsc::channel::<Value>(64);
        std::thread::spawn(move || {
            let mut stdin = std::io::stdin().lock();
            loop {
                let mut len = [0u8; 4];
                if stdin.read_exact(&mut len).is_err() {
                    break;
                }
                let len = u32::from_le_bytes(len) as usize;
                if len > 64 * 1024 * 1024 {
                    break;
                }
                let mut buf = vec![0u8; len];
                if stdin.read_exact(&mut buf).is_err() {
                    break;
                }
                match serde_json::from_slice(&buf) {
                    Ok(v) => {
                        if in_tx.blocking_send(v).is_err() {
                            break;
                        }
                    }
                    Err(e) => log::warn!("native host: bad message: {e}"),
                }
            }
        });
        let out = Arc::new(Mutex::new(std::io::stdout()));
        let client: Arc<tokio::sync::Mutex<Option<Client>>> = Arc::new(tokio::sync::Mutex::new(None));
        let mut hello: Option<Value> = None;
        while let Some(msg) = in_rx.recv().await {
            let id = msg.get("id").cloned().unwrap_or(Value::Null);
            let method = msg.get("method").and_then(Value::as_str).unwrap_or("").to_string();
            let params = msg.get("params").cloned().unwrap_or(Value::Null);
            if method == "hello" {
                hello = Some(params.clone());
            }
            // Only something the user did (download, probe, "Start YAD") may start the
            // service. Background calls report it as not running, so quitting YAD sticks.
            let may_start = !matches!(method.as_str(), "hello" | "ping" | "status" | "getSettings" | "setSettings");
            if method == "start" {
                let reply = match ensure_client(&client, &paths, &launch, hello.as_ref(), true).await {
                    Ok(c) => match c.call("status", Value::Null).await {
                        Ok(v) => json!({"id": id, "result": v}),
                        Err(e) => json!({"id": id, "error": {"message": e}}),
                    },
                    Err(e) => json!({"id": id, "error": {"message": e}}),
                };
                write_native(&out, &reply);
                continue;
            }
            let c = match ensure_client(&client, &paths, &launch, hello.as_ref(), may_start).await {
                Ok(c) => c,
                Err(e) => {
                    write_native(&out, &json!({"id": id, "error": {"message": e}}));
                    continue;
                }
            };
            let out = out.clone();
            tokio::spawn(async move {
                let reply = match c.call(&method, params).await {
                    Ok(v) => json!({"id": id, "result": v}),
                    Err(e) => json!({"id": id, "error": {"message": e}}),
                };
                write_native(&out, &reply);
            });
        }
        0
    })
}

async fn ensure_client(
    slot: &tokio::sync::Mutex<Option<Client>>,
    paths: &Paths,
    launch: &LaunchCommand,
    hello: Option<&Value>,
    may_start: bool,
) -> Result<Client, String> {
    let mut guard = slot.lock().await;
    if let Some(c) = guard.as_ref() {
        if c.call("ping", Value::Null).await.is_ok() {
            return Ok(c.clone());
        }
    }
    let (c, _events, _closed) = if may_start {
        Client::connect_or_spawn(paths, launch).await.map_err(|_| "YAD isn't running and couldn't be started.".to_string())?
    } else {
        Client::connect(paths).await.map_err(|_| "YAD isn't running.".to_string())?
    };
    // A new daemon connection: say hello again so the app shows the extension as linked.
    if let Some(h) = hello {
        let _ = c.call("hello", h.clone()).await;
    }
    *guard = Some(c.clone());
    Ok(c)
}

fn write_native(out: &Mutex<std::io::Stdout>, v: &Value) {
    let bytes = serde_json::to_vec(v).unwrap_or_default();
    let mut o = out.lock().unwrap_or_else(|e| e.into_inner());
    let _ = o.write_all(&(bytes.len() as u32).to_le_bytes());
    let _ = o.write_all(&bytes);
    let _ = o.flush();
}

/// `yadd install-native-host`: registers this binary for every installed Chromium browser.
pub fn install_native_host() -> i32 {
    let paths = Paths::from_env();
    let _ = paths.ensure();
    let exe = std::env::current_exe().unwrap_or_else(|_| Path::new("yadd").to_path_buf());
    match service::register_native_host(&exe, &paths) {
        Ok(done) if done.is_empty() => {
            eprintln!("No Chromium-based browser found.");
            1
        }
        Ok(done) => {
            for d in done {
                println!("Registered {}", d.display());
            }
            0
        }
        Err(e) => {
            eprintln!("Couldn't register the native host: {e}");
            1
        }
    }
}
