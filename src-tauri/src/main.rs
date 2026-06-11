use std::{
    collections::{BTreeMap, HashMap},
    env, fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};
#[cfg(target_os = "macos")]
use std::{
    io::Read,
    process::{Command, Stdio},
    time::Instant,
};

use chrono::{DateTime, Days, Local, NaiveDate, SecondsFormat, Utc};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{
    image::Image,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, Position, Runtime, Size,
    WebviewWindow, WindowEvent,
};
use tauri_plugin_global_shortcut::ShortcutState;

const TOGGLE_WIDGET_ID: &str = "toggle_widget";
const QUIT_ID: &str = "quit";
const TRAY_ID: &str = "lilimit";
const TOGGLE_SHORTCUT: &str = "CommandOrControl+Shift+L";
const SETTINGS_FILE: &str = "settings.json";
const COLLECTED_USAGE_FILE: &str = "collected_snapshot.json";
const COLLECTOR_STATE_FILE: &str = "collector_state.json";
const SIMPLE_WIDTH: f64 = 280.0;
const SIMPLE_HEIGHT: f64 = 140.0;
const FULL_WIDTH: f64 = 360.0;
const FULL_HEIGHT: f64 = 560.0;
const COLLECTION_REFRESH_INTERVAL: Duration = Duration::from_secs(5 * 60);
const WINDOW_POSITION_FLUSH_DELAY: Duration = Duration::from_millis(500);
const CLAUDE_REFRESH_INTERVAL: Duration = Duration::from_secs(5 * 60);
const CLAUDE_RATE_LIMIT_BASE_BACKOFF: Duration = Duration::from_secs(5 * 60);
const CLAUDE_RATE_LIMIT_MAX_BACKOFF: Duration = Duration::from_secs(6 * 60 * 60);
const CODEX_AUTH_REFRESH_AFTER_DAYS: i64 = 8;
const CODEX_REFRESH_ENDPOINT: &str = "https://auth.openai.com/oauth/token";
const CODEX_OAUTH_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const CODEX_DEFAULT_CHATGPT_BASE_URL: &str = "https://chatgpt.com/backend-api";
const CLAUDE_OAUTH_USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
#[cfg(target_os = "macos")]
const CLAUDE_KEYCHAIN_SERVICE: &str = "Claude Code-credentials";
#[cfg(target_os = "macos")]
const CLAUDE_KEYCHAIN_READ_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy)]
enum SnapshotSource {
    LilimitCollected,
}

impl SnapshotSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::LilimitCollected => "lilimitCollected",
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
enum KeychainAccess {
    Off,
    Allow,
}

fn default_keychain_access() -> KeychainAccess {
    KeychainAccess::Off
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum ToolbarDisplay {
    Text,
    Bars,
}

fn default_toolbar_display() -> ToolbarDisplay {
    ToolbarDisplay::Bars
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
    #[serde(default = "default_keychain_access")]
    keychain_access: KeychainAccess,
    #[serde(default = "default_toolbar_display")]
    toolbar_display: ToolbarDisplay,
    #[serde(default)]
    window_position: Option<WindowPosition>,
}

impl Default for WidgetSettings {
    fn default() -> Self {
        Self {
            display_mode: DisplayMode::Simple,
            background: WidgetBackground::Dark,
            keychain_access: KeychainAccess::Off,
            toolbar_display: ToolbarDisplay::Bars,
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
    #[serde(default)]
    top_model: Option<String>,
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
    account_email: Option<String>,
    #[serde(default)]
    plan_text: Option<String>,
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
    #[serde(default)]
    stale: bool,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct UsageSnapshotFile {
    updated_at: String,
    providers: Vec<ProviderUsage>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct CollectorState {
    #[serde(default)]
    claude_next_attempt_at: Option<String>,
    #[serde(default)]
    claude_rate_limit_failures: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UsageSnapshotResponse {
    status: String,
    path: String,
    source: Option<String>,
    updated_at: Option<String>,
    providers: Vec<ProviderUsage>,
    error: Option<String>,
}

fn usage_snapshot_candidate() -> Result<SnapshotCandidate, String> {
    Ok(SnapshotCandidate {
        path: collected_usage_snapshot_path()?,
        source: SnapshotSource::LilimitCollected,
    })
}

fn lilimit_config_dir() -> Result<PathBuf, String> {
    let home = env::var_os("HOME").ok_or_else(|| "HOME is not set".to_string())?;
    let home = PathBuf::from(home);

    #[cfg(target_os = "macos")]
    {
        Ok(home
            .join("Library")
            .join("Application Support")
            .join("lilimit"))
    }

    #[cfg(not(target_os = "macos"))]
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
    let contents = serde_json::to_string_pretty(settings).map_err(|error| error.to_string())?;
    write_atomic(&path, &contents, 0o644)
}

// Write via a same-directory temp file and rename so a crash mid-write can
// never leave a truncated file behind. This matters most for Codex's
// auth.json, which the Codex CLI also reads and writes.
fn write_atomic(path: &Path, contents: &str, default_mode: u32) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", path.display()))?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;

    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file");
    let temp_path = parent.join(format!(".{file_name}.{}.tmp", std::process::id()));
    fs::write(&temp_path, contents).map_err(|error| error.to_string())?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(path)
            .map(|metadata| metadata.permissions().mode() & 0o777)
            .unwrap_or(default_mode);
        if let Err(error) = fs::set_permissions(&temp_path, fs::Permissions::from_mode(mode)) {
            let _ = fs::remove_file(&temp_path);
            return Err(error.to_string());
        }
    }
    #[cfg(not(unix))]
    {
        let _ = default_mode;
    }

    fs::rename(&temp_path, path).map_err(|error| {
        let _ = fs::remove_file(&temp_path);
        error.to_string()
    })
}

fn collector_state_path() -> Result<PathBuf, String> {
    Ok(lilimit_config_dir()?.join(COLLECTOR_STATE_FILE))
}

fn read_collector_state() -> CollectorState {
    let Ok(path) = collector_state_path() else {
        return CollectorState::default();
    };
    let Ok(contents) = fs::read_to_string(&path) else {
        return CollectorState::default();
    };

    serde_json::from_str::<CollectorState>(&contents).unwrap_or_default()
}

fn write_collector_state(state: &CollectorState) -> Result<(), String> {
    let path = collector_state_path()?;
    let contents = serde_json::to_string_pretty(state).map_err(|error| error.to_string())?;
    write_atomic(&path, &contents, 0o644)
}

fn read_collected_usage_snapshot_file() -> Result<Option<UsageSnapshotFile>, String> {
    let path = collected_usage_snapshot_path()?;
    match fs::read_to_string(&path) {
        Ok(contents) => serde_json::from_str::<UsageSnapshotFile>(&contents)
            .map(Some)
            .map_err(|error| error.to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
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
    flush_pending_window_position(app.state::<Arc<PendingWindowPosition>>().inner());
    let existing = read_widget_settings().unwrap_or_default();
    // The backend owns the window position: the UI's copy goes stale whenever
    // the user drags the widget, so positions echoed back here are ignored.
    next.window_position = existing.window_position;
    let display_mode_changed = next.display_mode != existing.display_mode;

    write_widget_settings(&next)?;

    if let Some(window) = app.get_webview_window("main") {
        if let Some(position) = apply_window_settings(&window, &next, display_mode_changed) {
            let adjusted_position = WindowPosition {
                x: position.x,
                y: position.y,
            };
            if next.window_position != Some(adjusted_position) {
                next.window_position = Some(adjusted_position);
                write_widget_settings(&next)?;
            }
        }
    }
    let _ = app.emit_to("main", "settings-changed", &next);
    let _ = app.emit_to("settings", "settings-changed", &next);
    sync_tray_title_from_current_snapshot(&app);

    Ok(next)
}

#[tauri::command]
fn show_settings_window(app: AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("settings")
        .ok_or_else(|| "Settings window is unavailable.".to_string())?;
    window.show().map_err(|error| error.to_string())?;
    window.set_focus().map_err(|error| error.to_string())
}

fn persist_window_position(x: i32, y: i32) -> Result<(), String> {
    let mut settings = read_widget_settings().unwrap_or_default();
    settings.window_position = Some(WindowPosition { x, y });
    write_widget_settings(&settings)
}

#[derive(Default)]
struct PendingWindowPosition {
    position: Mutex<Option<WindowPosition>>,
    flush_scheduled: AtomicBool,
}

// Move events arrive for every pixel of a drag; writing settings.json each
// time would hammer the disk, so writes are coalesced behind a short delay.
fn queue_window_position_persist(pending: &Arc<PendingWindowPosition>, x: i32, y: i32) {
    if let Ok(mut slot) = pending.position.lock() {
        *slot = Some(WindowPosition { x, y });
    }
    if pending.flush_scheduled.swap(true, Ordering::SeqCst) {
        return;
    }

    let pending = Arc::clone(pending);
    thread::spawn(move || {
        thread::sleep(WINDOW_POSITION_FLUSH_DELAY);
        pending.flush_scheduled.store(false, Ordering::SeqCst);
        flush_pending_window_position(&pending);
    });
}

fn flush_pending_window_position(pending: &PendingWindowPosition) {
    let position = pending
        .position
        .lock()
        .ok()
        .and_then(|mut slot| slot.take());
    if let Some(position) = position {
        let _ = persist_window_position(position.x, position.y);
    }
}

fn window_size(display_mode: DisplayMode) -> (f64, f64) {
    match display_mode {
        DisplayMode::Simple => (SIMPLE_WIDTH, SIMPLE_HEIGHT),
        DisplayMode::Full => (FULL_WIDTH, FULL_HEIGHT),
    }
}

fn apply_window_settings<R: Runtime>(
    window: &WebviewWindow<R>,
    settings: &WidgetSettings,
    preserve_right_edge: bool,
) -> Option<PhysicalPosition<i32>> {
    let (width, height) = window_size(settings.display_mode);
    let current_position = window.outer_position().ok();
    let current_size = window.outer_size().ok();

    let stored_position = settings
        .window_position
        .map(|position| PhysicalPosition::new(position.x, position.y));
    let position = if preserve_right_edge {
        current_position.or(stored_position)
    } else {
        stored_position.or(current_position)
    }?;
    let position = clamp_window_position(
        window,
        position,
        current_size.map(|size| size.width),
        width,
        height,
        preserve_right_edge,
    );

    let _ = window.set_size(Size::Logical(LogicalSize::new(width, height)));

    // Tauri reports physical coordinates. They can be negative on macOS or
    // Linux multi-monitor layouts, so store and replay the raw values.
    let _ = window.set_position(Position::Physical(position));
    Some(position)
}

fn clamp_window_position<R: Runtime>(
    window: &WebviewWindow<R>,
    position: PhysicalPosition<i32>,
    current_width: Option<u32>,
    logical_width: f64,
    logical_height: f64,
    preserve_right_edge: bool,
) -> PhysicalPosition<i32> {
    let Some((left, top, right, bottom, scale_factor)) = work_area_for_position(window, position)
    else {
        return position;
    };
    let width = (logical_width * scale_factor).ceil() as i32;
    let height = (logical_height * scale_factor).ceil() as i32;
    let position = if preserve_right_edge {
        current_width
            .and_then(|width| i32::try_from(width).ok())
            .map(|current_width| right_edge_anchored_position(position, current_width, width))
            .unwrap_or(position)
    } else {
        position
    };

    clamp_position_to_bounds(position, width, height, left, top, right, bottom)
}

fn right_edge_anchored_position(
    position: PhysicalPosition<i32>,
    current_width: i32,
    target_width: i32,
) -> PhysicalPosition<i32> {
    PhysicalPosition::new(
        position
            .x
            .saturating_add(current_width)
            .saturating_sub(target_width),
        position.y,
    )
}

fn work_area_for_position<R: Runtime>(
    window: &WebviewWindow<R>,
    position: PhysicalPosition<i32>,
) -> Option<(i32, i32, i32, i32, f64)> {
    if let Ok(monitors) = window.available_monitors() {
        if let Some(monitor) = monitors
            .into_iter()
            .find(|monitor| work_area_contains(monitor.work_area(), position))
        {
            let area = monitor.work_area();
            return Some(work_area_bounds(
                area.position.x,
                area.position.y,
                area.size.width,
                area.size.height,
                monitor.scale_factor(),
            ));
        }
    }

    window.current_monitor().ok().flatten().map(|monitor| {
        let area = monitor.work_area();
        work_area_bounds(
            area.position.x,
            area.position.y,
            area.size.width,
            area.size.height,
            monitor.scale_factor(),
        )
    })
}

fn work_area_contains(
    area: &tauri::PhysicalRect<i32, u32>,
    position: PhysicalPosition<i32>,
) -> bool {
    let (left, top, right, bottom, _) = work_area_bounds(
        area.position.x,
        area.position.y,
        area.size.width,
        area.size.height,
        1.0,
    );

    position.x >= left && position.x < right && position.y >= top && position.y < bottom
}

fn work_area_bounds(
    left: i32,
    top: i32,
    width: u32,
    height: u32,
    scale_factor: f64,
) -> (i32, i32, i32, i32, f64) {
    let width = i32::try_from(width).unwrap_or(i32::MAX);
    let height = i32::try_from(height).unwrap_or(i32::MAX);

    (
        left,
        top,
        left.saturating_add(width),
        top.saturating_add(height),
        scale_factor,
    )
}

fn clamp_position_to_bounds(
    position: PhysicalPosition<i32>,
    width: i32,
    height: i32,
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
) -> PhysicalPosition<i32> {
    let max_x = right.saturating_sub(width);
    let max_y = bottom.saturating_sub(height);
    let x = clamp_axis(position.x, left, max_x);
    let y = clamp_axis(position.y, top, max_y);

    PhysicalPosition::new(x, y)
}

fn clamp_axis(value: i32, min: i32, max: i32) -> i32 {
    if max < min {
        return min;
    }
    value.clamp(min, max)
}

#[tauri::command]
fn get_usage_snapshot(app: AppHandle) -> UsageSnapshotResponse {
    let candidate = match usage_snapshot_candidate() {
        Ok(candidate) => candidate,
        Err(error) => {
            let response = UsageSnapshotResponse {
                status: "ioError".to_string(),
                path: String::new(),
                source: None,
                updated_at: None,
                providers: Vec::new(),
                error: Some(error),
            };
            sync_tray_title_from_response(&app, &response);
            return response;
        }
    };

    let path_text = candidate.path.to_string_lossy().into_owned();
    let contents = match fs::read_to_string(&candidate.path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let response = UsageSnapshotResponse {
                status: "missing".to_string(),
                path: path_text,
                source: None,
                updated_at: None,
                providers: Vec::new(),
                error: None,
            };
            sync_tray_title_from_response(&app, &response);
            return response;
        }
        Err(error) => {
            let response = UsageSnapshotResponse {
                status: "ioError".to_string(),
                path: path_text,
                source: Some(candidate.source.as_str().to_string()),
                updated_at: None,
                providers: Vec::new(),
                error: Some(error.to_string()),
            };
            sync_tray_title_from_response(&app, &response);
            return response;
        }
    };

    let response = parse_lilimit_snapshot(&contents, &path_text, candidate.source);
    sync_tray_title_from_response(&app, &response);
    response
}

fn parse_lilimit_snapshot(
    contents: &str,
    path_text: &str,
    source: SnapshotSource,
) -> UsageSnapshotResponse {
    match serde_json::from_str::<UsageSnapshotFile>(contents) {
        Ok(snapshot) => UsageSnapshotResponse {
            status: "ready".to_string(),
            path: path_text.to_string(),
            source: Some(source.as_str().to_string()),
            updated_at: Some(snapshot.updated_at),
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
            source: Some(source.as_str().to_string()),
            updated_at: None,
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

fn clamp_percent(value: f64) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }
    value.clamp(0.0, 100.0)
}

fn optional_f64_from_json<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => number
            .as_f64()
            .ok_or_else(|| serde::de::Error::custom("expected finite number"))
            .map(Some),
        Some(Value::String(raw)) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                Ok(None)
            } else {
                trimmed
                    .parse::<f64>()
                    .map(Some)
                    .map_err(serde::de::Error::custom)
            }
        }
        _ => Err(serde::de::Error::custom(
            "expected number or numeric string",
        )),
    }
}

#[derive(Debug)]
enum UsageFetchError {
    Unauthorized,
    AuthenticationRequired(String),
    RateLimited(Option<DateTime<Utc>>),
    Message(String),
}

impl std::fmt::Display for UsageFetchError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UsageFetchError::Unauthorized => write!(formatter, "unauthorized"),
            UsageFetchError::AuthenticationRequired(message) => write!(formatter, "{message}"),
            UsageFetchError::RateLimited(Some(retry_at)) => write!(
                formatter,
                "rate limited; retry {}",
                reset_countdown_description(*retry_at)
            ),
            UsageFetchError::RateLimited(None) => write!(formatter, "rate limited"),
            UsageFetchError::Message(message) => write!(formatter, "{message}"),
        }
    }
}

#[derive(Debug, Clone)]
struct CodexCredentials {
    access_token: String,
    refresh_token: String,
    id_token: Option<String>,
    account_id: Option<String>,
    last_refresh: Option<DateTime<Utc>>,
    path: PathBuf,
}

#[derive(Debug, Clone)]
struct ClaudeCredentials {
    access_token: String,
    refresh_token: Option<String>,
    expires_at_ms: Option<f64>,
    rate_limit_tier: Option<String>,
    subscription_type: Option<String>,
    path: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
struct CodexUsageApiResponse {
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    plan_type: Option<String>,
    #[serde(default)]
    rate_limit: Option<CodexRateLimitDetails>,
    #[serde(default)]
    credits: Option<CodexCreditDetails>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
struct CodexRateLimitDetails {
    #[serde(default)]
    primary_window: Option<CodexUsageWindow>,
    #[serde(default)]
    secondary_window: Option<CodexUsageWindow>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
struct CodexUsageWindow {
    #[serde(default)]
    used_percent: Option<f64>,
    #[serde(default)]
    reset_at: Option<i64>,
    #[serde(default)]
    limit_window_seconds: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
struct CodexCreditDetails {
    #[serde(default, deserialize_with = "optional_f64_from_json")]
    balance: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct ClaudeOAuthUsageResponse {
    #[serde(default)]
    five_hour: Option<ClaudeOAuthWindow>,
    #[serde(default)]
    seven_day: Option<ClaudeOAuthWindow>,
    #[serde(default)]
    seven_day_oauth_apps: Option<ClaudeOAuthWindow>,
    #[serde(default)]
    seven_day_opus: Option<ClaudeOAuthWindow>,
    #[serde(default)]
    seven_day_sonnet: Option<ClaudeOAuthWindow>,
    #[serde(default)]
    seven_day_design: Option<ClaudeOAuthWindow>,
    #[serde(default)]
    seven_day_claude_design: Option<ClaudeOAuthWindow>,
    #[serde(default)]
    claude_design: Option<ClaudeOAuthWindow>,
    #[serde(default)]
    design: Option<ClaudeOAuthWindow>,
    #[serde(default)]
    seven_day_omelette: Option<ClaudeOAuthWindow>,
    #[serde(default)]
    omelette: Option<ClaudeOAuthWindow>,
    #[serde(default)]
    omelette_promotional: Option<ClaudeOAuthWindow>,
    #[serde(default)]
    seven_day_routines: Option<ClaudeOAuthWindow>,
    #[serde(default)]
    seven_day_claude_routines: Option<ClaudeOAuthWindow>,
    #[serde(default)]
    claude_routines: Option<ClaudeOAuthWindow>,
    #[serde(default)]
    routines: Option<ClaudeOAuthWindow>,
    #[serde(default)]
    routine: Option<ClaudeOAuthWindow>,
    #[serde(default)]
    seven_day_cowork: Option<ClaudeOAuthWindow>,
    #[serde(default)]
    cowork: Option<ClaudeOAuthWindow>,
    #[serde(default)]
    extra_usage: Option<ClaudeExtraUsage>,
}

impl ClaudeOAuthUsageResponse {
    fn design_window(&self) -> Option<&ClaudeOAuthWindow> {
        self.seven_day_design
            .as_ref()
            .or(self.seven_day_claude_design.as_ref())
            .or(self.claude_design.as_ref())
            .or(self.design.as_ref())
            .or(self.seven_day_omelette.as_ref())
            .or(self.omelette.as_ref())
            .or(self.omelette_promotional.as_ref())
    }

    fn routines_window(&self) -> Option<&ClaudeOAuthWindow> {
        self.seven_day_routines
            .as_ref()
            .or(self.seven_day_claude_routines.as_ref())
            .or(self.claude_routines.as_ref())
            .or(self.routines.as_ref())
            .or(self.routine.as_ref())
            .or(self.seven_day_cowork.as_ref())
            .or(self.cowork.as_ref())
    }
}

#[derive(Debug, Clone, Deserialize)]
struct ClaudeOAuthWindow {
    #[serde(default)]
    utilization: Option<f64>,
    #[serde(default)]
    resets_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ClaudeExtraUsage {
    #[serde(default)]
    is_enabled: Option<bool>,
    #[serde(default, deserialize_with = "optional_f64_from_json")]
    monthly_limit: Option<f64>,
    #[serde(default, deserialize_with = "optional_f64_from_json")]
    used_credits: Option<f64>,
    #[serde(default, deserialize_with = "optional_f64_from_json")]
    utilization: Option<f64>,
    #[serde(default)]
    currency: Option<String>,
}

#[tauri::command]
async fn refresh_collected_usage_snapshot(
    app: AppHandle,
    force: Option<bool>,
) -> Result<UsageSnapshotResponse, String> {
    let force = force.unwrap_or(false);
    let settings = read_widget_settings().unwrap_or_default();
    let previous_snapshot = read_collected_usage_snapshot_file().unwrap_or(None);
    if !force {
        if let Some(snapshot) = previous_snapshot.as_ref() {
            if snapshot_is_fresh(snapshot, COLLECTION_REFRESH_INTERVAL) {
                let response = response_from_snapshot_file(snapshot)?;
                sync_tray_title_from_response(&app, &response);
                return Ok(response);
            }
        }
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent("lilimit")
        .build()
        .map_err(|error| error.to_string())?;

    let mut collector_state = read_collector_state();
    let previous_codex = previous_snapshot
        .as_ref()
        .and_then(|snapshot| snapshot_provider(snapshot, "Codex"))
        .cloned();
    let previous_claude = previous_snapshot
        .as_ref()
        .and_then(|snapshot| snapshot_provider(snapshot, "Claude"))
        .cloned();

    let codex_result = collect_codex_provider(&client, previous_codex).await;
    let claude_result = collect_claude_provider(
        &client,
        settings.keychain_access,
        previous_claude,
        &mut collector_state,
    )
    .await;
    write_collector_state(&collector_state)?;

    let any_available = [&codex_result, &claude_result]
        .iter()
        .any(|result| !matches!(result, ProviderCollectionResult::Unavailable(_)));
    let mut providers = Vec::new();
    let mut errors = Vec::new();
    for (name, result) in [("Codex", codex_result), ("Claude", claude_result)] {
        match result {
            ProviderCollectionResult::Provider(provider) => providers.push(provider),
            ProviderCollectionResult::Cached { provider, warning } => {
                providers.push(provider);
                errors.push(format!("{name}: {warning}"));
            }
            ProviderCollectionResult::Unavailable(error) => {
                if any_available || previous_snapshot.is_some() {
                    providers.push(unavailable_provider(name, error.clone()));
                }
                errors.push(format!("{name}: {error}"));
            }
        }
    }

    if providers.is_empty() {
        return Err(if errors.is_empty() {
            "No Codex or Claude credentials found.".to_string()
        } else {
            errors.join(" / ")
        });
    }

    let snapshot = UsageSnapshotFile {
        updated_at: now_iso_string(),
        providers,
    };
    write_collected_usage_snapshot(&snapshot)?;

    let path = collected_usage_snapshot_path()?;
    let contents = serde_json::to_string(&snapshot).map_err(|error| error.to_string())?;
    let mut response = parse_lilimit_snapshot(
        &contents,
        &path.to_string_lossy(),
        SnapshotSource::LilimitCollected,
    );
    sync_tray_title_from_response(&app, &response);
    if !errors.is_empty() {
        let error_summary = errors.join(" / ");
        eprintln!("lilimit collector warning: {error_summary}");
        response.error = Some(error_summary);
    }
    Ok(response)
}

fn collected_usage_snapshot_path() -> Result<PathBuf, String> {
    Ok(lilimit_config_dir()?.join(COLLECTED_USAGE_FILE))
}

fn write_collected_usage_snapshot(snapshot: &UsageSnapshotFile) -> Result<(), String> {
    let path = collected_usage_snapshot_path()?;
    let contents = serde_json::to_string_pretty(snapshot).map_err(|error| error.to_string())?;
    write_atomic(&path, &contents, 0o644)
}

fn unavailable_provider(name: &str, error: String) -> ProviderUsage {
    ProviderUsage {
        name: name.to_string(),
        account_email: None,
        plan_text: None,
        session_left_percent: None,
        weekly_left_percent: None,
        reset_text: "auth".to_string(),
        updated_at: None,
        usage_rows: Vec::new(),
        primary: None,
        secondary: None,
        tertiary: None,
        credits_remaining: None,
        code_review_remaining_percent: None,
        token_usage: None,
        daily_usage: Vec::new(),
        stale: true,
        error: Some(error),
    }
}

fn response_from_snapshot_file(
    snapshot: &UsageSnapshotFile,
) -> Result<UsageSnapshotResponse, String> {
    let path = collected_usage_snapshot_path()?;
    let contents = serde_json::to_string(snapshot).map_err(|error| error.to_string())?;
    Ok(parse_lilimit_snapshot(
        &contents,
        &path.to_string_lossy(),
        SnapshotSource::LilimitCollected,
    ))
}

fn snapshot_is_fresh(snapshot: &UsageSnapshotFile, interval: Duration) -> bool {
    timestamp_is_within(&snapshot.updated_at, interval)
}

fn timestamp_is_within(value: &str, interval: Duration) -> bool {
    let Some(timestamp) = parse_rfc3339_datetime(value) else {
        return false;
    };
    let age_seconds = Utc::now().signed_duration_since(timestamp).num_seconds();
    age_seconds >= 0 && age_seconds < interval.as_secs() as i64
}

fn snapshot_provider<'a>(snapshot: &'a UsageSnapshotFile, name: &str) -> Option<&'a ProviderUsage> {
    snapshot
        .providers
        .iter()
        .find(|provider| provider.name.eq_ignore_ascii_case(name))
}

enum ProviderCollectionResult {
    Provider(ProviderUsage),
    Cached {
        provider: ProviderUsage,
        warning: String,
    },
    Unavailable(String),
}

async fn collect_codex_provider(
    client: &reqwest::Client,
    previous: Option<ProviderUsage>,
) -> ProviderCollectionResult {
    match fetch_codex_provider(client).await {
        Ok(provider) => ProviderCollectionResult::Provider(provider),
        Err(error) => {
            let message = error.to_string();
            if let Some(provider) = previous {
                if should_preserve_cached_provider(&error) {
                    return ProviderCollectionResult::Cached {
                        provider: stale_provider(provider, message.clone()),
                        warning: format!("{message}; showing cached data"),
                    };
                }
            }

            ProviderCollectionResult::Unavailable(message)
        }
    }
}

async fn collect_claude_provider(
    client: &reqwest::Client,
    keychain_access: KeychainAccess,
    previous: Option<ProviderUsage>,
    collector_state: &mut CollectorState,
) -> ProviderCollectionResult {
    if let Some(provider) = previous.as_ref() {
        if provider
            .updated_at
            .as_deref()
            .is_some_and(|updated_at| timestamp_is_within(updated_at, CLAUDE_REFRESH_INTERVAL))
        {
            return ProviderCollectionResult::Provider(provider.clone());
        }
    }

    if let Some(next_attempt_at) = claude_backoff_until(collector_state) {
        let warning = format!(
            "rate limited; retry {}",
            reset_countdown_description(next_attempt_at)
        );
        if let Some(provider) = previous {
            return ProviderCollectionResult::Cached {
                provider: stale_provider(provider, warning.clone()),
                warning,
            };
        }
        return ProviderCollectionResult::Unavailable(warning);
    }

    match fetch_claude_provider(client, keychain_access).await {
        Ok(provider) => {
            collector_state.claude_next_attempt_at = None;
            collector_state.claude_rate_limit_failures = 0;
            ProviderCollectionResult::Provider(provider)
        }
        Err(error) => {
            if let UsageFetchError::RateLimited(retry_after) = &error {
                schedule_claude_backoff(collector_state, *retry_after);
            }

            let message = error.to_string();
            if let Some(provider) = previous {
                if should_preserve_cached_provider(&error) {
                    return ProviderCollectionResult::Cached {
                        provider: stale_provider(provider, message.clone()),
                        warning: format!("{message}; showing cached data"),
                    };
                }
            }

            ProviderCollectionResult::Unavailable(message)
        }
    }
}

fn stale_provider(mut provider: ProviderUsage, error: String) -> ProviderUsage {
    provider.stale = true;
    provider.error = Some(error);
    provider
}

fn claude_backoff_until(state: &CollectorState) -> Option<DateTime<Utc>> {
    let retry_at = state
        .claude_next_attempt_at
        .as_deref()
        .and_then(parse_rfc3339_datetime)?;
    if retry_at > Utc::now() {
        Some(retry_at)
    } else {
        None
    }
}

fn schedule_claude_backoff(state: &mut CollectorState, retry_after: Option<DateTime<Utc>>) {
    state.claude_rate_limit_failures = state.claude_rate_limit_failures.saturating_add(1).max(1);
    let backoff = claude_backoff_duration(state.claude_rate_limit_failures);
    let calculated_retry_at = Utc::now() + chrono::Duration::seconds(backoff.as_secs() as i64);
    let retry_at = retry_after
        .filter(|date| *date > Utc::now())
        .unwrap_or(calculated_retry_at);
    state.claude_next_attempt_at = Some(retry_at.to_rfc3339_opts(SecondsFormat::Secs, true));
}

fn claude_backoff_duration(failures: u32) -> Duration {
    let exponent = failures.saturating_sub(1).min(10);
    let multiplier = 1_u64.checked_shl(exponent).unwrap_or(u64::MAX);
    let seconds = CLAUDE_RATE_LIMIT_BASE_BACKOFF
        .as_secs()
        .saturating_mul(multiplier)
        .min(CLAUDE_RATE_LIMIT_MAX_BACKOFF.as_secs());
    Duration::from_secs(seconds)
}

fn should_preserve_cached_provider(error: &UsageFetchError) -> bool {
    match error {
        UsageFetchError::AuthenticationRequired(_) => true,
        UsageFetchError::RateLimited(_) => true,
        UsageFetchError::Unauthorized => false,
        UsageFetchError::Message(message) => is_transient_fetch_error(message),
    }
}

fn is_transient_fetch_error(message: &str) -> bool {
    let lower = message.to_lowercase();
    lower.contains("timed out")
        || lower.contains("timeout")
        || lower.contains("connection")
        || lower.contains("network")
        || lower.contains("dns")
        || lower.contains("lookup")
        || lower.contains("http 408")
        || lower.contains("http 425")
        || lower.contains("http 429")
        || lower.contains("http 500")
        || lower.contains("http 502")
        || lower.contains("http 503")
        || lower.contains("http 504")
}

async fn fetch_codex_provider(client: &reqwest::Client) -> Result<ProviderUsage, UsageFetchError> {
    let mut credentials = load_codex_credentials().map_err(UsageFetchError::Message)?;
    if codex_credentials_need_refresh(&credentials) {
        credentials = refresh_codex_credentials(client, &credentials).await?;
        save_codex_credentials(&credentials).map_err(UsageFetchError::Message)?;
    }

    match fetch_codex_usage(client, &credentials).await {
        Ok(response) => Ok(codex_provider_from_usage(response)),
        Err(UsageFetchError::Unauthorized) if !credentials.refresh_token.is_empty() => {
            let refreshed = refresh_codex_credentials(client, &credentials).await?;
            save_codex_credentials(&refreshed).map_err(UsageFetchError::Message)?;
            fetch_codex_usage(client, &refreshed)
                .await
                .map(codex_provider_from_usage)
        }
        Err(error) => Err(error),
    }
}

fn load_codex_credentials() -> Result<CodexCredentials, String> {
    let path = codex_home_dir()?.join("auth.json");
    let contents = fs::read_to_string(&path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            "Codex auth.json not found. Run `codex` to log in.".to_string()
        } else {
            error.to_string()
        }
    })?;
    let value: Value = serde_json::from_str(&contents).map_err(|error| error.to_string())?;

    let tokens = value.get("tokens").ok_or_else(|| {
        if value.get("OPENAI_API_KEY").is_some() {
            "Codex is using API-key auth, but usage stats require ChatGPT OAuth tokens. Run `codex` and log in with ChatGPT.".to_string()
        } else {
            "Codex OAuth tokens missing. Run `codex` to log in.".to_string()
        }
    })?;
    let access_token = string_at(tokens, &["access_token"])
        .or_else(|| string_at(tokens, &["accessToken"]))
        .ok_or_else(|| "Codex OAuth access token missing. Run `codex` to log in.".to_string())?;
    let refresh_token = string_at(tokens, &["refresh_token"])
        .or_else(|| string_at(tokens, &["refreshToken"]))
        .unwrap_or_default();

    Ok(CodexCredentials {
        access_token,
        refresh_token,
        id_token: string_at(tokens, &["id_token"]).or_else(|| string_at(tokens, &["idToken"])),
        account_id: string_at(tokens, &["account_id"])
            .or_else(|| string_at(tokens, &["accountId"])),
        last_refresh: string_at(&value, &["last_refresh"])
            .or_else(|| string_at(&value, &["lastRefresh"]))
            .and_then(|raw| parse_rfc3339_datetime(&raw)),
        path,
    })
}

fn codex_home_dir() -> Result<PathBuf, String> {
    if let Some(path) = env::var_os("CODEX_HOME") {
        if !path.is_empty() {
            return Ok(PathBuf::from(path));
        }
    }

    let home = env::var_os("HOME").ok_or_else(|| "HOME is not set".to_string())?;
    Ok(PathBuf::from(home).join(".codex"))
}

fn codex_credentials_need_refresh(credentials: &CodexCredentials) -> bool {
    if credentials.refresh_token.is_empty() {
        return false;
    }
    match credentials.last_refresh {
        Some(last_refresh) => {
            Utc::now().signed_duration_since(last_refresh).num_days()
                >= CODEX_AUTH_REFRESH_AFTER_DAYS
        }
        None => true,
    }
}

async fn refresh_codex_credentials(
    client: &reqwest::Client,
    credentials: &CodexCredentials,
) -> Result<CodexCredentials, UsageFetchError> {
    if credentials.refresh_token.is_empty() {
        return Err(UsageFetchError::Message(
            "Codex OAuth refresh token missing. Run `codex` to log in.".to_string(),
        ));
    }

    let response = client
        .post(CODEX_REFRESH_ENDPOINT)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .json(&json!({
            "client_id": CODEX_OAUTH_CLIENT_ID,
            "grant_type": "refresh_token",
            "refresh_token": credentials.refresh_token,
            "scope": "openid profile email"
        }))
        .send()
        .await
        .map_err(|error| UsageFetchError::Message(error.to_string()))?;

    let status = response.status();
    if !status.is_success() {
        return Err(UsageFetchError::Message(format!(
            "Codex OAuth refresh failed with HTTP {}. Run `codex` to log in again.",
            status.as_u16()
        )));
    }

    let body: Value = response
        .json()
        .await
        .map_err(|error| UsageFetchError::Message(error.to_string()))?;
    let access_token =
        string_at(&body, &["access_token"]).unwrap_or_else(|| credentials.access_token.clone());
    let refresh_token =
        string_at(&body, &["refresh_token"]).unwrap_or_else(|| credentials.refresh_token.clone());
    let id_token = string_at(&body, &["id_token"]).or_else(|| credentials.id_token.clone());

    Ok(CodexCredentials {
        access_token,
        refresh_token,
        id_token,
        account_id: credentials.account_id.clone(),
        last_refresh: Some(Utc::now()),
        path: credentials.path.clone(),
    })
}

fn save_codex_credentials(credentials: &CodexCredentials) -> Result<(), String> {
    let contents = fs::read_to_string(&credentials.path).unwrap_or_else(|_| "{}".to_string());
    let mut value: Value = serde_json::from_str(&contents).unwrap_or_else(|_| json!({}));
    let mut tokens = value.get("tokens").cloned().unwrap_or_else(|| json!({}));

    set_string(
        &mut tokens,
        "access_token",
        Some(credentials.access_token.clone()),
    );
    set_string(
        &mut tokens,
        "refresh_token",
        Some(credentials.refresh_token.clone()),
    );
    set_string(&mut tokens, "id_token", credentials.id_token.clone());
    set_string(&mut tokens, "account_id", credentials.account_id.clone());
    value["tokens"] = tokens;
    value["last_refresh"] = Value::String(now_iso_string());

    let serialized = serde_json::to_string_pretty(&value).map_err(|error| error.to_string())?;
    write_atomic(&credentials.path, &serialized, 0o600)
}

async fn fetch_codex_usage(
    client: &reqwest::Client,
    credentials: &CodexCredentials,
) -> Result<CodexUsageApiResponse, UsageFetchError> {
    let mut request = client
        .get(resolve_codex_usage_url())
        .bearer_auth(&credentials.access_token)
        .header(reqwest::header::ACCEPT, "application/json")
        .header(reqwest::header::USER_AGENT, "lilimit");

    if let Some(account_id) = credentials
        .account_id
        .as_ref()
        .filter(|value| !value.is_empty())
    {
        request = request.header("ChatGPT-Account-Id", account_id);
    }

    let response = request
        .send()
        .await
        .map_err(|error| UsageFetchError::Message(error.to_string()))?;
    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(UsageFetchError::Unauthorized);
    }
    if !status.is_success() {
        return Err(UsageFetchError::Message(format!(
            "Codex usage API returned HTTP {}",
            status.as_u16()
        )));
    }

    response
        .json::<CodexUsageApiResponse>()
        .await
        .map_err(|error| UsageFetchError::Message(error.to_string()))
}

fn resolve_codex_usage_url() -> String {
    let base_url = codex_config_chatgpt_base_url()
        .unwrap_or_else(|| CODEX_DEFAULT_CHATGPT_BASE_URL.to_string());
    let mut normalized = base_url.trim().trim_end_matches('/').to_string();
    if normalized.is_empty() {
        normalized = CODEX_DEFAULT_CHATGPT_BASE_URL.to_string();
    }
    if (normalized.starts_with("https://chatgpt.com")
        || normalized.starts_with("https://chat.openai.com"))
        && !normalized.contains("/backend-api")
    {
        normalized.push_str("/backend-api");
    }

    let path = if normalized.contains("/backend-api") {
        "/wham/usage"
    } else {
        "/api/codex/usage"
    };
    format!("{normalized}{path}")
}

fn codex_config_chatgpt_base_url() -> Option<String> {
    let config = codex_home_dir().ok()?.join("config.toml");
    let contents = fs::read_to_string(config).ok()?;
    for raw_line in contents.lines() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != "chatgpt_base_url" {
            continue;
        }
        let trimmed = value.trim().trim_matches('"').trim_matches('\'').trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    None
}

fn codex_provider_from_usage(response: CodexUsageApiResponse) -> ProviderUsage {
    let primary_raw = response
        .rate_limit
        .as_ref()
        .and_then(|details| details.primary_window.as_ref())
        .and_then(rate_window_from_codex);
    let secondary_raw = response
        .rate_limit
        .as_ref()
        .and_then(|details| details.secondary_window.as_ref())
        .and_then(rate_window_from_codex);
    let (primary, secondary) = normalize_codex_rate_windows(primary_raw, secondary_raw);
    let mut usage_rows = Vec::new();

    if let Some(window) = primary.as_ref() {
        usage_rows.push(usage_row_from_window("session", "Session", window));
    }
    if let Some(window) = secondary.as_ref() {
        usage_rows.push(usage_row_from_window("weekly", "Weekly", window));
    }

    let reset_text = primary
        .as_ref()
        .and_then(|window| window.reset_description.clone())
        .unwrap_or_default();
    let local_usage = load_codex_local_token_usage();
    let (token_usage, daily_usage) = local_usage.unwrap_or((None, Vec::new()));

    ProviderUsage {
        name: "Codex".to_string(),
        account_email: response.email,
        plan_text: response
            .plan_type
            .as_deref()
            .and_then(codex_plan_display_text),
        session_left_percent: primary.as_ref().and_then(|window| window.percent_left),
        weekly_left_percent: secondary.as_ref().and_then(|window| window.percent_left),
        reset_text,
        updated_at: Some(now_iso_string()),
        usage_rows,
        primary,
        secondary,
        tertiary: None,
        credits_remaining: response.credits.and_then(|credits| credits.balance),
        code_review_remaining_percent: None,
        token_usage,
        daily_usage,
        stale: false,
        error: None,
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodexBarCostCache {
    #[serde(default)]
    days: HashMap<String, HashMap<String, Vec<i64>>>,
    #[serde(default)]
    files: HashMap<String, CodexBarCostFile>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodexBarCostFile {
    #[serde(default)]
    codex_rows: Vec<CodexBarCodexRow>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodexBarCodexRow {
    day: String,
    model: String,
    #[serde(default)]
    input: i64,
    #[serde(default)]
    cached: i64,
    #[serde(default)]
    output: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PiSessionCostCache {
    #[serde(default)]
    days_by_provider: HashMap<String, HashMap<String, HashMap<String, PiPackedUsage>>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PiPackedUsage {
    #[serde(default)]
    input_tokens: i64,
    #[serde(default)]
    cache_read_tokens: i64,
    #[serde(default)]
    cache_write_tokens: i64,
    #[serde(default)]
    output_tokens: i64,
    #[serde(default)]
    total_tokens: i64,
    #[serde(default)]
    cost_nanos: i64,
    #[serde(default)]
    cost_sample_count: i64,
    #[serde(default)]
    usage_sample_count: Option<i64>,
}

#[derive(Debug, Clone, Copy)]
struct CodexModelPricing {
    input_rate: f64,
    output_rate: f64,
    cache_rate: Option<f64>,
    threshold_tokens: Option<i64>,
    input_rate_above_threshold: Option<f64>,
    output_rate_above_threshold: Option<f64>,
    cache_rate_above_threshold: Option<f64>,
}

#[derive(Debug, Clone, Copy)]
struct ClaudeModelPricing {
    input_rate: f64,
    output_rate: f64,
    cache_creation_rate: f64,
    cache_read_rate: f64,
    threshold_tokens: Option<i64>,
    input_rate_above_threshold: Option<f64>,
    output_rate_above_threshold: Option<f64>,
    cache_creation_rate_above_threshold: Option<f64>,
    cache_read_rate_above_threshold: Option<f64>,
}

#[derive(Debug, Default)]
struct LocalDayUsage {
    total_tokens: i64,
    cost_usd: f64,
    cost_seen: bool,
}

#[derive(Debug, Default)]
struct LocalModelUsage {
    total_tokens: i64,
    cost_usd: f64,
    cost_seen: bool,
}

#[derive(Debug, Clone, Copy, Default)]
struct CodexTokenTotals {
    input: i64,
    cached: i64,
    output: i64,
}

impl CodexTokenTotals {
    fn total_tokens(self) -> i64 {
        self.input.saturating_add(self.output).max(0)
    }

    fn add(self, other: CodexTokenTotals) -> CodexTokenTotals {
        CodexTokenTotals {
            input: self.input.saturating_add(other.input),
            cached: self.cached.saturating_add(other.cached),
            output: self.output.saturating_add(other.output),
        }
    }

    fn delta_from(self, previous: CodexTokenTotals) -> CodexTokenTotals {
        if self.input >= previous.input
            && self.cached >= previous.cached
            && self.output >= previous.output
        {
            return CodexTokenTotals {
                input: self.input.saturating_sub(previous.input),
                cached: self.cached.saturating_sub(previous.cached),
                output: self.output.saturating_sub(previous.output),
            };
        }
        self
    }
}

// CodexBar is a macOS menu bar app, so its cost caches only exist there.
fn codexbar_cost_cache_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        Some(
            home_dir()
                .ok()?
                .join("Library")
                .join("Caches")
                .join("CodexBar")
                .join("cost-usage"),
        )
    }

    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

fn load_codex_local_token_usage() -> Option<(Option<TokenUsageSummary>, Vec<DailyUsagePoint>)> {
    let today = Local::now().date_naive();
    let since = today.checked_sub_days(Days::new(29)).unwrap_or(today);
    let mut days: BTreeMap<String, LocalDayUsage> = BTreeMap::new();
    let mut models: HashMap<String, LocalModelUsage> = HashMap::new();

    if let Some(cache_dir) = codexbar_cost_cache_dir() {
        if let Some(cache) = read_json_file::<CodexBarCostCache>(&cache_dir.join("codex-v7.json")) {
            merge_codexbar_cost_cache(cache, since, today, &mut days, &mut models);
        }

        if let Some(cache) =
            read_json_file::<PiSessionCostCache>(&cache_dir.join("pi-sessions-v2.json"))
        {
            merge_pi_session_cost_cache(cache, since, today, &mut days, &mut models);
        }
    }

    if days.is_empty() {
        scan_codex_session_logs(since, today, &mut days, &mut models);
    }

    summarize_local_token_usage(&days, &models)
}

fn load_claude_local_token_usage() -> Option<(Option<TokenUsageSummary>, Vec<DailyUsagePoint>)> {
    let cache_dir = codexbar_cost_cache_dir()?;
    let today = Local::now().date_naive();
    let since = today.checked_sub_days(Days::new(29)).unwrap_or(today);
    let mut days: BTreeMap<String, LocalDayUsage> = BTreeMap::new();
    let mut models: HashMap<String, LocalModelUsage> = HashMap::new();

    if let Some(cache) = read_json_file::<CodexBarCostCache>(&cache_dir.join("claude-v2.json")) {
        merge_claude_cost_cache(cache, since, today, &mut days, &mut models);
    }

    if let Some(cache) =
        read_json_file::<PiSessionCostCache>(&cache_dir.join("pi-sessions-v2.json"))
    {
        merge_pi_claude_cost_cache(cache, since, today, &mut days, &mut models);
    }

    summarize_local_token_usage(&days, &models)
}

fn summarize_local_token_usage(
    days: &BTreeMap<String, LocalDayUsage>,
    models: &HashMap<String, LocalModelUsage>,
) -> Option<(Option<TokenUsageSummary>, Vec<DailyUsagePoint>)> {
    if days.is_empty() {
        return None;
    }

    let current_day = days.iter().next_back().map(|(_, usage)| usage);
    let total_tokens = days.values().map(|usage| usage.total_tokens).sum::<i64>();
    let cost_seen = days.values().any(|usage| usage.cost_seen);
    let total_cost = days.values().map(|usage| usage.cost_usd).sum::<f64>();
    let top_model = top_local_model(models);
    let daily_usage = days
        .iter()
        .map(|(day_key, usage)| DailyUsagePoint {
            day_key: day_key.clone(),
            total_tokens: (usage.total_tokens > 0).then_some(usage.total_tokens),
            cost_usd: usage.cost_seen.then_some(usage.cost_usd),
        })
        .collect::<Vec<_>>();

    let token_usage = TokenUsageSummary {
        session_cost_usd: current_day.and_then(|usage| usage.cost_seen.then_some(usage.cost_usd)),
        session_tokens: current_day
            .and_then(|usage| (usage.total_tokens > 0).then_some(usage.total_tokens)),
        last30_days_cost_usd: cost_seen.then_some(total_cost),
        last30_days_tokens: (total_tokens > 0).then_some(total_tokens),
        top_model,
    };

    Some((Some(token_usage), daily_usage))
}

fn scan_codex_session_logs(
    since: NaiveDate,
    until: NaiveDate,
    days: &mut BTreeMap<String, LocalDayUsage>,
    models: &mut HashMap<String, LocalModelUsage>,
) {
    let Ok(root) = codex_home_dir().map(|path| path.join("sessions")) else {
        return;
    };
    let mut files = Vec::new();
    // Sessions live at sessions/YYYY/MM/DD/rollout-*.jsonl; the depth cap just
    // guards against symlink cycles.
    collect_codex_session_files(&root, since, until, &mut files, 8);
    files.sort();
    for path in files {
        scan_codex_session_file(&path, since, until, days, models);
    }
}

fn collect_codex_session_files(
    root: &Path,
    since: NaiveDate,
    until: NaiveDate,
    files: &mut Vec<PathBuf>,
    max_depth: usize,
) {
    let Some(remaining_depth) = max_depth.checked_sub(1) else {
        return;
    };
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_codex_session_files(&path, since, until, files, remaining_depth);
            continue;
        }

        if path.extension().and_then(|value| value.to_str()) != Some("jsonl") {
            continue;
        }
        if codex_session_file_date(&path)
            .map(|date| date >= since && date <= until)
            .unwrap_or(false)
        {
            files.push(path);
        }
    }
}

fn scan_codex_session_file(
    path: &Path,
    since: NaiveDate,
    until: NaiveDate,
    days: &mut BTreeMap<String, LocalDayUsage>,
    models: &mut HashMap<String, LocalModelUsage>,
) {
    let Ok(file) = fs::File::open(path) else {
        return;
    };
    let reader = BufReader::new(file);
    let mut current_model = String::new();
    let mut previous_total: Option<CodexTokenTotals> = None;

    for line in reader.lines().map_while(Result::ok) {
        if !(line.contains("\"token_count\"") || line.contains("\"turn_context\"")) {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };

        if value.get("type").and_then(Value::as_str) == Some("turn_context") {
            if let Some(model) = string_at(&value, &["payload", "model"]) {
                current_model = normalize_codex_model(&model);
            }
            continue;
        }

        let Some(payload) = value.get("payload") else {
            continue;
        };
        if payload.get("type").and_then(Value::as_str) != Some("token_count") {
            continue;
        }

        let Some(timestamp) = value.get("timestamp").and_then(Value::as_str) else {
            continue;
        };
        let Some(day_key) = local_day_key_from_timestamp(timestamp) else {
            continue;
        };
        let day_in_range = day_key_in_range(&day_key, since, until);
        let info = payload.get("info").filter(|value| !value.is_null());
        let total = info
            .and_then(|info| info.get("total_token_usage"))
            .map(codex_totals_from_value);
        let last = info
            .and_then(|info| info.get("last_token_usage"))
            .map(codex_totals_from_value);

        let delta = if let Some(total) = total {
            let delta = previous_total
                .map(|previous| total.delta_from(previous))
                .unwrap_or(total);
            previous_total = Some(total);
            delta
        } else if let Some(last) = last {
            previous_total = Some(previous_total.unwrap_or_default().add(last));
            last
        } else {
            continue;
        };

        if !day_in_range || current_model.is_empty() || delta.total_tokens() <= 0 {
            continue;
        }
        let cost = codex_cost_usd(&current_model, delta.input, delta.cached, delta.output);
        record_daily_usage(
            days,
            models,
            &day_key,
            &current_model,
            delta.total_tokens(),
            cost,
        );
    }
}

fn codex_session_file_date(path: &Path) -> Option<NaiveDate> {
    let filename = path.file_name()?.to_str()?;
    let date = filename.strip_prefix("rollout-")?.get(..10)?;
    NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()
}

fn local_day_key_from_timestamp(timestamp: &str) -> Option<String> {
    DateTime::parse_from_rfc3339(timestamp).ok().map(|date| {
        date.with_timezone(&Local)
            .date_naive()
            .format("%Y-%m-%d")
            .to_string()
    })
}

fn codex_totals_from_value(value: &Value) -> CodexTokenTotals {
    CodexTokenTotals {
        input: i64_at(value, &["input_tokens"]).unwrap_or_default().max(0),
        cached: i64_at(value, &["cached_input_tokens"])
            .or_else(|| i64_at(value, &["cache_read_input_tokens"]))
            .unwrap_or_default()
            .max(0),
        output: i64_at(value, &["output_tokens"]).unwrap_or_default().max(0),
    }
}

fn i64_at(value: &Value, keys: &[&str]) -> Option<i64> {
    let mut current = value;
    for key in keys {
        current = current.get(*key)?;
    }

    current
        .as_i64()
        .or_else(|| current.as_u64().and_then(|value| i64::try_from(value).ok()))
        .or_else(|| current.as_f64().map(|value| value.round() as i64))
}

fn read_json_file<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let contents = fs::read_to_string(path).ok()?;
    serde_json::from_str::<T>(&contents).ok()
}

fn merge_codexbar_cost_cache(
    cache: CodexBarCostCache,
    since: NaiveDate,
    until: NaiveDate,
    days: &mut BTreeMap<String, LocalDayUsage>,
    models: &mut HashMap<String, LocalModelUsage>,
) {
    let mut row_costs: HashMap<(String, String), LocalDayUsage> = HashMap::new();
    for file in cache.files.values() {
        for row in &file.codex_rows {
            if !day_key_in_range(&row.day, since, until) {
                continue;
            }
            let model = normalize_codex_model(&row.model);
            let cost = codex_cost_usd(&model, row.input, row.cached, row.output);
            record_local_usage(
                &mut row_costs,
                &row.day,
                &model,
                row.input.saturating_add(row.output),
                cost,
            );
        }
    }

    for (day_key, model_values) in cache.days {
        if !day_key_in_range(&day_key, since, until) {
            continue;
        }
        for (raw_model, packed) in model_values {
            let model = normalize_codex_model(&raw_model);
            let input = packed.first().copied().unwrap_or_default();
            let cached = packed.get(1).copied().unwrap_or_default();
            let output = packed.get(2).copied().unwrap_or_default();
            let tokens = input.saturating_add(output).max(0);
            let row_key = (day_key.clone(), model.clone());
            let cost = row_costs
                .get(&row_key)
                .and_then(|usage| usage.cost_seen.then_some(usage.cost_usd))
                .or_else(|| codex_cost_usd(&model, input, cached, output));

            record_daily_usage(days, models, &day_key, &model, tokens, cost);
        }
    }
}

fn merge_pi_session_cost_cache(
    cache: PiSessionCostCache,
    since: NaiveDate,
    until: NaiveDate,
    days: &mut BTreeMap<String, LocalDayUsage>,
    models: &mut HashMap<String, LocalModelUsage>,
) {
    let Some(provider_days) = cache.days_by_provider.get("codex") else {
        return;
    };

    for (day_key, model_values) in provider_days {
        if !day_key_in_range(day_key, since, until) {
            continue;
        }
        for (raw_model, packed) in model_values {
            let model = normalize_codex_model(raw_model);
            let derived_tokens = packed
                .input_tokens
                .saturating_add(packed.cache_read_tokens)
                .saturating_add(packed.cache_write_tokens)
                .saturating_add(packed.output_tokens);
            let tokens = packed.total_tokens.max(derived_tokens).max(0);
            let has_complete_cached_cost = packed
                .usage_sample_count
                .is_some_and(|count| count > 0 && count == packed.cost_sample_count);
            let cost = if has_complete_cached_cost {
                Some(packed.cost_nanos as f64 / 1_000_000_000.0)
            } else {
                codex_cost_usd(
                    &model,
                    packed
                        .input_tokens
                        .saturating_add(packed.cache_read_tokens)
                        .saturating_add(packed.cache_write_tokens),
                    packed.cache_read_tokens,
                    packed.output_tokens,
                )
            };

            record_daily_usage(days, models, day_key, &model, tokens, cost);
        }
    }
}

fn merge_claude_cost_cache(
    cache: CodexBarCostCache,
    since: NaiveDate,
    until: NaiveDate,
    days: &mut BTreeMap<String, LocalDayUsage>,
    models: &mut HashMap<String, LocalModelUsage>,
) {
    for (day_key, model_values) in cache.days {
        if !day_key_in_range(&day_key, since, until) {
            continue;
        }
        for (raw_model, packed) in model_values {
            let model = normalize_claude_model(&raw_model);
            let input = packed.first().copied().unwrap_or_default();
            let cache_read = packed.get(1).copied().unwrap_or_default();
            let cache_create = packed.get(2).copied().unwrap_or_default();
            let output = packed.get(3).copied().unwrap_or_default();
            let cached_cost_nanos = packed.get(4).copied().unwrap_or_default();
            let sample_count = packed.get(5).copied().unwrap_or_default();
            let priced_sample_count = packed.get(6).copied().unwrap_or_default();
            let tokens = input
                .saturating_add(cache_read)
                .saturating_add(cache_create)
                .saturating_add(output)
                .max(0);
            let has_complete_cached_cost = sample_count > 0 && priced_sample_count == sample_count;
            let cost = if has_complete_cached_cost {
                Some(cached_cost_nanos as f64 / 1_000_000_000.0)
            } else {
                claude_cost_usd(&model, input, cache_read, cache_create, output)
            };

            record_daily_usage(days, models, &day_key, &model, tokens, cost);
        }
    }
}

fn merge_pi_claude_cost_cache(
    cache: PiSessionCostCache,
    since: NaiveDate,
    until: NaiveDate,
    days: &mut BTreeMap<String, LocalDayUsage>,
    models: &mut HashMap<String, LocalModelUsage>,
) {
    let Some(provider_days) = cache.days_by_provider.get("claude") else {
        return;
    };

    for (day_key, model_values) in provider_days {
        if !day_key_in_range(day_key, since, until) {
            continue;
        }
        for (raw_model, packed) in model_values {
            let model = normalize_claude_model(raw_model);
            let derived_tokens = packed
                .input_tokens
                .saturating_add(packed.cache_read_tokens)
                .saturating_add(packed.cache_write_tokens)
                .saturating_add(packed.output_tokens);
            let tokens = packed.total_tokens.max(derived_tokens).max(0);
            let has_complete_cached_cost = packed
                .usage_sample_count
                .is_some_and(|count| count > 0 && count == packed.cost_sample_count);
            let cost = if has_complete_cached_cost {
                Some(packed.cost_nanos as f64 / 1_000_000_000.0)
            } else {
                claude_cost_usd(
                    &model,
                    packed.input_tokens,
                    packed.cache_read_tokens,
                    packed.cache_write_tokens,
                    packed.output_tokens,
                )
            };

            record_daily_usage(days, models, day_key, &model, tokens, cost);
        }
    }
}

fn record_local_usage(
    row_costs: &mut HashMap<(String, String), LocalDayUsage>,
    day_key: &str,
    model: &str,
    tokens: i64,
    cost_usd: Option<f64>,
) {
    let usage = row_costs
        .entry((day_key.to_string(), model.to_string()))
        .or_default();
    usage.total_tokens = usage.total_tokens.saturating_add(tokens.max(0));
    if let Some(cost_usd) = finite_non_negative(cost_usd) {
        usage.cost_usd += cost_usd;
        usage.cost_seen = true;
    }
}

fn record_daily_usage(
    days: &mut BTreeMap<String, LocalDayUsage>,
    models: &mut HashMap<String, LocalModelUsage>,
    day_key: &str,
    model: &str,
    tokens: i64,
    cost_usd: Option<f64>,
) {
    let tokens = tokens.max(0);
    let day = days.entry(day_key.to_string()).or_default();
    day.total_tokens = day.total_tokens.saturating_add(tokens);

    let model_usage = models.entry(model.to_string()).or_default();
    model_usage.total_tokens = model_usage.total_tokens.saturating_add(tokens);

    if let Some(cost_usd) = finite_non_negative(cost_usd) {
        day.cost_usd += cost_usd;
        day.cost_seen = true;
        model_usage.cost_usd += cost_usd;
        model_usage.cost_seen = true;
    }
}

fn finite_non_negative(value: Option<f64>) -> Option<f64> {
    value.filter(|value| value.is_finite() && *value >= 0.0)
}

fn day_key_in_range(day_key: &str, since: NaiveDate, until: NaiveDate) -> bool {
    NaiveDate::parse_from_str(day_key, "%Y-%m-%d")
        .map(|date| date >= since && date <= until)
        .unwrap_or(false)
}

fn top_local_model(models: &HashMap<String, LocalModelUsage>) -> Option<String> {
    models
        .iter()
        .filter(|(_, usage)| usage.total_tokens > 0 || usage.cost_seen)
        .max_by(
            |(lhs_model, lhs), (rhs_model, rhs)| match (lhs.cost_seen, rhs.cost_seen) {
                (true, true) => lhs
                    .cost_usd
                    .partial_cmp(&rhs.cost_usd)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| lhs.total_tokens.cmp(&rhs.total_tokens))
                    .then_with(|| rhs_model.cmp(lhs_model)),
                (true, false) => std::cmp::Ordering::Greater,
                (false, true) => std::cmp::Ordering::Less,
                (false, false) => lhs
                    .total_tokens
                    .cmp(&rhs.total_tokens)
                    .then_with(|| rhs_model.cmp(lhs_model)),
            },
        )
        .map(|(model, _)| model.clone())
}

fn normalize_codex_model(raw: &str) -> String {
    let mut model = raw.trim();
    if let Some(stripped) = model.strip_prefix("openai/") {
        model = stripped;
    }
    if codex_model_pricing_exact(model).is_some() {
        return model.to_string();
    }
    if let Some(base) = strip_codex_dated_suffix(model) {
        if codex_model_pricing_exact(base).is_some() {
            return base.to_string();
        }
    }
    model.to_string()
}

fn strip_codex_dated_suffix(model: &str) -> Option<&str> {
    let bytes = model.as_bytes();
    if bytes.len() < 11 {
        return None;
    }
    let start = bytes.len() - 11;
    let suffix = &bytes[start..];
    let matches = suffix[0] == b'-'
        && suffix[1..5].iter().all(u8::is_ascii_digit)
        && suffix[5] == b'-'
        && suffix[6..8].iter().all(u8::is_ascii_digit)
        && suffix[8] == b'-'
        && suffix[9..11].iter().all(u8::is_ascii_digit);
    matches.then_some(&model[..start])
}

fn codex_cost_usd(
    model: &str,
    input_tokens: i64,
    cached_input_tokens: i64,
    output_tokens: i64,
) -> Option<f64> {
    let pricing = codex_model_pricing_exact(&normalize_codex_model(model))?;
    let input_tokens = input_tokens.max(0);
    let cached = cached_input_tokens.max(0).min(input_tokens);
    let non_cached = input_tokens.saturating_sub(cached);
    let output_tokens = output_tokens.max(0);
    let cached_rate = pricing.cache_rate.unwrap_or(pricing.input_rate);
    let uses_long_context = pricing
        .threshold_tokens
        .is_some_and(|threshold| input_tokens > threshold);
    let input_rate = if uses_long_context {
        pricing
            .input_rate_above_threshold
            .unwrap_or(pricing.input_rate)
    } else {
        pricing.input_rate
    };
    let cached_input_rate = if uses_long_context {
        pricing.cache_rate_above_threshold.unwrap_or(cached_rate)
    } else {
        cached_rate
    };
    let output_rate = if uses_long_context {
        pricing
            .output_rate_above_threshold
            .unwrap_or(pricing.output_rate)
    } else {
        pricing.output_rate
    };

    Some(
        non_cached as f64 * input_rate
            + cached as f64 * cached_input_rate
            + output_tokens as f64 * output_rate,
    )
}

fn codex_model_pricing_exact(model: &str) -> Option<CodexModelPricing> {
    let pricing = match model {
        "gpt-5" | "gpt-5-codex" | "gpt-5.1" | "gpt-5.1-codex" | "gpt-5.1-codex-max" => {
            CodexModelPricing {
                input_rate: 1.25e-6,
                output_rate: 1e-5,
                cache_rate: Some(1.25e-7),
                threshold_tokens: None,
                input_rate_above_threshold: None,
                output_rate_above_threshold: None,
                cache_rate_above_threshold: None,
            }
        }
        "gpt-5-mini" | "gpt-5.1-codex-mini" => CodexModelPricing {
            input_rate: 2.5e-7,
            output_rate: 2e-6,
            cache_rate: Some(2.5e-8),
            threshold_tokens: None,
            input_rate_above_threshold: None,
            output_rate_above_threshold: None,
            cache_rate_above_threshold: None,
        },
        "gpt-5-nano" => CodexModelPricing {
            input_rate: 5e-8,
            output_rate: 4e-7,
            cache_rate: Some(5e-9),
            threshold_tokens: None,
            input_rate_above_threshold: None,
            output_rate_above_threshold: None,
            cache_rate_above_threshold: None,
        },
        "gpt-5-pro" => CodexModelPricing {
            input_rate: 1.5e-5,
            output_rate: 1.2e-4,
            cache_rate: None,
            threshold_tokens: None,
            input_rate_above_threshold: None,
            output_rate_above_threshold: None,
            cache_rate_above_threshold: None,
        },
        "gpt-5.2" | "gpt-5.2-codex" | "gpt-5.3-codex" => CodexModelPricing {
            input_rate: 1.75e-6,
            output_rate: 1.4e-5,
            cache_rate: Some(1.75e-7),
            threshold_tokens: None,
            input_rate_above_threshold: None,
            output_rate_above_threshold: None,
            cache_rate_above_threshold: None,
        },
        "gpt-5.2-pro" => CodexModelPricing {
            input_rate: 2.1e-5,
            output_rate: 1.68e-4,
            cache_rate: None,
            threshold_tokens: None,
            input_rate_above_threshold: None,
            output_rate_above_threshold: None,
            cache_rate_above_threshold: None,
        },
        "gpt-5.3-codex-spark" => CodexModelPricing {
            input_rate: 0.0,
            output_rate: 0.0,
            cache_rate: Some(0.0),
            threshold_tokens: None,
            input_rate_above_threshold: None,
            output_rate_above_threshold: None,
            cache_rate_above_threshold: None,
        },
        "gpt-5.4" => CodexModelPricing {
            input_rate: 2.5e-6,
            output_rate: 1.5e-5,
            cache_rate: Some(2.5e-7),
            threshold_tokens: Some(272_000),
            input_rate_above_threshold: Some(5e-6),
            output_rate_above_threshold: Some(2.25e-5),
            cache_rate_above_threshold: Some(5e-7),
        },
        "gpt-5.4-mini" => CodexModelPricing {
            input_rate: 7.5e-7,
            output_rate: 4.5e-6,
            cache_rate: Some(7.5e-8),
            threshold_tokens: None,
            input_rate_above_threshold: None,
            output_rate_above_threshold: None,
            cache_rate_above_threshold: None,
        },
        "gpt-5.4-nano" => CodexModelPricing {
            input_rate: 2e-7,
            output_rate: 1.25e-6,
            cache_rate: Some(2e-8),
            threshold_tokens: None,
            input_rate_above_threshold: None,
            output_rate_above_threshold: None,
            cache_rate_above_threshold: None,
        },
        "gpt-5.4-pro" | "gpt-5.5-pro" => CodexModelPricing {
            input_rate: 3e-5,
            output_rate: 1.8e-4,
            cache_rate: None,
            threshold_tokens: None,
            input_rate_above_threshold: None,
            output_rate_above_threshold: None,
            cache_rate_above_threshold: None,
        },
        "gpt-5.5" => CodexModelPricing {
            input_rate: 5e-6,
            output_rate: 3e-5,
            cache_rate: Some(5e-7),
            threshold_tokens: Some(272_000),
            input_rate_above_threshold: Some(1e-5),
            output_rate_above_threshold: Some(4.5e-5),
            cache_rate_above_threshold: Some(1e-6),
        },
        _ => return None,
    };
    Some(pricing)
}

fn normalize_claude_model(raw: &str) -> String {
    let mut model = raw.trim().to_string();
    if let Some(stripped) = model.strip_prefix("anthropic.") {
        model = stripped.to_string();
    }
    if let Some(last_dot) = model.rfind('.') {
        let tail = &model[last_dot + 1..];
        if model.contains("claude-") && tail.starts_with("claude-") {
            model = tail.to_string();
        }
    }
    if let Some(base) = strip_claude_version_suffix(&model) {
        model = base.to_string();
    }
    if let Some(base) = strip_claude_compact_date_suffix(&model) {
        if claude_model_pricing_exact(base).is_some() {
            return base.to_string();
        }
    }
    model
}

fn strip_claude_version_suffix(model: &str) -> Option<&str> {
    let start = model.rfind("-v")?;
    let suffix = &model[start + 2..];
    let (major, minor) = suffix.split_once(':')?;
    if !major.is_empty()
        && !minor.is_empty()
        && major.bytes().all(|byte| byte.is_ascii_digit())
        && minor.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Some(&model[..start]);
    }
    None
}

fn strip_claude_compact_date_suffix(model: &str) -> Option<&str> {
    let bytes = model.as_bytes();
    if bytes.len() < 9 {
        return None;
    }
    let start = bytes.len() - 9;
    let suffix = &bytes[start..];
    (suffix[0] == b'-' && suffix[1..].iter().all(u8::is_ascii_digit)).then_some(&model[..start])
}

fn claude_cost_usd(
    model: &str,
    input_tokens: i64,
    cache_read_tokens: i64,
    cache_creation_tokens: i64,
    output_tokens: i64,
) -> Option<f64> {
    let pricing = claude_model_pricing_exact(&normalize_claude_model(model))?;
    Some(
        tiered_token_cost(
            input_tokens,
            pricing.input_rate,
            pricing.input_rate_above_threshold,
            pricing.threshold_tokens,
        ) + tiered_token_cost(
            cache_read_tokens,
            pricing.cache_read_rate,
            pricing.cache_read_rate_above_threshold,
            pricing.threshold_tokens,
        ) + tiered_token_cost(
            cache_creation_tokens,
            pricing.cache_creation_rate,
            pricing.cache_creation_rate_above_threshold,
            pricing.threshold_tokens,
        ) + tiered_token_cost(
            output_tokens,
            pricing.output_rate,
            pricing.output_rate_above_threshold,
            pricing.threshold_tokens,
        ),
    )
}

fn tiered_token_cost(
    tokens: i64,
    base_rate: f64,
    above_threshold_rate: Option<f64>,
    threshold_tokens: Option<i64>,
) -> f64 {
    let tokens = tokens.max(0);
    let Some(threshold) = threshold_tokens else {
        return tokens as f64 * base_rate;
    };
    let Some(above_threshold_rate) = above_threshold_rate else {
        return tokens as f64 * base_rate;
    };

    let below = tokens.min(threshold);
    let over = tokens.saturating_sub(threshold);
    below as f64 * base_rate + over as f64 * above_threshold_rate
}

fn claude_model_pricing_exact(model: &str) -> Option<ClaudeModelPricing> {
    let pricing = match model {
        "claude-haiku-4-5-20251001" | "claude-haiku-4-5" => ClaudeModelPricing {
            input_rate: 1e-6,
            output_rate: 5e-6,
            cache_creation_rate: 1.25e-6,
            cache_read_rate: 1e-7,
            threshold_tokens: None,
            input_rate_above_threshold: None,
            output_rate_above_threshold: None,
            cache_creation_rate_above_threshold: None,
            cache_read_rate_above_threshold: None,
        },
        "claude-opus-4-5-20251101"
        | "claude-opus-4-5"
        | "claude-opus-4-6-20260205"
        | "claude-opus-4-6"
        | "claude-opus-4-7" => ClaudeModelPricing {
            input_rate: 5e-6,
            output_rate: 2.5e-5,
            cache_creation_rate: 6.25e-6,
            cache_read_rate: 5e-7,
            threshold_tokens: None,
            input_rate_above_threshold: None,
            output_rate_above_threshold: None,
            cache_creation_rate_above_threshold: None,
            cache_read_rate_above_threshold: None,
        },
        "claude-sonnet-4-5"
        | "claude-sonnet-4-6"
        | "claude-sonnet-4-5-20250929"
        | "claude-sonnet-4-20250514" => ClaudeModelPricing {
            input_rate: 3e-6,
            output_rate: 1.5e-5,
            cache_creation_rate: 3.75e-6,
            cache_read_rate: 3e-7,
            threshold_tokens: Some(200_000),
            input_rate_above_threshold: Some(6e-6),
            output_rate_above_threshold: Some(2.25e-5),
            cache_creation_rate_above_threshold: Some(7.5e-6),
            cache_read_rate_above_threshold: Some(6e-7),
        },
        "claude-opus-4-20250514" | "claude-opus-4-1" => ClaudeModelPricing {
            input_rate: 1.5e-5,
            output_rate: 7.5e-5,
            cache_creation_rate: 1.875e-5,
            cache_read_rate: 1.5e-6,
            threshold_tokens: None,
            input_rate_above_threshold: None,
            output_rate_above_threshold: None,
            cache_creation_rate_above_threshold: None,
            cache_read_rate_above_threshold: None,
        },
        _ => return None,
    };
    Some(pricing)
}

fn codex_plan_display_text(raw: &str) -> Option<String> {
    let normalized = raw.trim().to_lowercase();
    if normalized.is_empty() {
        return None;
    }
    let display = match normalized.as_str() {
        "free" => "Free".to_string(),
        "plus" => "Plus".to_string(),
        "prolite" => "Pro 5x".to_string(),
        "pro" => "Pro 20x".to_string(),
        "team" => "Team".to_string(),
        "enterprise" => "Enterprise".to_string(),
        _ => raw
            .split(|character: char| {
                character == '_' || character == '-' || character.is_whitespace()
            })
            .filter(|part| !part.is_empty())
            .map(|part| {
                let mut characters = part.chars();
                match characters.next() {
                    Some(first) => {
                        first.to_uppercase().collect::<String>()
                            + &characters.as_str().to_lowercase()
                    }
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join(" "),
    };
    Some(display)
}

fn claude_plan_display_text(
    subscription_type: Option<&str>,
    rate_limit_tier: Option<&str>,
) -> Option<String> {
    claude_plan_from_text(subscription_type)
        .or_else(|| claude_plan_from_text(rate_limit_tier))
        .map(ToString::to_string)
}

fn claude_plan_from_text(raw: Option<&str>) -> Option<&'static str> {
    let normalized = raw?.trim().to_lowercase();
    if normalized.is_empty() {
        return None;
    }
    if normalized.contains("max") {
        return Some("Max");
    }
    if normalized.contains("pro") {
        return Some("Pro");
    }
    if normalized.contains("team") {
        return Some("Team");
    }
    if normalized.contains("enterprise") {
        return Some("Enterprise");
    }
    if normalized.contains("ultra") {
        return Some("Ultra");
    }
    None
}

fn rate_window_from_codex(window: &CodexUsageWindow) -> Option<RateWindowDetail> {
    let used_percent = window.used_percent?;
    let window_minutes = window
        .limit_window_seconds
        .map(|seconds| seconds as f64 / 60.0);
    let reset_at = window
        .reset_at
        .and_then(|seconds| DateTime::<Utc>::from_timestamp(seconds, 0));
    Some(rate_window_detail_from_parts(
        used_percent,
        window_minutes,
        reset_at,
    ))
}

fn normalize_codex_rate_windows(
    primary: Option<RateWindowDetail>,
    secondary: Option<RateWindowDetail>,
) -> (Option<RateWindowDetail>, Option<RateWindowDetail>) {
    match (primary, secondary) {
        (Some(primary), Some(secondary)) => {
            let primary_role = codex_window_role(&primary);
            let secondary_role = codex_window_role(&secondary);
            match (primary_role, secondary_role) {
                ("weekly", "session") | ("weekly", "unknown") => (Some(secondary), Some(primary)),
                _ => (Some(primary), Some(secondary)),
            }
        }
        (Some(window), None) if codex_window_role(&window) == "weekly" => (None, Some(window)),
        (None, Some(window)) if codex_window_role(&window) != "weekly" => (Some(window), None),
        other => other,
    }
}

fn codex_window_role(window: &RateWindowDetail) -> &'static str {
    match window.window_minutes.map(|value| value.round() as i64) {
        Some(300) => "session",
        Some(10080) => "weekly",
        _ => "unknown",
    }
}

async fn fetch_claude_provider(
    client: &reqwest::Client,
    keychain_access: KeychainAccess,
) -> Result<ProviderUsage, UsageFetchError> {
    // The Keychain path can wait up to 10s on a user prompt; keep that off
    // the async runtime's worker threads.
    let credentials =
        tauri::async_runtime::spawn_blocking(move || load_claude_credentials(keychain_access))
            .await
            .map_err(|error| UsageFetchError::Message(error.to_string()))?
            .map_err(UsageFetchError::Message)?;
    if claude_credentials_are_expired(&credentials) {
        // Claude Code owns these credentials. Direct refreshes can be rejected
        // by Anthropic for CLI-owned tokens, so let `claude` refresh them.
        return Err(UsageFetchError::AuthenticationRequired(
            "Claude OAuth credentials are expired. Run `claude` to refresh Claude Code credentials."
                .to_string(),
        ));
    }

    let usage = fetch_claude_usage(client, &credentials).await?;
    let plan_text = claude_plan_display_text(
        credentials.subscription_type.as_deref(),
        credentials.rate_limit_tier.as_deref(),
    )
    .or_else(|| Some("Pro".to_string()));
    Ok(claude_provider_from_usage_with_plan(usage, plan_text))
}

fn load_claude_credentials(keychain_access: KeychainAccess) -> Result<ClaudeCredentials, String> {
    if let Ok(token) = env::var("LILIMIT_CLAUDE_OAUTH_TOKEN") {
        let token = token.trim().to_string();
        if !token.is_empty() {
            return Ok(ClaudeCredentials {
                access_token: token,
                refresh_token: None,
                expires_at_ms: None,
                rate_limit_tier: None,
                subscription_type: None,
                path: None,
            });
        }
    }

    let path = home_dir()?.join(".claude").join(".credentials.json");
    match fs::read_to_string(&path) {
        Ok(contents) => {
            let mut credentials = parse_claude_credentials(&contents)?;
            credentials.path = Some(path);
            Ok(credentials)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if keychain_access == KeychainAccess::Allow {
                if let Some(contents) = read_claude_credentials_from_keychain()? {
                    return parse_claude_credentials(&contents);
                }
            }
            Err("Claude OAuth credentials not found. Run `claude` to authenticate.".to_string())
        }
        Err(error) => Err(error.to_string()),
    }
}

fn parse_claude_credentials(contents: &str) -> Result<ClaudeCredentials, String> {
    let value: Value = serde_json::from_str(contents).map_err(|error| error.to_string())?;
    let oauth = value.get("claudeAiOauth").ok_or_else(|| {
        "Claude OAuth credentials missing. Run `claude` to authenticate.".to_string()
    })?;
    let access_token = string_at(oauth, &["accessToken"]).ok_or_else(|| {
        "Claude OAuth access token missing. Run `claude` to authenticate.".to_string()
    })?;

    Ok(ClaudeCredentials {
        access_token,
        refresh_token: string_at(oauth, &["refreshToken"]),
        expires_at_ms: oauth.get("expiresAt").and_then(Value::as_f64),
        rate_limit_tier: string_at(oauth, &["rateLimitTier"]),
        subscription_type: string_at(oauth, &["subscriptionType"]),
        path: None,
    })
}

fn claude_credentials_are_expired(credentials: &ClaudeCredentials) -> bool {
    match credentials.expires_at_ms {
        Some(expires_at_ms) => {
            let now_ms = Utc::now().timestamp_millis() as f64;
            now_ms >= expires_at_ms
        }
        None => credentials.refresh_token.is_some(),
    }
}

async fn fetch_claude_usage(
    client: &reqwest::Client,
    credentials: &ClaudeCredentials,
) -> Result<ClaudeOAuthUsageResponse, UsageFetchError> {
    let response = client
        .get(CLAUDE_OAUTH_USAGE_URL)
        .bearer_auth(&credentials.access_token)
        .header(reqwest::header::ACCEPT, "application/json")
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header("anthropic-beta", "oauth-2025-04-20")
        .header(reqwest::header::USER_AGENT, "claude-code/2.1.0")
        .send()
        .await
        .map_err(|error| UsageFetchError::Message(error.to_string()))?;

    let status = response.status();
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err(UsageFetchError::RateLimited(parse_retry_after(
            response.headers().get(reqwest::header::RETRY_AFTER),
        )));
    }
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(UsageFetchError::AuthenticationRequired(
            "Claude OAuth credentials were rejected. Run `claude` to re-authenticate.".to_string(),
        ));
    }
    if !status.is_success() {
        return Err(UsageFetchError::Message(format!(
            "Claude usage API returned HTTP {}",
            status.as_u16()
        )));
    }

    response
        .json::<ClaudeOAuthUsageResponse>()
        .await
        .map_err(|error| UsageFetchError::Message(error.to_string()))
}

#[cfg(test)]
fn claude_provider_from_usage(response: ClaudeOAuthUsageResponse) -> ProviderUsage {
    claude_provider_from_usage_with_local_usage(response, Some("Pro".to_string()), None)
}

fn claude_provider_from_usage_with_plan(
    response: ClaudeOAuthUsageResponse,
    plan_text: Option<String>,
) -> ProviderUsage {
    let local_usage = load_claude_local_token_usage();
    claude_provider_from_usage_with_local_usage(response, plan_text, local_usage)
}

fn claude_provider_from_usage_with_local_usage(
    response: ClaudeOAuthUsageResponse,
    plan_text: Option<String>,
    local_usage: Option<(Option<TokenUsageSummary>, Vec<DailyUsagePoint>)>,
) -> ProviderUsage {
    const WEEK_MINUTES: f64 = 7.0 * 24.0 * 60.0;
    let (token_usage, daily_usage) = local_usage.unwrap_or((None, Vec::new()));
    let five_hour = response
        .five_hour
        .as_ref()
        .and_then(|window| rate_window_from_claude(window, Some(5.0 * 60.0)));
    let seven_day = response
        .seven_day
        .as_ref()
        .and_then(|window| rate_window_from_claude(window, Some(WEEK_MINUTES)));
    let oauth_apps = response
        .seven_day_oauth_apps
        .as_ref()
        .and_then(|window| rate_window_from_claude(window, Some(WEEK_MINUTES)));
    let model_weekly = response
        .seven_day_sonnet
        .as_ref()
        .or(response.seven_day_opus.as_ref())
        .and_then(|window| rate_window_from_claude(window, Some(WEEK_MINUTES)));

    // Each window fills at most one slot, and the labels follow the data that
    // landed in the slot instead of assuming five_hour is always present.
    let (primary, primary_title, secondary, tertiary) = if five_hour.is_some() {
        (five_hour, "Session", seven_day, model_weekly)
    } else if seven_day.is_some() {
        (seven_day, "Weekly", None, model_weekly)
    } else if oauth_apps.is_some() {
        (oauth_apps, "Weekly", None, model_weekly)
    } else {
        (model_weekly, "Model weekly", None, None)
    };

    let mut usage_rows = Vec::new();
    if let Some(window) = primary.as_ref() {
        usage_rows.push(usage_row_from_window("primary", primary_title, window));
    }
    if let Some(window) = secondary.as_ref() {
        usage_rows.push(usage_row_from_window("secondary", "Weekly", window));
    }
    if let Some(window) = tertiary.as_ref() {
        usage_rows.push(usage_row_from_window("tertiary", "Model weekly", window));
    }
    if let Some(window) = response
        .design_window()
        .and_then(|window| rate_window_from_claude(window, Some(7.0 * 24.0 * 60.0)))
    {
        usage_rows.push(usage_row_from_window("claude-design", "Designs", &window));
    }
    if let Some(window) = response
        .routines_window()
        .and_then(|window| rate_window_from_claude(window, Some(7.0 * 24.0 * 60.0)))
    {
        usage_rows.push(usage_row_from_window(
            "claude-routines",
            "Daily Routines",
            &window,
        ));
    }
    if let Some(window) = response.extra_usage.as_ref().and_then(extra_usage_window) {
        usage_rows.push(usage_row_from_window("extra-usage", "Extra usage", &window));
    }

    let reset_text = primary
        .as_ref()
        .and_then(|window| window.reset_description.clone())
        .unwrap_or_default();

    ProviderUsage {
        name: "Claude".to_string(),
        account_email: None,
        plan_text,
        session_left_percent: primary.as_ref().and_then(|window| window.percent_left),
        weekly_left_percent: secondary.as_ref().and_then(|window| window.percent_left),
        reset_text,
        updated_at: Some(now_iso_string()),
        usage_rows,
        primary,
        secondary,
        tertiary,
        credits_remaining: None,
        code_review_remaining_percent: None,
        token_usage,
        daily_usage,
        stale: false,
        error: None,
    }
}

fn rate_window_from_claude(
    window: &ClaudeOAuthWindow,
    window_minutes: Option<f64>,
) -> Option<RateWindowDetail> {
    let used_percent = window.utilization?;
    let reset_at = window.resets_at.as_deref().and_then(parse_rfc3339_datetime);
    Some(rate_window_detail_from_parts(
        used_percent,
        window_minutes,
        reset_at,
    ))
}

fn extra_usage_window(extra: &ClaudeExtraUsage) -> Option<RateWindowDetail> {
    if extra.is_enabled != Some(true) {
        return None;
    }
    let used_percent = extra.utilization.or_else(|| {
        let used = extra.used_credits?;
        let limit = extra.monthly_limit?;
        (limit > 0.0).then_some((used / limit) * 100.0)
    })?;
    Some(RateWindowDetail {
        used_percent: Some(clamp_percent(used_percent)),
        percent_left: Some(clamp_percent(100.0 - used_percent)),
        window_minutes: None,
        resets_at: None,
        reset_description: extra.currency.as_ref().and_then(|currency| {
            let used = extra.used_credits?;
            let limit = extra.monthly_limit?;
            let (used, limit) = normalize_claude_extra_usage_amounts(used, limit);
            Some(format!(
                "Monthly cap: {} / {}",
                format_currency_amount(used, currency),
                format_currency_amount(limit, currency)
            ))
        }),
    })
}

fn normalize_claude_extra_usage_amounts(used: f64, limit: f64) -> (f64, f64) {
    // Claude returns extra usage amounts in minor currency units.
    (used / 100.0, limit / 100.0)
}

fn format_currency_amount(value: f64, currency: &str) -> String {
    match currency.trim().to_uppercase().as_str() {
        "EUR" => format!("\u{20ac}{value:.2}"),
        "USD" => format!("${value:.2}"),
        "GBP" => format!("\u{00a3}{value:.2}"),
        other if !other.is_empty() => format!("{value:.2} {other}"),
        _ => format!("{value:.2}"),
    }
}

fn rate_window_detail_from_parts(
    used_percent: f64,
    window_minutes: Option<f64>,
    resets_at: Option<DateTime<Utc>>,
) -> RateWindowDetail {
    RateWindowDetail {
        used_percent: Some(clamp_percent(used_percent)),
        percent_left: Some(clamp_percent(100.0 - used_percent)),
        window_minutes,
        resets_at: resets_at.map(|date| date.to_rfc3339_opts(SecondsFormat::Secs, true)),
        reset_description: resets_at.map(reset_countdown_description),
    }
}

fn usage_row_from_window(id: &str, title: &str, window: &RateWindowDetail) -> UsageRow {
    UsageRow {
        id: id.to_string(),
        title: title.to_string(),
        percent_left: window.percent_left,
        reset_text: window.reset_description.clone(),
    }
}

fn reset_countdown_description(date: DateTime<Utc>) -> String {
    let seconds = date.signed_duration_since(Utc::now()).num_seconds();
    if seconds <= 0 {
        return "now".to_string();
    }

    let total_minutes = ((seconds as f64) / 60.0).ceil().max(1.0) as i64;
    let days = total_minutes / (24 * 60);
    let hours = (total_minutes / 60) % 24;
    let minutes = total_minutes % 60;

    if days > 0 {
        if hours > 0 {
            return format!("in {days}d {hours}h");
        }
        return format!("in {days}d");
    }
    if hours > 0 {
        if minutes > 0 {
            return format!("in {hours}h {minutes}m");
        }
        return format!("in {hours}h");
    }
    format!("in {total_minutes}m")
}

fn parse_rfc3339_datetime(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|date| date.with_timezone(&Utc))
}

fn parse_retry_after(value: Option<&reqwest::header::HeaderValue>) -> Option<DateTime<Utc>> {
    let raw = value?.to_str().ok()?.trim();
    if let Ok(seconds) = raw.parse::<i64>() {
        return Some(Utc::now() + chrono::Duration::seconds(seconds.max(0)));
    }

    DateTime::parse_from_rfc2822(raw)
        .ok()
        .map(|date| date.with_timezone(&Utc))
}

fn now_iso_string() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn home_dir() -> Result<PathBuf, String> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "HOME is not set".to_string())
}

fn string_at(value: &Value, keys: &[&str]) -> Option<String> {
    let mut current = value;
    for key in keys {
        current = current.get(*key)?;
    }
    current
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn set_string(value: &mut Value, key: &str, next: Option<String>) {
    if let Some(next) = next.filter(|value| !value.is_empty()) {
        value[key] = Value::String(next);
    }
}

#[cfg(target_os = "macos")]
fn read_claude_credentials_from_keychain() -> Result<Option<String>, String> {
    // This reads Claude Code's own OAuth item, not Chrome browser cookies.
    // Chrome cookie import requires decrypting browser storage and can trigger
    // macOS Keychain prompts, so lilimit keeps that path out of background
    // refreshes and only offers this explicit credential read.
    let mut child = Command::new("/usr/bin/security")
        .args(["find-generic-password", "-s", CLAUDE_KEYCHAIN_SERVICE, "-w"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| error.to_string())?;
    let mut stdout = child.stdout.take();
    // macOS may show a Keychain prompt for this subprocess. Give the user
    // enough time to approve it instead of failing the background refresh.
    let deadline = Instant::now() + CLAUDE_KEYCHAIN_READ_TIMEOUT;

    loop {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            if !status.success() {
                return Ok(None);
            }
            let mut output = String::new();
            if let Some(mut stdout) = stdout.take() {
                stdout
                    .read_to_string(&mut output)
                    .map_err(|error| error.to_string())?;
            }
            let trimmed = output.trim();
            return Ok((!trimmed.is_empty()).then_some(trimmed.to_string()));
        }

        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Claude Keychain read timed out.".to_string());
        }
        thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(not(target_os = "macos"))]
fn read_claude_credentials_from_keychain() -> Result<Option<String>, String> {
    Err("Keychain credential reads are only implemented on macOS.".to_string())
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
    refresh_tray_menu(app);
}

fn hide_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
    refresh_tray_menu(app);
}

fn toggle_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        match window.is_visible() {
            Ok(true) => {
                hide_main_window(app);
            }
            Ok(false) | Err(_) => {
                show_main_window(app);
            }
        }
    }
}

#[tauri::command]
fn hide_widget_window(app: AppHandle) {
    hide_main_window(&app);
}

fn sync_tray_title_from_response(app: &AppHandle, response: &UsageSnapshotResponse) {
    let settings = read_widget_settings().unwrap_or_default();
    if response.status == "ready" {
        set_tray_usage(app, &settings, Some(&response.providers));
    } else {
        set_tray_usage(app, &settings, None);
    }
}

fn sync_tray_title_from_current_snapshot(app: &AppHandle) {
    let settings = read_widget_settings().unwrap_or_default();
    let Ok(candidate) = usage_snapshot_candidate() else {
        set_tray_usage(app, &settings, None);
        return;
    };
    let path_text = candidate.path.to_string_lossy().into_owned();
    let Ok(contents) = fs::read_to_string(&candidate.path) else {
        set_tray_usage(app, &settings, None);
        return;
    };

    let response = parse_lilimit_snapshot(&contents, &path_text, candidate.source);
    sync_tray_title_from_response(app, &response);
}

fn set_tray_usage(app: &AppHandle, settings: &WidgetSettings, providers: Option<&[ProviderUsage]>) {
    match settings.toolbar_display {
        ToolbarDisplay::Text => set_tray_usage_title(app, providers.and_then(tray_usage_title)),
        ToolbarDisplay::Bars => {
            if let Some(providers) = providers {
                if let Some(icon) = tray_usage_bar_icon(providers) {
                    set_tray_usage_icon(app, icon, tray_usage_title(providers));
                    return;
                }
            }

            set_tray_usage_title(app, None);
        }
    }
}

fn tray_usage_title(providers: &[ProviderUsage]) -> Option<String> {
    let parts = providers
        .iter()
        .filter_map(|provider| {
            let used_percent = provider_tray_used_percent(provider)?;
            Some(format!(
                "{} {}%",
                tray_provider_label(&provider.name),
                used_percent.round() as i64
            ))
        })
        .collect::<Vec<_>>();

    (!parts.is_empty()).then(|| parts.join("  "))
}

fn provider_tray_used_percent(provider: &ProviderUsage) -> Option<f64> {
    provider
        .primary
        .as_ref()
        .and_then(|window| window.used_percent)
        .or_else(|| provider.session_left_percent.map(|left| 100.0 - left))
        .map(clamp_percent)
}

fn tray_provider_label(name: &str) -> &str {
    match name {
        "Codex" => "Codex",
        "Claude" => "Claude",
        _ => name,
    }
}

#[derive(Debug, Clone, PartialEq)]
struct TrayUsageMetric {
    used_percent: f64,
    color: [u8; 4],
}

fn tray_usage_metrics(providers: &[ProviderUsage]) -> Vec<TrayUsageMetric> {
    providers
        .iter()
        .filter_map(|provider| {
            provider_tray_used_percent(provider).map(|used_percent| TrayUsageMetric {
                used_percent,
                color: tray_provider_bar_color(&provider.name, provider.stale),
            })
        })
        .take(2)
        .collect()
}

fn tray_provider_bar_color(name: &str, stale: bool) -> [u8; 4] {
    let mut color = match name {
        "Codex" => [90, 213, 140, 255],
        "Claude" => [214, 126, 91, 255],
        _ => [130, 170, 210, 255],
    };
    if stale {
        color[3] = 150;
    }
    color
}

fn tray_usage_bar_icon(providers: &[ProviderUsage]) -> Option<Image<'static>> {
    let metrics = tray_usage_metrics(providers);
    (!metrics.is_empty()).then(|| render_tray_usage_bar_icon(&metrics))
}

fn render_tray_usage_bar_icon(metrics: &[TrayUsageMetric]) -> Image<'static> {
    const WIDTH: u32 = 52;
    const HEIGHT: u32 = 22;
    const TRACK_X: u32 = 4;
    const TRACK_WIDTH: u32 = 44;

    let mut rgba = vec![0_u8; (WIDTH * HEIGHT * 4) as usize];
    let rows: &[(u32, u32)] = if metrics.len() == 1 {
        &[(8, 6)]
    } else {
        &[(5, 5), (13, 5)]
    };

    for (metric, (y, height)) in metrics.iter().zip(rows.iter().copied()) {
        fill_rect(
            &mut rgba,
            WIDTH,
            TRACK_X,
            y,
            TRACK_WIDTH,
            height,
            [96, 96, 96, 110],
        );
        let fill_width = ((TRACK_WIDTH as f64) * (metric.used_percent / 100.0)).round() as u32;
        let fill_width = if metric.used_percent > 0.0 {
            fill_width.max(1)
        } else {
            0
        };
        fill_rect(
            &mut rgba,
            WIDTH,
            TRACK_X,
            y,
            fill_width.min(TRACK_WIDTH),
            height,
            metric.color,
        );
    }

    Image::new_owned(rgba, WIDTH, HEIGHT)
}

fn fill_rect(
    rgba: &mut [u8],
    image_width: u32,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    color: [u8; 4],
) {
    for yy in y..y.saturating_add(height) {
        for xx in x..x.saturating_add(width) {
            let index = ((yy * image_width + xx) * 4) as usize;
            if index + 3 < rgba.len() {
                rgba[index..index + 4].copy_from_slice(&color);
            }
        }
    }
}

fn set_tray_usage_icon(app: &AppHandle, icon: Image<'static>, title: Option<String>) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };

    let _ = tray.set_title(None::<&str>);
    let _ = tray.set_tooltip(Some(
        title
            .map(|value| format!("lilimit - {value}"))
            .unwrap_or_else(|| "lilimit".to_string()),
    ));
    let _ = tray.set_icon_with_as_template(Some(icon), false);
}

fn set_tray_usage_title(app: &AppHandle, title: Option<String>) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };

    match title {
        Some(title) => {
            let _ = tray.set_title(Some(title.as_str()));
            let _ = tray.set_tooltip(Some(format!("lilimit - {title}")));

            // macOS can render a status item as text-only, which makes the
            // usage numbers visible in the menu bar instead of the lilimit
            // icon. Linux panels generally need the icon, and GNOME Wayland
            // may still hide the title depending on AppIndicator support.
            #[cfg(target_os = "macos")]
            {
                let _ = tray.set_icon(None);
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = tray.set_icon_with_as_template(
                    Some(tauri::include_image!("./icons/tray-template.png")),
                    true,
                );
            }
        }
        None => {
            let _ = tray.set_title(None::<&str>);
            let _ = tray.set_tooltip(Some("lilimit"));

            let _ = tray.set_icon_with_as_template(
                Some(tauri::include_image!("./icons/tray-template.png")),
                true,
            );
        }
    }
}

fn main_window_is_visible<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.get_webview_window("main")
        .and_then(|window| window.is_visible().ok())
        .unwrap_or(false)
}

fn toggle_widget_menu_label<R: Runtime>(app: &AppHandle<R>) -> &'static str {
    if main_window_is_visible(app) {
        "Hide Widget"
    } else {
        "Show Widget"
    }
}

fn build_tray_menu<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let toggle = MenuItem::with_id(
        app,
        TOGGLE_WIDGET_ID,
        toggle_widget_menu_label(app),
        true,
        None::<&str>,
    )?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, QUIT_ID, "Quit lilimit", true, None::<&str>)?;

    Menu::with_items(app, &[&toggle, &separator, &quit])
}

fn refresh_tray_menu(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    if let Ok(menu) = build_tray_menu(app) {
        let _ = tray.set_menu(Some(menu));
    }
}

fn setup_tray(app: &mut tauri::App) -> tauri::Result<()> {
    let menu = build_tray_menu(app.handle())?;

    // macOS can show a text status item. GNOME support depends on the
    // shell/session: X11 and AppIndicator-capable setups are more predictable
    // than stock GNOME Wayland, and tray titles may not be displayed there.
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(tauri::include_image!("./icons/tray-template.png"))
        .icon_as_template(true)
        .tooltip("lilimit")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| {
            if event.id() == TOGGLE_WIDGET_ID {
                toggle_main_window(app);
            } else if event.id() == QUIT_ID {
                flush_pending_window_position(app.state::<Arc<PendingWindowPosition>>().inner());
                app.exit(0);
            }
        })
        .build(app)?;

    sync_tray_title_from_current_snapshot(app.handle());

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
            app.manage(Arc::new(PendingWindowPosition::default()));
            let settings = read_widget_settings().unwrap_or_default();
            if let Some(window) = app.get_webview_window("main") {
                apply_window_settings(&window, &settings, false);
            }
            setup_tray(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "settings" {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            } else if window.label() == "main" {
                if let WindowEvent::Moved(position) = event {
                    // macOS and X11 usually report move events promptly. Some
                    // Wayland compositors may delay or limit programmatic
                    // positioning; persisting the last reported value is still
                    // the best local-only behavior Tauri exposes.
                    let pending = window.app_handle().state::<Arc<PendingWindowPosition>>();
                    queue_window_position_persist(pending.inner(), position.x, position.y);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            refresh_collected_usage_snapshot,
            get_usage_snapshot,
            get_widget_settings,
            save_widget_settings,
            show_settings_window,
            hide_widget_window
        ])
        .run(tauri::generate_context!())
        .expect("failed to run lilimit");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_codex_oauth_usage_to_lilimit_provider() {
        let json = r#"{
          "email": "dev@example.com",
          "plan_type": "prolite",
          "rate_limit": {
            "primary_window": {
              "used_percent": 25,
              "reset_at": 4102444800,
              "limit_window_seconds": 18000
            },
            "secondary_window": {
              "used_percent": 40,
              "reset_at": 4103049600,
              "limit_window_seconds": 604800
            }
          },
          "credits": { "balance": "12.5" }
        }"#;
        let response: CodexUsageApiResponse = serde_json::from_str(json).unwrap();
        let provider = codex_provider_from_usage(response);

        assert_eq!(provider.name, "Codex");
        assert_eq!(provider.account_email.as_deref(), Some("dev@example.com"));
        assert_eq!(provider.plan_text.as_deref(), Some("Pro 5x"));
        assert_eq!(provider.session_left_percent, Some(75.0));
        assert_eq!(provider.weekly_left_percent, Some(60.0));
        assert_eq!(provider.credits_remaining, Some(12.5));
        assert_eq!(provider.usage_rows.len(), 2);
        assert_eq!(provider.usage_rows[0].id, "session");
    }

    #[test]
    fn maps_claude_oauth_usage_to_lilimit_provider() {
        let json = r#"{
          "five_hour": { "utilization": 58, "resets_at": "2100-01-01T00:00:00Z" },
          "seven_day": { "utilization": 20, "resets_at": "2100-01-08T00:00:00Z" },
          "seven_day_sonnet": { "utilization": 27, "resets_at": "2100-01-08T00:00:00Z" },
          "seven_day_omelette": { "utilization": 0, "resets_at": "2100-01-08T00:00:00Z" },
          "omelette_promotional": null,
          "extra_usage": {
            "is_enabled": true,
            "monthly_limit": 1700,
            "used_credits": 62,
            "utilization": 4,
            "currency": "EUR"
          }
        }"#;
        let response: ClaudeOAuthUsageResponse = serde_json::from_str(json).unwrap();
        let provider = claude_provider_from_usage(response);

        assert_eq!(provider.name, "Claude");
        assert_eq!(provider.session_left_percent, Some(42.0));
        assert_eq!(provider.weekly_left_percent, Some(80.0));
        assert_eq!(provider.usage_rows.len(), 5);
        assert!(provider
            .usage_rows
            .iter()
            .any(|row| row.id == "claude-design" && row.percent_left == Some(100.0)));
        assert!(provider.usage_rows.iter().any(|row| {
            row.id == "extra-usage"
                && row.percent_left == Some(96.0)
                && row.reset_text.as_deref() == Some("Monthly cap: \u{20ac}0.62 / \u{20ac}17.00")
        }));
    }

    #[test]
    fn does_not_duplicate_seven_day_window_when_five_hour_is_missing() {
        let json = r#"{
          "seven_day": { "utilization": 20, "resets_at": "2100-01-08T00:00:00Z" },
          "seven_day_sonnet": { "utilization": 27, "resets_at": "2100-01-08T00:00:00Z" }
        }"#;
        let response: ClaudeOAuthUsageResponse = serde_json::from_str(json).unwrap();
        let provider = claude_provider_from_usage(response);

        assert_eq!(provider.usage_rows.len(), 2);
        assert_eq!(provider.usage_rows[0].id, "primary");
        assert_eq!(provider.usage_rows[0].title, "Weekly");
        assert_eq!(provider.usage_rows[1].id, "tertiary");
        assert!(provider.secondary.is_none());
        assert_eq!(provider.session_left_percent, Some(80.0));
        assert_eq!(provider.weekly_left_percent, None);
    }

    #[test]
    fn builds_tray_usage_title_from_used_session_percent() {
        let providers = vec![
            ProviderUsage {
                name: "Codex".to_string(),
                account_email: None,
                plan_text: None,
                session_left_percent: Some(71.0),
                weekly_left_percent: None,
                reset_text: String::new(),
                updated_at: None,
                usage_rows: Vec::new(),
                primary: None,
                secondary: None,
                tertiary: None,
                credits_remaining: None,
                code_review_remaining_percent: None,
                token_usage: None,
                daily_usage: Vec::new(),
                stale: false,
                error: None,
            },
            ProviderUsage {
                name: "Claude".to_string(),
                account_email: None,
                plan_text: None,
                session_left_percent: Some(42.0),
                weekly_left_percent: None,
                reset_text: String::new(),
                updated_at: None,
                usage_rows: Vec::new(),
                primary: Some(RateWindowDetail {
                    used_percent: Some(58.0),
                    percent_left: Some(42.0),
                    window_minutes: None,
                    resets_at: None,
                    reset_description: None,
                }),
                secondary: None,
                tertiary: None,
                credits_remaining: None,
                code_review_remaining_percent: None,
                token_usage: None,
                daily_usage: Vec::new(),
                stale: false,
                error: None,
            },
        ];

        assert_eq!(
            tray_usage_title(&providers),
            Some("Codex 29%  Claude 58%".to_string())
        );
    }

    #[test]
    fn builds_tray_usage_bar_metrics_from_used_session_percent() {
        let providers = vec![
            ProviderUsage {
                name: "Codex".to_string(),
                account_email: None,
                plan_text: None,
                session_left_percent: Some(71.0),
                weekly_left_percent: None,
                reset_text: String::new(),
                updated_at: None,
                usage_rows: Vec::new(),
                primary: None,
                secondary: None,
                tertiary: None,
                credits_remaining: None,
                code_review_remaining_percent: None,
                token_usage: None,
                daily_usage: Vec::new(),
                stale: false,
                error: None,
            },
            ProviderUsage {
                name: "Claude".to_string(),
                account_email: None,
                plan_text: None,
                session_left_percent: Some(42.0),
                weekly_left_percent: None,
                reset_text: String::new(),
                updated_at: None,
                usage_rows: Vec::new(),
                primary: Some(RateWindowDetail {
                    used_percent: Some(58.0),
                    percent_left: Some(42.0),
                    window_minutes: None,
                    resets_at: None,
                    reset_description: None,
                }),
                secondary: None,
                tertiary: None,
                credits_remaining: None,
                code_review_remaining_percent: None,
                token_usage: None,
                daily_usage: Vec::new(),
                stale: false,
                error: None,
            },
        ];

        let metrics = tray_usage_metrics(&providers);

        assert_eq!(metrics.len(), 2);
        assert_eq!(metrics[0].used_percent, 29.0);
        assert_eq!(metrics[1].used_percent, 58.0);
    }

    #[test]
    fn clamps_expanded_window_inside_screen_bounds() {
        let position =
            clamp_position_to_bounds(PhysicalPosition::new(600, 30), 360, 560, 0, 24, 668, 1200);

        assert_eq!(position, PhysicalPosition::new(308, 30));
    }

    #[test]
    fn anchors_right_edge_when_switching_display_modes() {
        let expanded = right_edge_anchored_position(PhysicalPosition::new(600, 30), 280, 360);
        let collapsed = right_edge_anchored_position(expanded, 360, 280);

        assert_eq!(expanded, PhysicalPosition::new(520, 30));
        assert_eq!(collapsed, PhysicalPosition::new(600, 30));
    }
}
