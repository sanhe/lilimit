use std::{env, fs, path::PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    AppHandle, LogicalSize, Manager, PhysicalPosition, Position, Runtime, Size, WebviewWindow,
    WindowEvent,
};
use tauri_plugin_global_shortcut::ShortcutState;

const SHOW_WIDGET_ID: &str = "show_widget";
const HIDE_WIDGET_ID: &str = "hide_widget";
const QUIT_ID: &str = "quit";
const TOGGLE_SHORTCUT: &str = "CommandOrControl+Shift+L";
const SETTINGS_FILE: &str = "settings.json";
const SIMPLE_WIDTH: f64 = 280.0;
const SIMPLE_HEIGHT: f64 = 140.0;
const FULL_WIDTH: f64 = 360.0;
const FULL_HEIGHT: f64 = 560.0;

#[derive(Debug, Clone, Copy)]
enum SnapshotSource {
    CodexBarWidget,
    LilimitSample,
}

impl SnapshotSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::CodexBarWidget => "codexBarWidget",
            Self::LilimitSample => "lilimitSample",
        }
    }
}

#[derive(Debug)]
struct SnapshotCandidate {
    path: PathBuf,
    source: SnapshotSource,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum DisplayMode {
    Simple,
    Full,
}

fn default_display_mode() -> DisplayMode {
    DisplayMode::Simple
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum WidgetBackground {
    Dark,
    Light,
}

fn default_background() -> WidgetBackground {
    WidgetBackground::Dark
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct WindowPosition {
    x: i32,
    y: i32,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct WidgetSettings {
    #[serde(default = "default_display_mode")]
    display_mode: DisplayMode,
    #[serde(default = "default_background")]
    background: WidgetBackground,
    #[serde(default)]
    window_position: Option<WindowPosition>,
}

impl Default for WidgetSettings {
    fn default() -> Self {
        Self {
            display_mode: DisplayMode::Simple,
            background: WidgetBackground::Dark,
            window_position: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct UsageRow {
    id: String,
    title: String,
    #[serde(default)]
    percent_left: Option<f64>,
    #[serde(default)]
    reset_text: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct TokenUsageSummary {
    #[serde(default, rename = "sessionCostUSD")]
    session_cost_usd: Option<f64>,
    #[serde(default)]
    session_tokens: Option<i64>,
    #[serde(default, rename = "last30DaysCostUSD")]
    last30_days_cost_usd: Option<f64>,
    #[serde(default)]
    last30_days_tokens: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct DailyUsagePoint {
    day_key: String,
    #[serde(default)]
    total_tokens: Option<i64>,
    #[serde(default, rename = "costUSD")]
    cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct RateWindowDetail {
    used_percent: Option<f64>,
    percent_left: Option<f64>,
    window_minutes: Option<f64>,
    resets_at: Option<String>,
    reset_description: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderUsage {
    name: String,
    #[serde(default)]
    session_left_percent: Option<f64>,
    #[serde(default)]
    weekly_left_percent: Option<f64>,
    #[serde(default)]
    reset_text: String,
    #[serde(default)]
    updated_at: Option<String>,
    #[serde(default)]
    usage_rows: Vec<UsageRow>,
    #[serde(default)]
    primary: Option<RateWindowDetail>,
    #[serde(default)]
    secondary: Option<RateWindowDetail>,
    #[serde(default)]
    tertiary: Option<RateWindowDetail>,
    #[serde(default)]
    credits_remaining: Option<f64>,
    #[serde(default)]
    code_review_remaining_percent: Option<f64>,
    #[serde(default)]
    token_usage: Option<TokenUsageSummary>,
    #[serde(default)]
    daily_usage: Vec<DailyUsagePoint>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UsageSnapshotFile {
    updated_at: String,
    providers: Vec<ProviderUsage>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodexBarWidgetSnapshot {
    entries: Vec<CodexBarProviderEntry>,
    generated_at: String,
    #[serde(default)]
    shows_used_percent: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodexBarProviderEntry {
    provider: String,
    #[serde(default)]
    updated_at: Option<String>,
    #[serde(default)]
    primary: Option<CodexBarRateWindow>,
    #[serde(default)]
    secondary: Option<CodexBarRateWindow>,
    #[serde(default)]
    tertiary: Option<CodexBarRateWindow>,
    #[serde(default)]
    usage_rows: Option<Vec<UsageRow>>,
    #[serde(default)]
    credits_remaining: Option<f64>,
    #[serde(default)]
    code_review_remaining_percent: Option<f64>,
    #[serde(default)]
    token_usage: Option<TokenUsageSummary>,
    #[serde(default)]
    daily_usage: Vec<DailyUsagePoint>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodexBarRateWindow {
    #[serde(default)]
    used_percent: Option<f64>,
    #[serde(default)]
    window_minutes: Option<f64>,
    #[serde(default)]
    resets_at: Option<String>,
    #[serde(default)]
    reset_description: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UsageSnapshotResponse {
    status: String,
    path: String,
    source: Option<String>,
    updated_at: Option<String>,
    shows_used_percent: bool,
    providers: Vec<ProviderUsage>,
    error: Option<String>,
}

fn usage_snapshot_candidates() -> Result<Vec<SnapshotCandidate>, String> {
    let home = env::var_os("HOME").ok_or_else(|| "HOME is not set".to_string())?;
    let home = PathBuf::from(home);

    #[cfg(target_os = "macos")]
    {
        let group_containers = home.join("Library").join("Group Containers");
        let application_support = home.join("Library").join("Application Support");

        return Ok(vec![
            SnapshotCandidate {
                path: group_containers
                    .join("Y5PE65HELJ.com.steipete.codexbar")
                    .join("widget-snapshot.json"),
                source: SnapshotSource::CodexBarWidget,
            },
            SnapshotCandidate {
                path: group_containers
                    .join("Y5PE65HELJ.com.steipete.codexbar.debug")
                    .join("widget-snapshot.json"),
                source: SnapshotSource::CodexBarWidget,
            },
            SnapshotCandidate {
                path: group_containers
                    .join("group.com.steipete.codexbar")
                    .join("widget-snapshot.json"),
                source: SnapshotSource::CodexBarWidget,
            },
            SnapshotCandidate {
                path: group_containers
                    .join("group.com.steipete.codexbar.debug")
                    .join("widget-snapshot.json"),
                source: SnapshotSource::CodexBarWidget,
            },
            SnapshotCandidate {
                path: application_support
                    .join("CodexBar")
                    .join("widget-snapshot.json"),
                source: SnapshotSource::CodexBarWidget,
            },
            SnapshotCandidate {
                path: application_support
                    .join("lilimit")
                    .join("usage_snapshot.json"),
                source: SnapshotSource::LilimitSample,
            },
        ]);
    }

    #[cfg(target_os = "linux")]
    {
        return Ok(vec![
            SnapshotCandidate {
                path: home
                    .join(".local")
                    .join("share")
                    .join("CodexBar")
                    .join("widget-snapshot.json"),
                source: SnapshotSource::CodexBarWidget,
            },
            SnapshotCandidate {
                path: home
                    .join(".config")
                    .join("CodexBar")
                    .join("widget-snapshot.json"),
                source: SnapshotSource::CodexBarWidget,
            },
            SnapshotCandidate {
                path: home
                    .join(".config")
                    .join("codexbar")
                    .join("widget-snapshot.json"),
                source: SnapshotSource::CodexBarWidget,
            },
            SnapshotCandidate {
                path: home
                    .join(".config")
                    .join("lilimit")
                    .join("usage_snapshot.json"),
                source: SnapshotSource::LilimitSample,
            },
        ]);
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Ok(vec![SnapshotCandidate {
            path: home
                .join(".config")
                .join("lilimit")
                .join("usage_snapshot.json"),
            source: SnapshotSource::LilimitSample,
        }])
    }
}

fn lilimit_config_dir() -> Result<PathBuf, String> {
    let home = env::var_os("HOME").ok_or_else(|| "HOME is not set".to_string())?;
    let home = PathBuf::from(home);

    #[cfg(target_os = "macos")]
    {
        return Ok(home
            .join("Library")
            .join("Application Support")
            .join("lilimit"));
    }

    #[cfg(target_os = "linux")]
    {
        return Ok(home.join(".config").join("lilimit"));
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Ok(home.join(".config").join("lilimit"))
    }
}

fn widget_settings_path() -> Result<PathBuf, String> {
    Ok(lilimit_config_dir()?.join(SETTINGS_FILE))
}

fn read_widget_settings() -> Result<WidgetSettings, String> {
    let path = widget_settings_path()?;
    let contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(WidgetSettings::default());
        }
        Err(error) => return Err(error.to_string()),
    };

    serde_json::from_str::<WidgetSettings>(&contents).map_err(|error| error.to_string())
}

fn write_widget_settings(settings: &WidgetSettings) -> Result<(), String> {
    let path = widget_settings_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    let contents = serde_json::to_string_pretty(settings).map_err(|error| error.to_string())?;
    fs::write(path, contents).map_err(|error| error.to_string())
}

#[tauri::command]
fn get_widget_settings() -> WidgetSettings {
    read_widget_settings().unwrap_or_default()
}

#[tauri::command]
fn save_widget_settings(
    settings: WidgetSettings,
    app: AppHandle,
) -> Result<WidgetSettings, String> {
    let mut next = settings;
    if next.window_position.is_none() {
        if let Ok(existing) = read_widget_settings() {
            next.window_position = existing.window_position;
        }
    }

    write_widget_settings(&next)?;

    if let Some(window) = app.get_webview_window("main") {
        apply_window_settings(&window, &next);
    }

    Ok(next)
}

fn persist_window_position(x: i32, y: i32) -> Result<(), String> {
    let mut settings = read_widget_settings().unwrap_or_default();
    settings.window_position = Some(WindowPosition { x, y });
    write_widget_settings(&settings)
}

fn window_size(display_mode: DisplayMode) -> (f64, f64) {
    match display_mode {
        DisplayMode::Simple => (SIMPLE_WIDTH, SIMPLE_HEIGHT),
        DisplayMode::Full => (FULL_WIDTH, FULL_HEIGHT),
    }
}

fn apply_window_settings<R: Runtime>(window: &WebviewWindow<R>, settings: &WidgetSettings) {
    let (width, height) = window_size(settings.display_mode);
    let _ = window.set_size(Size::Logical(LogicalSize::new(width, height)));

    if let Some(position) = settings.window_position {
        // Tauri reports physical coordinates. They can be negative on macOS or
        // Linux multi-monitor layouts, so store and replay the raw values.
        let _ = window.set_position(Position::Physical(PhysicalPosition::new(
            position.x, position.y,
        )));
    }
}

#[tauri::command]
fn get_usage_snapshot() -> UsageSnapshotResponse {
    let candidates = match usage_snapshot_candidates() {
        Ok(candidates) => candidates,
        Err(error) => {
            return UsageSnapshotResponse {
                status: "ioError".to_string(),
                path: String::new(),
                source: None,
                updated_at: None,
                shows_used_percent: false,
                providers: Vec::new(),
                error: Some(error),
            }
        }
    };

    let missing_path = candidates
        .first()
        .map(|candidate| candidate.path.to_string_lossy().into_owned())
        .unwrap_or_default();

    for candidate in candidates {
        let path_text = candidate.path.to_string_lossy().into_owned();

        let contents = match fs::read_to_string(&candidate.path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return UsageSnapshotResponse {
                    status: "ioError".to_string(),
                    path: path_text,
                    source: Some(candidate.source.as_str().to_string()),
                    updated_at: None,
                    shows_used_percent: false,
                    providers: Vec::new(),
                    error: Some(error.to_string()),
                }
            }
        };

        return match candidate.source {
            SnapshotSource::CodexBarWidget => parse_codexbar_widget_snapshot(&contents, &path_text),
            SnapshotSource::LilimitSample => parse_lilimit_snapshot(&contents, &path_text),
        };
    }

    UsageSnapshotResponse {
        status: "missing".to_string(),
        path: missing_path,
        source: None,
        updated_at: None,
        shows_used_percent: false,
        providers: Vec::new(),
        error: None,
    }
}

fn parse_lilimit_snapshot(contents: &str, path_text: &str) -> UsageSnapshotResponse {
    match serde_json::from_str::<UsageSnapshotFile>(&contents) {
        Ok(snapshot) => UsageSnapshotResponse {
            status: "ready".to_string(),
            path: path_text.to_string(),
            source: Some(SnapshotSource::LilimitSample.as_str().to_string()),
            updated_at: Some(snapshot.updated_at),
            shows_used_percent: false,
            providers: snapshot
                .providers
                .into_iter()
                .map(normalize_lilimit_provider)
                .collect(),
            error: None,
        },
        Err(error) => UsageSnapshotResponse {
            status: "invalidJson".to_string(),
            path: path_text.to_string(),
            source: Some(SnapshotSource::LilimitSample.as_str().to_string()),
            updated_at: None,
            shows_used_percent: false,
            providers: Vec::new(),
            error: Some(error.to_string()),
        },
    }
}

fn parse_codexbar_widget_snapshot(contents: &str, path_text: &str) -> UsageSnapshotResponse {
    match serde_json::from_str::<CodexBarWidgetSnapshot>(contents) {
        Ok(snapshot) => UsageSnapshotResponse {
            status: "ready".to_string(),
            path: path_text.to_string(),
            source: Some(SnapshotSource::CodexBarWidget.as_str().to_string()),
            updated_at: Some(snapshot.generated_at.clone()),
            shows_used_percent: snapshot.shows_used_percent,
            providers: codexbar_widget_providers(snapshot),
            error: None,
        },
        Err(error) => UsageSnapshotResponse {
            status: "invalidJson".to_string(),
            path: path_text.to_string(),
            source: Some(SnapshotSource::CodexBarWidget.as_str().to_string()),
            updated_at: None,
            shows_used_percent: false,
            providers: Vec::new(),
            error: Some(error.to_string()),
        },
    }
}

fn normalize_lilimit_provider(mut provider: ProviderUsage) -> ProviderUsage {
    provider.session_left_percent = provider.session_left_percent.map(clamp_percent);
    provider.weekly_left_percent = provider.weekly_left_percent.map(clamp_percent);

    if provider.usage_rows.is_empty() {
        if let Some(percent_left) = provider.session_left_percent {
            provider.usage_rows.push(UsageRow {
                id: "session".to_string(),
                title: "Session".to_string(),
                percent_left: Some(percent_left),
                reset_text: Some(provider.reset_text.clone()).filter(|value| !value.is_empty()),
            });
        }

        if let Some(percent_left) = provider.weekly_left_percent {
            provider.usage_rows.push(UsageRow {
                id: "weekly".to_string(),
                title: "Weekly".to_string(),
                percent_left: Some(percent_left),
                reset_text: None,
            });
        }
    }

    provider
}

fn codexbar_widget_providers(snapshot: CodexBarWidgetSnapshot) -> Vec<ProviderUsage> {
    snapshot
        .entries
        .into_iter()
        .filter_map(|entry| {
            let name = match entry.provider.as_str() {
                "codex" => "Codex",
                "claude" => "Claude",
                _ => return None,
            };

            Some(ProviderUsage {
                name: name.to_string(),
                session_left_percent: codexbar_row_percent(&entry, &["session", "primary"])
                    .or_else(|| percent_left_from_window(entry.primary.as_ref())),
                weekly_left_percent: codexbar_row_percent(&entry, &["weekly", "secondary"])
                    .or_else(|| percent_left_from_window(entry.secondary.as_ref())),
                reset_text: codexbar_reset_text(&entry),
                updated_at: entry.updated_at.clone(),
                usage_rows: codexbar_usage_rows(&entry),
                primary: rate_window_detail(entry.primary.as_ref()),
                secondary: rate_window_detail(entry.secondary.as_ref()),
                tertiary: rate_window_detail(entry.tertiary.as_ref()),
                credits_remaining: entry.credits_remaining,
                code_review_remaining_percent: entry
                    .code_review_remaining_percent
                    .map(clamp_percent),
                token_usage: entry.token_usage.clone(),
                daily_usage: entry.daily_usage.clone(),
            })
        })
        .collect()
}

fn codexbar_usage_rows(entry: &CodexBarProviderEntry) -> Vec<UsageRow> {
    if let Some(rows) = entry.usage_rows.as_ref() {
        return rows
            .iter()
            .map(|row| UsageRow {
                id: row.id.clone(),
                title: row.title.clone(),
                percent_left: row.percent_left.map(clamp_percent),
                reset_text: codexbar_usage_row_reset_text(&row.id, entry),
            })
            .collect();
    }

    let mut rows = Vec::new();
    if let Some(primary) = entry.primary.as_ref() {
        rows.push(UsageRow {
            id: "primary".to_string(),
            title: "Session".to_string(),
            percent_left: percent_left_from_window(Some(primary)),
            reset_text: rate_window_reset_text(primary),
        });
    }
    if let Some(secondary) = entry.secondary.as_ref() {
        rows.push(UsageRow {
            id: "secondary".to_string(),
            title: "Weekly".to_string(),
            percent_left: percent_left_from_window(Some(secondary)),
            reset_text: rate_window_reset_text(secondary),
        });
    }
    if let Some(tertiary) = entry.tertiary.as_ref() {
        rows.push(UsageRow {
            id: "tertiary".to_string(),
            title: "Extra".to_string(),
            percent_left: percent_left_from_window(Some(tertiary)),
            reset_text: rate_window_reset_text(tertiary),
        });
    }
    rows
}

fn codexbar_usage_row_reset_text(id: &str, entry: &CodexBarProviderEntry) -> Option<String> {
    match id {
        "session" | "primary" => entry.primary.as_ref().and_then(rate_window_reset_text),
        "weekly" | "secondary" => entry.secondary.as_ref().and_then(rate_window_reset_text),
        "opus" | "tertiary" => entry.tertiary.as_ref().and_then(rate_window_reset_text),
        _ => None,
    }
}

fn codexbar_row_percent(entry: &CodexBarProviderEntry, ids: &[&str]) -> Option<f64> {
    let rows = entry.usage_rows.as_ref()?;
    rows.iter()
        .find(|row| {
            let id = row.id.to_ascii_lowercase();
            ids.iter().any(|candidate| id == *candidate)
        })
        .and_then(|row| row.percent_left)
        .or_else(|| {
            rows.iter()
                .find(|row| {
                    let title = row.title.to_ascii_lowercase();
                    ids.iter().any(|candidate| title.contains(candidate))
                })
                .and_then(|row| row.percent_left)
        })
        .map(clamp_percent)
}

fn percent_left_from_window(window: Option<&CodexBarRateWindow>) -> Option<f64> {
    window
        .and_then(|window| window.used_percent)
        .map(|used| clamp_percent(100.0 - used))
}

fn rate_window_detail(window: Option<&CodexBarRateWindow>) -> Option<RateWindowDetail> {
    window.map(|window| RateWindowDetail {
        used_percent: window.used_percent.map(clamp_percent),
        percent_left: percent_left_from_window(Some(window)),
        window_minutes: window.window_minutes,
        resets_at: window.resets_at.clone(),
        reset_description: window.reset_description.clone(),
    })
}

fn rate_window_reset_text(window: &CodexBarRateWindow) -> Option<String> {
    window
        .reset_description
        .clone()
        .filter(|value| !value.is_empty())
        .or_else(|| window.resets_at.clone())
}

fn codexbar_reset_text(entry: &CodexBarProviderEntry) -> String {
    entry
        .primary
        .as_ref()
        .and_then(|window| window.reset_description.clone())
        .or_else(|| {
            entry
                .secondary
                .as_ref()
                .and_then(|window| window.reset_description.clone())
        })
        .unwrap_or_default()
}

fn clamp_percent(value: f64) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }
    value.clamp(0.0, 100.0)
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
            let settings = read_widget_settings().unwrap_or_default();
            if let Some(window) = app.get_webview_window("main") {
                apply_window_settings(&window, &settings);
            }
            setup_tray(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let WindowEvent::Moved(position) = event {
                    // macOS and X11 usually report move events promptly. Some
                    // Wayland compositors may delay or limit programmatic
                    // positioning; persisting the last reported value is still
                    // the best local-only behavior Tauri exposes.
                    let _ = persist_window_position(position.x, position.y);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_usage_snapshot,
            get_widget_settings,
            save_widget_settings
        ])
        .run(tauri::generate_context!())
        .expect("failed to run lilimit");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_codexbar_widget_snapshot_for_codex_and_claude() {
        let json = r#"{
          "entries": [
            {
              "provider": "codex",
              "updatedAt": "2026-05-18T21:14:44Z",
              "usageRows": [
                { "id": "session", "title": "Session", "percentLeft": 70 },
                { "id": "weekly", "title": "Weekly", "percentLeft": 83 }
              ],
              "primary": { "usedPercent": 30, "resetDescription": "23:25" },
              "secondary": { "usedPercent": 17, "resetDescription": "May 24" },
              "tokenUsage": {
                "sessionCostUSD": 1.25,
                "sessionTokens": 1234567,
                "last30DaysCostUSD": 40.5,
                "last30DaysTokens": 98765432
              },
              "dailyUsage": [
                { "dayKey": "2026-05-18", "costUSD": 1.25, "totalTokens": 1234567 }
              ]
            },
            {
              "provider": "claude",
              "updatedAt": "2026-05-18T21:14:45Z",
              "usageRows": [
                { "id": "primary", "title": "Session", "percentLeft": 24 },
                { "id": "secondary", "title": "Weekly", "percentLeft": 80 }
              ],
              "primary": { "usedPercent": 76, "resetDescription": "May 19 at 2:50AM" },
              "secondary": { "usedPercent": 20, "resetDescription": "May 22 at 11:00PM" },
              "dailyUsage": []
            }
          ],
          "enabledProviders": ["codex", "claude"],
          "generatedAt": "2026-05-18T21:14:45Z",
          "showsUsedPercent": false
        }"#;

        let response = parse_codexbar_widget_snapshot(json, "/tmp/widget-snapshot.json");

        assert_eq!(response.status, "ready");
        assert_eq!(response.source.as_deref(), Some("codexBarWidget"));
        assert_eq!(response.updated_at.as_deref(), Some("2026-05-18T21:14:45Z"));
        assert!(!response.shows_used_percent);
        assert_eq!(response.providers.len(), 2);
        assert_eq!(response.providers[0].name, "Codex");
        assert_eq!(response.providers[0].session_left_percent, Some(70.0));
        assert_eq!(response.providers[0].weekly_left_percent, Some(83.0));
        assert_eq!(response.providers[0].reset_text, "23:25");
        assert_eq!(response.providers[0].usage_rows.len(), 2);
        assert_eq!(
            response.providers[0].usage_rows[0].reset_text.as_deref(),
            Some("23:25")
        );
        assert_eq!(
            response.providers[0]
                .token_usage
                .as_ref()
                .and_then(|usage| usage.session_tokens),
            Some(1234567)
        );
        assert_eq!(response.providers[0].daily_usage.len(), 1);
        assert_eq!(response.providers[0].daily_usage[0].cost_usd, Some(1.25));
        assert_eq!(response.providers[1].name, "Claude");
        assert_eq!(response.providers[1].session_left_percent, Some(24.0));
        assert_eq!(response.providers[1].weekly_left_percent, Some(80.0));
    }

    #[test]
    fn falls_back_to_rate_windows_when_usage_rows_are_missing() {
        let json = r#"{
          "entries": [
            {
              "provider": "codex",
              "updatedAt": "2026-05-18T21:14:44Z",
              "primary": { "usedPercent": 30, "resetDescription": "23:25" },
              "secondary": { "usedPercent": 17, "resetDescription": "May 24" },
              "dailyUsage": []
            }
          ],
          "generatedAt": "2026-05-18T21:14:45Z"
        }"#;

        let response = parse_codexbar_widget_snapshot(json, "/tmp/widget-snapshot.json");

        assert_eq!(response.providers[0].session_left_percent, Some(70.0));
        assert_eq!(response.providers[0].weekly_left_percent, Some(83.0));
    }
}
