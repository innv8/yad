//! The YAD window. It owns no downloads: every call goes to the background service over
//! the local socket (starting it when needed) and service events are relayed to the page.

use serde_json::Value;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tauri::{
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, PhysicalPosition, RunEvent, WindowEvent,
};
use tokio::sync::RwLock;
use yad_core::{config::Paths, ipc::Client, protocol::Event, service::LaunchCommand};

struct Bridge {
    client: RwLock<Option<Client>>,
    paths: Paths,
    launch: LaunchCommand,
    /// Mirrors Settings.keepRunning: closing the window hides it instead of quitting.
    keep_running: AtomicBool,
}

impl Bridge {
    async fn client(&self) -> Result<Client, String> {
        if let Some(c) = self.client.read().await.clone() {
            return Ok(c);
        }
        // The connection loop is (re)connecting; give it a moment.
        for _ in 0..50 {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            if let Some(c) = self.client.read().await.clone() {
                return Ok(c);
            }
        }
        Err("Can't reach the YAD service.".into())
    }
}

#[tauri::command]
async fn ipc(bridge: tauri::State<'_, Arc<Bridge>>, method: String, params: Option<Value>) -> Result<Value, String> {
    let client = bridge.client().await?;
    let result = client.call(&method, params.unwrap_or(Value::Null)).await;
    if method == "getSettings" || method == "setSettings" {
        if let Ok(v) = &result {
            if let Some(k) = v.get("keepRunning").and_then(Value::as_bool) {
                bridge.keep_running.store(k, Ordering::SeqCst);
            }
        }
    }
    result
}

#[tauri::command]
fn open_path(app: AppHandle, path: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    if path.starts_with("http://") || path.starts_with("https://") {
        app.opener().open_url(path, None::<&str>).map_err(|e| e.to_string())
    } else {
        app.opener().open_path(path, None::<&str>).map_err(|e| format!("Couldn't open the file: {e}"))
    }
}

#[tauri::command]
fn reveal_path(app: AppHandle, path: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let p = std::path::PathBuf::from(&path);
    if p.exists() {
        app.opener().reveal_item_in_dir(p).map_err(|e| format!("Couldn't show the file: {e}"))
    } else {
        // Not downloaded yet: open the folder it will go to.
        let dir = p.parent().map(|d| d.to_path_buf()).unwrap_or(p);
        let _ = std::fs::create_dir_all(&dir);
        app.opener().open_path(dir.to_string_lossy(), None::<&str>).map_err(|e| e.to_string())
    }
}

#[tauri::command]
fn show_main(app: AppHandle) {
    show_main_window(&app);
}

#[tauri::command]
async fn quit_app(app: AppHandle, bridge: tauri::State<'_, Arc<Bridge>>, stop_daemon: Option<bool>) -> Result<(), String> {
    if stop_daemon.unwrap_or(false) {
        if let Ok(c) = bridge.client().await {
            let _ = c.call("shutdown", Value::Null).await;
        }
    }
    app.exit(0);
    Ok(())
}

fn show_main_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
    if let Some(t) = app.get_webview_window("tray") {
        let _ = t.hide();
    }
}

/// Keeps one connection to the service, restarting it if it goes away, and relays events.
async fn connection_loop(app: AppHandle, bridge: Arc<Bridge>) {
    loop {
        match Client::connect_or_spawn(&bridge.paths, &bridge.launch).await {
            Ok((client, mut events, closed)) => {
                let _ = client.call("subscribe", Value::Null).await;
                if let Ok(s) = client.call("getSettings", Value::Null).await {
                    if let Some(k) = s.get("keepRunning").and_then(Value::as_bool) {
                        bridge.keep_running.store(k, Ordering::SeqCst);
                    }
                }
                *bridge.client.write().await = Some(client);
                let _ = app.emit("yad-connection", serde_json::json!({ "connected": true }));
                let relay = {
                    let app = app.clone();
                    let bridge = bridge.clone();
                    tokio::spawn(async move {
                        while let Some(e) = events.recv().await {
                            if let Event::Settings { settings } = &e {
                                bridge.keep_running.store(settings.keep_running, Ordering::SeqCst);
                            }
                            let _ = app.emit("yad-event", &e);
                        }
                    })
                };
                let _ = closed.await;
                relay.abort();
                *bridge.client.write().await = None;
                let _ = app.emit("yad-connection", serde_json::json!({ "connected": false }));
            }
            Err(e) => log::warn!("can't reach the service: {e}"),
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}

fn toggle_tray_window(app: &AppHandle, click: PhysicalPosition<f64>) {
    let Some(w) = app.get_webview_window("tray") else { return };
    if w.is_visible().unwrap_or(false) {
        let _ = w.hide();
        return;
    }
    let size = w.outer_size().unwrap_or_default();
    let (mut x, mut y) = (click.x - size.width as f64 / 2.0, click.y + 8.0);
    let mut edge = "top";
    if let Ok(Some(m)) = w.current_monitor().or_else(|_| app.primary_monitor()) {
        let mpos = m.position();
        let msize = m.size();
        // Taskbar at the bottom (Windows, many Linux desktops): open upwards.
        if click.y > mpos.y as f64 + msize.height as f64 / 2.0 {
            y = click.y - size.height as f64 - 8.0;
            edge = "bottom";
        }
        let max_x = (mpos.x + msize.width as i32) as f64 - size.width as f64 - 8.0;
        x = x.clamp(mpos.x as f64 + 8.0, max_x.max(mpos.x as f64 + 8.0));
    }
    let _ = w.set_position(PhysicalPosition::new(x, y));
    // The card hugs the side next to the tray icon; the rest of the window is transparent.
    let _ = w.emit("tray-anchor", serde_json::json!({ "edge": edge }));
    let _ = w.show();
    let _ = w.set_focus();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let bridge = Arc::new(Bridge {
        client: RwLock::new(None),
        paths: Paths::from_env(),
        launch: LaunchCommand::current(&["--daemon"]),
        keep_running: AtomicBool::new(true),
    });

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_main_window(app)))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(bridge.clone())
        .invoke_handler(tauri::generate_handler![ipc, open_path, reveal_path, show_main, quit_app])
        .setup(move |app| {
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(connection_loop(handle, bridge.clone()));

            let mut tray = TrayIconBuilder::with_id("yad").tooltip("YAD").on_tray_icon_event(|tray, event| {
                if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, position, .. } = event {
                    toggle_tray_window(tray.app_handle(), position);
                }
            });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            #[cfg(target_os = "macos")]
            {
                tray = tray.icon_as_template(false);
            }
            tray.build(app)?;

            if let Some(t) = app.get_webview_window("tray") {
                let t2 = t.clone();
                t.on_window_event(move |e| {
                    if let WindowEvent::Focused(false) = e {
                        let _ = t2.hide();
                    }
                });
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }
            if let WindowEvent::CloseRequested { api, .. } = event {
                let bridge = window.state::<Arc<Bridge>>();
                if bridge.keep_running.load(Ordering::SeqCst) {
                    // Downloads go on in the service; the tray icon brings the window back.
                    api.prevent_close();
                    let _ = window.hide();
                } else {
                    // "Keep downloading when the window is closed" is off: stop everything.
                    let bridge = bridge.inner().clone();
                    let app = window.app_handle().clone();
                    api.prevent_close();
                    tauri::async_runtime::spawn(async move {
                        if let Some(c) = bridge.client.read().await.clone() {
                            let _ = c.call("shutdown", Value::Null).await;
                        }
                        app.exit(0);
                    });
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building YAD");

    app.run(|app, event| {
        #[cfg(target_os = "macos")]
        if let RunEvent::Reopen { .. } = event {
            show_main_window(app);
        }
        let _ = (app, &event);
    });
}
