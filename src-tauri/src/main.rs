use std::{env, fs, path::PathBuf, time::Duration};
#[cfg(target_os = "macos")]
use std::{
    io::Read,
    process::{Command, Stdio},
    thread,
    time::Instant,
};

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
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
const COLLECTED_USAGE_FILE: &str = "collected_snapshot.json";
const SIMPLE_WIDTH: f64 = 280.0;
const SIMPLE_HEIGHT: f64 = 140.0;
const FULL_WIDTH: f64 = 360.0;
const FULL_HEIGHT: f64 = 560.0;
const CODEX_AUTH_REFRESH_AFTER_DAYS: i64 = 8;
const CODEX_REFRESH_ENDPOINT: &str = "https://auth.openai.com/oauth/token";
const CODEX_OAUTH_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const CODEX_DEFAULT_CHATGPT_BASE_URL: &str = "https://chatgpt.com/backend-api";
const CLAUDE_OAUTH_USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const CLAUDE_REFRESH_ENDPOINT: &str = "https://platform.claude.com/v1/oauth/token";
const CLAUDE_OAUTH_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
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
    #[serde(default)]
    window_position: Option<WindowPosition>,
}

impl Default for WidgetSettings {
    fn default() -> Self {
        Self {
            display_mode: DisplayMode::Simple,
            background: WidgetBackground::Dark,
            keychain_access: KeychainAccess::Off,
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

#[derive(Debug, Deserialize, Serialize)]
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
    source: Option<String>,
    updated_at: Option<String>,
    shows_used_percent: bool,
    providers: Vec<ProviderUsage>,
    error: Option<String>,
}

fn usage_snapshot_candidate() -> Result<SnapshotCandidate, String> {
    Ok(SnapshotCandidate {
        path: lilimit_config_dir()?.join(COLLECTED_USAGE_FILE),
        source: SnapshotSource::LilimitCollected,
    })
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
    let candidate = match usage_snapshot_candidate() {
        Ok(candidate) => candidate,
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

    let path_text = candidate.path.to_string_lossy().into_owned();
    let contents = match fs::read_to_string(&candidate.path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return UsageSnapshotResponse {
                status: "missing".to_string(),
                path: path_text,
                source: None,
                updated_at: None,
                shows_used_percent: false,
                providers: Vec::new(),
                error: None,
            }
        }
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

    parse_lilimit_snapshot(&contents, &path_text, candidate.source)
}

fn parse_lilimit_snapshot(
    contents: &str,
    path_text: &str,
    source: SnapshotSource,
) -> UsageSnapshotResponse {
    match serde_json::from_str::<UsageSnapshotFile>(&contents) {
        Ok(snapshot) => UsageSnapshotResponse {
            status: "ready".to_string(),
            path: path_text.to_string(),
            source: Some(source.as_str().to_string()),
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
            source: Some(source.as_str().to_string()),
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
    Message(String),
}

impl std::fmt::Display for UsageFetchError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UsageFetchError::Unauthorized => write!(formatter, "unauthorized"),
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
    scopes: Vec<String>,
    rate_limit_tier: Option<String>,
    subscription_type: Option<String>,
    path: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
struct CodexUsageApiResponse {
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
async fn refresh_collected_usage_snapshot() -> Result<UsageSnapshotResponse, String> {
    let settings = read_widget_settings().unwrap_or_default();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent("lilimit")
        .build()
        .map_err(|error| error.to_string())?;

    let mut providers = Vec::new();
    let mut errors = Vec::new();

    match fetch_codex_provider(&client).await {
        Ok(provider) => providers.push(provider),
        Err(error) => errors.push(format!("Codex: {error}")),
    }

    match fetch_claude_provider(&client, settings.keychain_access).await {
        Ok(provider) => providers.push(provider),
        Err(error) => errors.push(format!("Claude: {error}")),
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
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let contents = serde_json::to_string_pretty(snapshot).map_err(|error| error.to_string())?;
    fs::write(path, contents).map_err(|error| error.to_string())
}

async fn fetch_codex_provider(client: &reqwest::Client) -> Result<ProviderUsage, String> {
    let mut credentials = load_codex_credentials()?;
    if codex_credentials_need_refresh(&credentials) {
        credentials = refresh_codex_credentials(client, &credentials)
            .await
            .map_err(|error| error.to_string())?;
        save_codex_credentials(&credentials)?;
    }

    match fetch_codex_usage(client, &credentials).await {
        Ok(response) => Ok(codex_provider_from_usage(response)),
        Err(UsageFetchError::Unauthorized) if !credentials.refresh_token.is_empty() => {
            let refreshed = refresh_codex_credentials(client, &credentials)
                .await
                .map_err(|error| error.to_string())?;
            save_codex_credentials(&refreshed)?;
            fetch_codex_usage(client, &refreshed)
                .await
                .map(codex_provider_from_usage)
                .map_err(|error| error.to_string())
        }
        Err(error) => Err(error.to_string()),
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
    fs::write(&credentials.path, serialized).map_err(|error| error.to_string())
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

    ProviderUsage {
        name: "Codex".to_string(),
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
        token_usage: None,
        daily_usage: Vec::new(),
    }
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
) -> Result<ProviderUsage, String> {
    let mut credentials = load_claude_credentials(keychain_access)?;
    if claude_credentials_are_expired(&credentials) {
        credentials = refresh_claude_credentials(client, &credentials)
            .await
            .map_err(|error| error.to_string())?;
        if credentials.path.is_some() {
            save_claude_credentials(&credentials)?;
        }
    }

    let usage = fetch_claude_usage(client, &credentials)
        .await
        .map_err(|error| error.to_string())?;
    Ok(claude_provider_from_usage(usage))
}

fn load_claude_credentials(keychain_access: KeychainAccess) -> Result<ClaudeCredentials, String> {
    if let Ok(token) = env::var("CODEXBAR_CLAUDE_OAUTH_TOKEN") {
        let token = token.trim().to_string();
        if !token.is_empty() {
            return Ok(ClaudeCredentials {
                access_token: token,
                refresh_token: None,
                expires_at_ms: None,
                scopes: vec!["user:profile".to_string()],
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

    let scopes = oauth
        .get("scopes")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Ok(ClaudeCredentials {
        access_token,
        refresh_token: string_at(oauth, &["refreshToken"]),
        expires_at_ms: oauth.get("expiresAt").and_then(Value::as_f64),
        scopes,
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

async fn refresh_claude_credentials(
    client: &reqwest::Client,
    credentials: &ClaudeCredentials,
) -> Result<ClaudeCredentials, UsageFetchError> {
    let refresh_token = credentials.refresh_token.as_ref().ok_or_else(|| {
        UsageFetchError::Message(
            "Claude OAuth refresh token missing. Run `claude` to authenticate.".to_string(),
        )
    })?;

    let response = client
        .post(CLAUDE_REFRESH_ENDPOINT)
        .header(reqwest::header::ACCEPT, "application/json")
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token.as_str()),
            ("client_id", CLAUDE_OAUTH_CLIENT_ID),
        ])
        .send()
        .await
        .map_err(|error| UsageFetchError::Message(error.to_string()))?;

    let status = response.status();
    if !status.is_success() {
        return Err(UsageFetchError::Message(format!(
            "Claude OAuth refresh failed with HTTP {}. Run `claude` to authenticate.",
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
        string_at(&body, &["refresh_token"]).or_else(|| credentials.refresh_token.clone());
    let expires_at_ms = body
        .get("expires_in")
        .and_then(Value::as_i64)
        .map(|seconds| (Utc::now().timestamp() + seconds) as f64 * 1000.0)
        .or(credentials.expires_at_ms);

    Ok(ClaudeCredentials {
        access_token,
        refresh_token,
        expires_at_ms,
        scopes: credentials.scopes.clone(),
        rate_limit_tier: credentials.rate_limit_tier.clone(),
        subscription_type: credentials.subscription_type.clone(),
        path: credentials.path.clone(),
    })
}

fn save_claude_credentials(credentials: &ClaudeCredentials) -> Result<(), String> {
    let path = credentials
        .path
        .as_ref()
        .ok_or_else(|| "Claude credentials path is unavailable.".to_string())?;
    let contents = fs::read_to_string(path).unwrap_or_else(|_| "{}".to_string());
    let mut value: Value = serde_json::from_str(&contents).unwrap_or_else(|_| json!({}));
    let mut oauth = value
        .get("claudeAiOauth")
        .cloned()
        .unwrap_or_else(|| json!({}));

    set_string(
        &mut oauth,
        "accessToken",
        Some(credentials.access_token.clone()),
    );
    set_string(
        &mut oauth,
        "refreshToken",
        credentials.refresh_token.clone(),
    );
    if let Some(expires_at_ms) = credentials.expires_at_ms {
        oauth["expiresAt"] = json!(expires_at_ms);
    }
    oauth["scopes"] = json!(credentials.scopes);
    set_string(
        &mut oauth,
        "rateLimitTier",
        credentials.rate_limit_tier.clone(),
    );
    set_string(
        &mut oauth,
        "subscriptionType",
        credentials.subscription_type.clone(),
    );
    value["claudeAiOauth"] = oauth;

    let serialized = serde_json::to_string_pretty(&value).map_err(|error| error.to_string())?;
    fs::write(path, serialized).map_err(|error| error.to_string())
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
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(UsageFetchError::Unauthorized);
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

fn claude_provider_from_usage(response: ClaudeOAuthUsageResponse) -> ProviderUsage {
    let primary = response
        .five_hour
        .as_ref()
        .and_then(|window| rate_window_from_claude(window, Some(5.0 * 60.0)))
        .or_else(|| {
            response
                .seven_day
                .as_ref()
                .and_then(|window| rate_window_from_claude(window, Some(7.0 * 24.0 * 60.0)))
        })
        .or_else(|| {
            response
                .seven_day_oauth_apps
                .as_ref()
                .and_then(|window| rate_window_from_claude(window, Some(7.0 * 24.0 * 60.0)))
        })
        .or_else(|| {
            response
                .seven_day_sonnet
                .as_ref()
                .or(response.seven_day_opus.as_ref())
                .and_then(|window| rate_window_from_claude(window, Some(7.0 * 24.0 * 60.0)))
        });

    let secondary = response
        .seven_day
        .as_ref()
        .and_then(|window| rate_window_from_claude(window, Some(7.0 * 24.0 * 60.0)));
    let tertiary = response
        .seven_day_sonnet
        .as_ref()
        .or(response.seven_day_opus.as_ref())
        .and_then(|window| rate_window_from_claude(window, Some(7.0 * 24.0 * 60.0)));

    let mut usage_rows = Vec::new();
    if let Some(window) = primary.as_ref() {
        usage_rows.push(usage_row_from_window("primary", "Session", window));
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
        token_usage: None,
        daily_usage: Vec::new(),
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
            Some(format!("{used:.0}/{limit:.0} {currency}"))
        }),
    })
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
        // macOS renders this as visible status-item text next to the icon,
        // which makes lilimit easy to find even when template icons blend
        // into a crowded menu bar. Linux panel support varies by shell.
        .title("li")
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
            refresh_collected_usage_snapshot,
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
    fn maps_codex_oauth_usage_to_lilimit_provider() {
        let json = r#"{
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
          "omelette_promotional": null
        }"#;
        let response: ClaudeOAuthUsageResponse = serde_json::from_str(json).unwrap();
        let provider = claude_provider_from_usage(response);

        assert_eq!(provider.name, "Claude");
        assert_eq!(provider.session_left_percent, Some(42.0));
        assert_eq!(provider.weekly_left_percent, Some(80.0));
        assert_eq!(provider.usage_rows.len(), 4);
        assert!(provider
            .usage_rows
            .iter()
            .any(|row| row.id == "claude-design" && row.percent_left == Some(100.0)));
    }
}
