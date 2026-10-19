use std::{path::PathBuf, sync::Arc};
use tauri::{Emitter, Manager};
use voxloom_core::service::Service;

#[tauri::command]
async fn dispatch(
    app: tauri::AppHandle,
    service: tauri::State<'_, Arc<Service>>,
    action: String,
    payload: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    if action == "quit" {
        service.shutdown();
        app.exit(0);
        return Ok(serde_json::Value::Null);
    }
    if action == "window_mode" {
        let compact = payload
            .as_ref()
            .and_then(|p| p["compact"].as_bool())
            .unwrap_or(false);
        if let Some(main) = app.get_webview_window("main") {
            main.emit("window-mode", compact)
                .map_err(|e| e.to_string())?;
            if compact {
                // See-through only while floating, so the rounded bar has no square corners.
                main.set_background_color(Some(tauri::window::Color(0, 0, 0, 0)))
                    .map_err(|e| e.to_string())?;
                main.set_resizable(true).map_err(|e| e.to_string())?;
                main.set_min_size(Some(tauri::LogicalSize::new(390., 92.)))
                    .map_err(|e| e.to_string())?;
                main.set_max_size(Some(tauri::LogicalSize::new(390., 92.)))
                    .map_err(|e| e.to_string())?;
                main.set_size(tauri::LogicalSize::new(390., 92.))
                    .map_err(|e| e.to_string())?;
                main.set_decorations(false).map_err(|e| e.to_string())?;
                main.set_always_on_top(true).map_err(|e| e.to_string())?;
                main.set_title("VoxLoom · Recorder")
                    .map_err(|e| e.to_string())?;
            } else {
                main.set_background_color(opaque_background(&app))
                    .map_err(|e| e.to_string())?;
                main.set_max_size(None::<tauri::LogicalSize<f64>>)
                    .map_err(|e| e.to_string())?;
                main.set_resizable(true).map_err(|e| e.to_string())?;
                main.set_decorations(true).map_err(|e| e.to_string())?;
                main.set_size(tauri::LogicalSize::new(1240., 820.))
                    .map_err(|e| e.to_string())?;
                main.set_min_size(Some(tauri::LogicalSize::new(900., 640.)))
                    .map_err(|e| e.to_string())?;
                main.set_always_on_top(false).map_err(|e| e.to_string())?;
                main.set_title("VoxLoom").map_err(|e| e.to_string())?;
                let _ = main.set_focus();
                let _ = main.emit("open-studio", ());
            }
            main.emit("window-mode", compact)
                .map_err(|e| e.to_string())?;
        }
        return Ok(serde_json::Value::Null);
    }
    let s = service.inner().clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        s.dispatch(&action, payload.unwrap_or(serde_json::Value::Null))
    })
    .await
    .map_err(|e| e.to_string())??;
    // Grant only exact, host-owned recording paths to the media protocol.
    if let Some(tracks) = result["tracks"].as_array() {
        for track in tracks {
            if let Some(path) = track["path"].as_str() {
                app.asset_protocol_scope()
                    .allow_file(path)
                    .map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(result)
}
/// The window is created transparent for the floating recorder, which leaves the
/// webview see-through until the page paints. The studio keeps the configured colour.
fn opaque_background(app: &tauri::AppHandle) -> Option<tauri::window::Color> {
    app.config().app.windows.first().and_then(|w| w.background_color)
}
fn main() {
    // Private files from every Rust/Python/FFmpeg process, including WAL and temp.
    unsafe {
        libc::umask(0o077);
    }
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .ok_or("HOME is not set")?;
            let data = std::env::var_os("VOXLOOM_DATA_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    std::env::var_os("XDG_DATA_HOME")
                        .map(PathBuf::from)
                        .unwrap_or_else(|| home.join(".local/share"))
                        .join("voxloom")
                });
            let cache = std::env::var_os("VOXLOOM_CACHE_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    std::env::var_os("XDG_CACHE_HOME")
                        .map(PathBuf::from)
                        .unwrap_or_else(|| home.join(".cache"))
                        .join("voxloom")
                });
            let resources = if cfg!(debug_assertions) {
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .parent()
                    .unwrap()
                    .to_path_buf()
            } else {
                app.path().resource_dir()?
            };
            let handle = app.handle().clone();
            let notify = Arc::new(move |event| {
                let _ = handle.emit("voxloom", event);
            });
            let service =
                Service::open(data, cache, resources, notify).map_err(std::io::Error::other)?;
            app.manage(service);
            if let Some(main) = app.get_webview_window("main") {
                main.set_background_color(opaque_background(app.handle()))?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![dispatch])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                if window.label() == "mini" {
                    let _ = window.hide();
                    if let Some(main) = window.app_handle().get_webview_window("main") {
                        let _ = main.show();
                        let _ = main.set_focus();
                    }
                } else {
                    window.state::<Arc<Service>>().shutdown();
                    window.app_handle().exit(0);
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("VoxLoom startup failed");
    app.run(|handle, event| {
        if matches!(
            event,
            tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
        ) {
            handle.state::<Arc<Service>>().shutdown();
        }
    });
}
