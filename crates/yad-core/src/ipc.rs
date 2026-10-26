//! Newline-delimited JSON over a local socket: a Unix domain socket in the config directory
//! (user-only permissions) or a named pipe on Windows.

use interprocess::local_socket::{
    tokio::{prelude::*, Stream},
    ListenerOptions, Name,
};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    io,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    sync::{mpsc, oneshot},
};

use crate::{
    config::Paths,
    manager::Manager,
    protocol::{Event, Message, Request},
    service::LaunchCommand,
};

pub fn socket_name(paths: &Paths) -> io::Result<Name<'static>> {
    #[cfg(windows)]
    {
        use interprocess::local_socket::GenericNamespaced;
        let _ = paths;
        crate::config::PIPE_NAME.to_ns_name::<GenericNamespaced>().map(|n| n.into_owned())
    }
    #[cfg(not(windows))]
    {
        use interprocess::local_socket::GenericFilePath;
        paths.socket_path().to_fs_name::<GenericFilePath>().map(|n| n.into_owned())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ServeError {
    #[error("YAD is already running")]
    AlreadyRunning,
    #[error("{0}")]
    Io(#[from] io::Error),
}

/// Accepts connections until the manager shuts down.
pub async fn serve(manager: Manager) -> Result<(), ServeError> {
    let paths = manager.paths().clone();
    if Stream::connect(socket_name(&paths)?).await.is_ok() {
        return Err(ServeError::AlreadyRunning);
    }
    #[cfg(unix)]
    {
        // A socket file left by a crashed daemon blocks binding.
        let _ = std::fs::remove_file(paths.socket_path());
    }
    let listener = ListenerOptions::new().name(socket_name(&paths)?).create_tokio()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(paths.socket_path(), std::fs::Permissions::from_mode(0o600))?;
    }
    log::info!("listening on {}", paths.socket_path().display());
    let next_conn = AtomicU64::new(1);
    let shutdown = manager.shutdown_token();
    loop {
        let conn = tokio::select! {
            c = listener.accept() => c,
            _ = shutdown.cancelled() => break,
        };
        match conn {
            Ok(stream) => {
                let id = next_conn.fetch_add(1, Ordering::SeqCst);
                let m = manager.clone();
                tokio::spawn(async move {
                    handle_connection(m.clone(), stream, id).await;
                    m.connection_closed(id);
                });
            }
            Err(e) => log::warn!("accept failed: {e}"),
        }
    }
    #[cfg(unix)]
    {
        let _ = std::fs::remove_file(paths.socket_path());
    }
    Ok(())
}

async fn handle_connection(manager: Manager, stream: Stream, conn: u64) {
    let stream = Arc::new(stream);
    let (tx, mut rx) = mpsc::channel::<Message>(256);
    let out = stream.clone();
    let writer = tokio::spawn(async move {
        let mut send = &*out;
        while let Some(msg) = rx.recv().await {
            let mut line = serde_json::to_vec(&msg).unwrap_or_default();
            line.push(b'\n');
            if send.write_all(&line).await.is_err() {
                break;
            }
        }
    });
    let mut lines = BufReader::new(&*stream).lines();
    let mut subscribed = false;
    while let Ok(Some(line)) = lines.next_line().await {
        if line.trim().is_empty() {
            continue;
        }
        let req: Request = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let _ = tx.send(Message::err(Value::Null, format!("Bad request: {e}"))).await;
                continue;
            }
        };
        if req.method == "subscribe" {
            if !subscribed {
                subscribed = true;
                let mut events = manager.subscribe();
                let tx = tx.clone();
                tokio::spawn(async move {
                    loop {
                        match events.recv().await {
                            Ok(e) => {
                                if tx.send(Message::event(e)).await.is_err() {
                                    break;
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                            Err(_) => break,
                        }
                    }
                });
            }
            let _ = tx.send(Message::ok(req.id, json!(true))).await;
            continue;
        }
        // Requests run concurrently so a slow probe doesn't block a pause.
        let m = manager.clone();
        let tx = tx.clone();
        tokio::spawn(async move {
            let msg = match m.handle(&req.method, req.params, conn).await {
                Ok(v) => Message::ok(req.id, v),
                Err(e) => Message::err(req.id, e),
            };
            let _ = tx.send(msg).await;
        });
    }
    drop(tx);
    writer.abort();
}

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>>;

/// A connection to the daemon. Cheap to clone.
#[derive(Clone)]
pub struct Client {
    tx: mpsc::Sender<Vec<u8>>,
    pending: Pending,
    next: Arc<AtomicU64>,
}

impl Client {
    pub async fn connect(paths: &Paths) -> io::Result<(Client, mpsc::Receiver<Event>, oneshot::Receiver<()>)> {
        let stream = Arc::new(Stream::connect(socket_name(paths)?).await?);
        let (send_half, recv_half) = (stream.clone(), stream);
        let (tx, mut out_rx) = mpsc::channel::<Vec<u8>>(256);
        let (event_tx, event_rx) = mpsc::channel::<Event>(1024);
        let (closed_tx, closed_rx) = oneshot::channel();
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));

        tokio::spawn(async move {
            let mut send = &*send_half;
            while let Some(mut buf) = out_rx.recv().await {
                buf.push(b'\n');
                if send.write_all(&buf).await.is_err() {
                    break;
                }
            }
        });
        let p = pending.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(&*recv_half).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let Ok(msg) = serde_json::from_str::<Message>(&line) else { continue };
                if let Some(e) = msg.event {
                    let _ = event_tx.try_send(e);
                    continue;
                }
                let Some(id) = msg.id.as_ref().and_then(Value::as_u64) else { continue };
                if let Some(waiter) = p.lock().unwrap_or_else(|e| e.into_inner()).remove(&id) {
                    let _ = waiter.send(match msg.error {
                        Some(e) => Err(e.message),
                        None => Ok(msg.result.unwrap_or(Value::Null)),
                    });
                }
            }
            // Fail everything still waiting.
            p.lock().unwrap_or_else(|e| e.into_inner()).clear();
            let _ = closed_tx.send(());
        });
        Ok((Client { tx, pending, next: Arc::new(AtomicU64::new(1)) }, event_rx, closed_rx))
    }

    /// Connects, starting the daemon with `launch` if it isn't running.
    pub async fn connect_or_spawn(
        paths: &Paths,
        launch: &LaunchCommand,
    ) -> io::Result<(Client, mpsc::Receiver<Event>, oneshot::Receiver<()>)> {
        if let Ok(c) = Client::connect(paths).await {
            return Ok(c);
        }
        launch.spawn_detached()?;
        let mut last = io::Error::other("YAD service didn't start");
        for _ in 0..50 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            match Client::connect(paths).await {
                Ok(c) => return Ok(c),
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    pub async fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap_or_else(|e| e.into_inner()).insert(id, tx);
        let line = serde_json::to_vec(&json!({"id": id, "method": method, "params": params})).map_err(|e| e.to_string())?;
        if self.tx.send(line).await.is_err() {
            self.pending.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
            return Err("Can't reach the YAD service.".into());
        }
        // Probing a slow server can take a while; everything else answers at once.
        match tokio::time::timeout(Duration::from_secs(90), rx).await {
            Ok(Ok(r)) => r,
            Ok(Err(_)) => Err("Can't reach the YAD service.".into()),
            Err(_) => {
                self.pending.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
                Err("The YAD service didn't answer in time.".into())
            }
        }
    }
}
