use tauri::{
    AppHandle,
    menu::{MenuBuilder, SubmenuBuilder},
    tray::TrayIconBuilder,
    utils::config::Color,
    Emitter, Manager,
};

const MODE_LEISURE: &str = "mode_leisure";
const MODE_PATROL: &str = "mode_patrol";
const MODE_INTERACTIVE: &str = "mode_interactive";
const APP_QUIT: &str = "app_quit";

fn append_log(app: &AppHandle, message: &str) -> Result<String, String> {
    let log_dir = app
        .path()
        .app_log_dir()
        .map_err(|error| format!("resolve app log dir failed: {error}"))?;
    std::fs::create_dir_all(&log_dir)
        .map_err(|error| format!("create app log dir failed: {error}"))?;

    let log_path = log_dir.join("desktop-pet.log");
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or_default();
    let line = format!("[{timestamp:.3}] {message}\n");

    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|error| format!("open log file failed: {error}"))?;
    file.write_all(line.as_bytes())
        .map_err(|error| format!("write log failed: {error}"))?;

    Ok(log_path.display().to_string())
}

#[tauri::command]
fn write_frontend_log(app: AppHandle, message: String) -> Result<String, String> {
    append_log(&app, &format!("[frontend] {message}"))
}

#[cfg(target_os = "macos")]
fn set_visible_on_all_spaces(app: &AppHandle, window: &tauri::WebviewWindow) {
    use objc2_app_kit::{NSWindow, NSWindowCollectionBehavior};

    match window.ns_window() {
        Ok(ns_window) => unsafe {
            let ns_window = &*(ns_window.cast::<NSWindow>());
            let behavior = ns_window.collectionBehavior()
                | NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::IgnoresCycle;
            ns_window.setCollectionBehavior(behavior);
            let _ = append_log(
                app,
                &format!(
                    "[rust] set all-spaces collection behavior ok: {}",
                    behavior.bits()
                ),
            );
        },
        Err(error) => {
            let _ = append_log(
                app,
                &format!("[rust] set all-spaces collection behavior failed: {error}"),
            );
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn set_visible_on_all_spaces(_app: &AppHandle, _window: &tauri::WebviewWindow) {}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![write_frontend_log])
        .setup(|app| {
            let _ = append_log(app.handle(), "[rust] setup start");
            let mode_menu = SubmenuBuilder::new(app, "模式选择")
                .text(MODE_LEISURE, "休闲模式")
                .text(MODE_PATROL, "巡回模式")
                .text(MODE_INTERACTIVE, "交互模式")
                .build()?;
            let tray_menu = MenuBuilder::new(app)
                .item(&mode_menu)
                .text(APP_QUIT, "退出")
                .build()?;

            TrayIconBuilder::with_id("pet-mode")
                .tooltip("Juboss Desktop Pet")
                .icon(
                    app.default_window_icon()
                        .cloned()
                        .expect("missing default window icon"),
                )
                .menu(&tray_menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| {
                    if event.id().as_ref() == APP_QUIT {
                        app.exit(0);
                        return;
                    }

                    let mode = match event.id().as_ref() {
                        MODE_LEISURE => Some("leisure"),
                        MODE_PATROL => Some("patrol"),
                        MODE_INTERACTIVE => Some("interactive"),
                        _ => None,
                    };

                    if let Some(mode) = mode {
                        let _ = app.emit("pet://mode", mode);
                    }
                })
                .build(app)?;

            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_always_on_top(true);
                let _ = window.set_decorations(false);
                let _ = window.set_shadow(false);
                match window.set_background_color(Some(Color(0, 0, 0, 0))) {
                    Ok(()) => {
                        let _ = append_log(app.handle(), "[rust] set background transparent ok");
                    }
                    Err(error) => {
                        let _ = append_log(
                            app.handle(),
                            &format!("[rust] set background transparent failed: {error}"),
                        );
                    }
                }
                set_visible_on_all_spaces(app.handle(), &window);
                let _ = append_log(app.handle(), "[rust] window transparent=true configured");
            }

            if let Ok(path) = append_log(app.handle(), "[rust] setup complete") {
                println!("desktop pet log: {path}");
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running juboss desktop pet");
}
