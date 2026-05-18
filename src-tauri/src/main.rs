use std::{env, fs, path::PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    Manager,
};
use tauri_plugin_global_shortcut::ShortcutState;

const SHOW_WIDGET_ID: &str = "show_widget";
const HIDE_WIDGET_ID: &str = "hide_widget";
const QUIT_ID: &str = "quit";
const TOGGLE_SHORTCUT: &str = "CommandOrControl+Shift+L";

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderUsage {
    name: String,
    session_left_percent: u8,
    weekly_left_percent: u8,
    reset_text: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UsageSnapshotFile {
    updated_at: String,
    providers: Vec<ProviderUsage>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UsageSnapshotResponse {
    status: String,
    path: String,
    updated_at: Option<String>,
    providers: Vec<ProviderUsage>,
    error: Option<String>,
}

fn usage_snapshot_path() -> Result<PathBuf, String> {
    let home = env::var_os("HOME").ok_or_else(|| "HOME is not set".to_string())?;
    let home = PathBuf::from(home);

    // macOS stores per-user app support files under Library/Application Support.
    #[cfg(target_os = "macos")]
    {
        return Ok(home
            .join("Library")
            .join("Application Support")
            .join("lilimit")
            .join("usage_snapshot.json"));
    }

    // Ubuntu and other Linux desktops conventionally use XDG config under
    // ~/.config. GNOME Wayland/X11 window behavior differs, but the data path
    // is stable for this local JSON reader.
    #[cfg(target_os = "linux")]
    {
        return Ok(home
            .join(".config")
            .join("lilimit")
            .join("usage_snapshot.json"));
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Ok(home
            .join(".config")
            .join("lilimit")
            .join("usage_snapshot.json"))
    }
}

#[tauri::command]
fn get_usage_snapshot() -> UsageSnapshotResponse {
    let path = match usage_snapshot_path() {
        Ok(path) => path,
        Err(error) => {
            return UsageSnapshotResponse {
                status: "ioError".to_string(),
                path: String::new(),
                updated_at: None,
                providers: Vec::new(),
                error: Some(error),
            }
        }
    };

    let path_text = path.to_string_lossy().into_owned();

    let contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return UsageSnapshotResponse {
                status: "missing".to_string(),
                path: path_text,
                updated_at: None,
                providers: Vec::new(),
                error: None,
            }
        }
        Err(error) => {
            return UsageSnapshotResponse {
                status: "ioError".to_string(),
                path: path_text,
                updated_at: None,
                providers: Vec::new(),
                error: Some(error.to_string()),
            }
        }
    };

    match serde_json::from_str::<UsageSnapshotFile>(&contents) {
        Ok(snapshot) => UsageSnapshotResponse {
            status: "ready".to_string(),
            path: path_text,
            updated_at: Some(snapshot.updated_at),
            providers: snapshot.providers,
            error: None,
        },
        Err(error) => UsageSnapshotResponse {
            status: "invalidJson".to_string(),
            path: path_text,
            updated_at: None,
            providers: Vec::new(),
            error: Some(error.to_string()),
        },
    }
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn hide_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
}

fn toggle_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        match window.is_visible() {
            Ok(true) => {
                let _ = window.hide();
            }
            Ok(false) | Err(_) => {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }
    }
}

fn setup_tray(app: &mut tauri::App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, SHOW_WIDGET_ID, "Show Widget", true, None::<&str>)?;
    let hide = MenuItem::with_id(app, HIDE_WIDGET_ID, "Hide Widget", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, QUIT_ID, "Quit lilimit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &hide, &separator, &quit])?;

    // macOS shows this as a menu-bar status item. GNOME support depends on the
    // shell/session: X11 and AppIndicator-capable setups are more predictable
    // than stock GNOME Wayland.
    TrayIconBuilder::with_id("lilimit")
        .icon(tauri::include_image!("./icons/tray-template.png"))
        .icon_as_template(true)
        .tooltip("lilimit")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| {
            if event.id() == SHOW_WIDGET_ID {
                show_main_window(app);
            } else if event.id() == HIDE_WIDGET_ID {
                hide_main_window(app);
            } else if event.id() == QUIT_ID {
                app.exit(0);
            }
        })
        .build(app)?;

    Ok(())
}

fn main() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_shortcuts([TOGGLE_SHORTCUT])
                .expect("failed to configure lilimit shortcut")
                .with_handler(|app, _shortcut, event| {
                    if event.state == ShortcutState::Pressed {
                        toggle_main_window(app);
                    }
                })
                .build(),
        )
        .setup(|app| {
            // Registers Cmd+Shift+L on macOS and Ctrl+Shift+L on Linux.
            // Ubuntu GNOME Wayland may restrict global shortcuts at the
            // compositor level; X11 sessions are generally more predictable.
            setup_tray(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![get_usage_snapshot])
        .run(tauri::generate_context!())
        .expect("failed to run lilimit");
}
