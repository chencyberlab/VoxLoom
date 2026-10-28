// Release builds on Windows are GUI apps: no console window behind the app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use tauri::{Emitter, Manager};
use voxloom_core::service::Service;

/// The floating recorder: a bubble that keeps one size whether idle or recording
/// (resizing a see-through, always-on-top window shows the old picture for a
/// frame), and the bar of controls its click opens, as tall as the bubble.
const BUBBLE: (f64, f64) = (64., 64.);
const BAR: (f64, f64) = (200., 64.);
/// The open bar grew leftwards: its bubble sat too near the right edge.
static BAR_LEFT: AtomicBool = AtomicBool::new(false);
/// The window is the floating recorder. Bubble requests that arrive after it
/// became the studio again (a bar's close timer) must not shrink the studio.
static FLOATING: AtomicBool = AtomicBool::new(false);
fn float_size(main: &tauri::WebviewWindow, (width, height): (f64, f64)) -> tauri::Result<()> {
    let size = tauri::LogicalSize::new(width, height);
    main.set_max_size(None::<tauri::LogicalSize<f64>>)?;
    main.set_min_size(Some(size))?;
    main.set_max_size(Some(size))?;
    main.set_size(size)
}
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
    if action == "float_size" || action == "float_move" {
        if !FLOATING.load(Ordering::SeqCst) {
            return Ok(serde_json::Value::Null);
        }
        let main = app.get_webview_window("main").ok_or("The window is not open")?;
        let p = payload.unwrap_or_default();
        if action == "float_move" {
            let (x, y) = (p["x"].as_f64().unwrap_or(0.), p["y"].as_f64().unwrap_or(0.));
            main.set_position(tauri::LogicalPosition::new(x, y))
                .map_err(|e| e.to_string())?;
            return Ok(serde_json::Value::Null);
        }
        let bar = p["bar"].as_bool().unwrap_or(false);
        let scale = main.scale_factor().map_err(|e| e.to_string())?;
        let at = main.outer_position().map_err(|e| e.to_string())?;
        let grow = ((BAR.0 - BUBBLE.0) * scale).round() as i32;
        // The bar opens away from the nearer screen edge, so the bubble's tile stays
        // where it was; closing it moves the window back the same way.
        let left = if bar {
            main.current_monitor().ok().flatten().is_some_and(|m| {
                let area = m.work_area();
                at.x + (BAR.0 * scale).round() as i32 > area.position.x + area.size.width as i32
            })
        } else {
            BAR_LEFT.load(Ordering::SeqCst)
        };
        if left {
            let x = if bar { at.x - grow } else { at.x + grow };
            main.set_position(tauri::PhysicalPosition::new(x, at.y))
                .map_err(|e| e.to_string())?;
        }
        float_size(&main, if bar { BAR } else { BUBBLE }).map_err(|e| e.to_string())?;
        BAR_LEFT.store(bar && left, Ordering::SeqCst);
        return Ok(serde_json::json!({ "left": bar && left }));
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
                BAR_LEFT.store(false, Ordering::SeqCst);
                FLOATING.store(true, Ordering::SeqCst);
                float_size(&main, BUBBLE).map_err(|e| e.to_string())?;
                main.set_decorations(false).map_err(|e| e.to_string())?;
                main.set_always_on_top(true).map_err(|e| e.to_string())?;
                main.set_title("VoxLoom · Recorder")
                    .map_err(|e| e.to_string())?;
            } else {
                FLOATING.store(false, Ordering::SeqCst);
                main.set_background_color(opaque_background(&app))
                    .map_err(|e| e.to_string())?;
                main.set_max_size(None::<tauri::LogicalSize<f64>>)
                    .map_err(|e| e.to_string())?;
                main.set_resizable(true).map_err(|e| e.to_string())?;
                main.set_decorations(true).map_err(|e| e.to_string())?;
                main.set_size(tauri::LogicalSize::new(1240., 820.))
                    .map_err(|e| e.to_string())?;
                // The bubble may sit at a screen edge; the studio opens whole, centred on
                // that screen. center() would use the bubble's size: the resize is pending.
                if let Ok(Some(screen)) = main.current_monitor() {
                    let (area, scale) = (screen.work_area(), screen.scale_factor());
                    let (w, h) = ((1240. * scale) as i32, (820. * scale) as i32);
                    let x = area.position.x + (area.size.width as i32 - w).max(0) / 2;
                    let y = area.position.y + (area.size.height as i32 - h).max(0) / 2;
                    let _ = main.set_position(tauri::PhysicalPosition::new(x, y));
                }
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
    let playback = action == "playback";
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
    if playback {
        if let Some(path) = result["path"].as_str() {
            app.asset_protocol_scope()
                .allow_file(path)
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(result)
}
/// A track's playback file, for a webview that cannot stream it from asset:
/// (WebKitGTK's media player opens only schemes GStreamer knows). Only tracks
/// in the library, looked up by id, as the `playback` action resolves them.
#[tauri::command]
async fn playback_bytes(
    service: tauri::State<'_, Arc<Service>>,
    id: String,
) -> Result<tauri::ipc::Response, String> {
    let s = service.inner().clone();
    let bytes = tauri::async_runtime::spawn_blocking(move || {
        let found = s.dispatch("playback", serde_json::json!({ "id": id }))?;
        std::fs::read(found["path"].as_str().unwrap_or_default()).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(tauri::ipc::Response::new(bytes))
}
/// The window is created transparent for the floating recorder, which leaves the
/// webview see-through until the page paints. The studio keeps the configured colour.
fn opaque_background(app: &tauri::AppHandle) -> Option<tauri::window::Color> {
    app.config().app.windows.first().and_then(|w| w.background_color)
}
fn main() {
    voxloom_core::platform::prefer_x11();
    // Private files from every Rust/Python/FFmpeg process, including WAL and temp.
    voxloom_core::platform::restrict_new_files();
    voxloom_core::process::contain_children();
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let (data, cache) =
                voxloom_core::platform::data_dirs().map_err(std::io::Error::other)?;
            let resources = if cfg!(debug_assertions) {
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .parent()
                    .unwrap()
                    .to_path_buf()
            } else {
                app.path().resource_dir()?
            };
            voxloom_core::platform::use_bundled_tools(&resources);
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
        .invoke_handler(tauri::generate_handler![dispatch, playback_bytes])
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
