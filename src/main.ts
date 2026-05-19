import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import "./styles.css";

type UsageStatus = "ready" | "missing" | "invalidJson" | "ioError";
type UsageSource = "lilimitCollected";
type DisplayMode = "simple" | "full";
type WidgetBackground = "dark" | "light";
type KeychainAccess = "off" | "allow";

type WindowPosition = {
  x: number;
  y: number;
};

type WidgetSettings = {
  displayMode: DisplayMode;
  background: WidgetBackground;
  keychainAccess: KeychainAccess;
  windowPosition: WindowPosition | null;
};

type UsageRow = {
  id: string;
  title: string;
  percentLeft: number | null;
  resetText: string | null;
};

type RateWindowDetail = {
  usedPercent: number | null;
  percentLeft: number | null;
  windowMinutes: number | null;
  resetsAt: string | null;
  resetDescription: string | null;
};

type TokenUsageSummary = {
  sessionCostUSD: number | null;
  sessionTokens: number | null;
  last30DaysCostUSD: number | null;
  last30DaysTokens: number | null;
};

type DailyUsagePoint = {
  dayKey: string;
  totalTokens: number | null;
  costUSD: number | null;
};

type ProviderUsage = {
  name: string;
  sessionLeftPercent: number | null;
  weeklyLeftPercent: number | null;
  resetText: string;
  updatedAt: string | null;
  usageRows: UsageRow[];
  primary: RateWindowDetail | null;
  secondary: RateWindowDetail | null;
  tertiary: RateWindowDetail | null;
  creditsRemaining: number | null;
  codeReviewRemainingPercent: number | null;
  tokenUsage: TokenUsageSummary | null;
  dailyUsage: DailyUsagePoint[];
};

type UsageSnapshot = {
  status: UsageStatus;
  path: string;
  source: UsageSource | null;
  updatedAt: string | null;
  showsUsedPercent: boolean;
  providers: ProviderUsage[];
  error: string | null;
};

const REFRESH_MS = 30_000;
const STALE_MS = 15 * 60 * 1000;
const DEFAULT_SETTINGS: WidgetSettings = {
  displayMode: "simple",
  background: "dark",
  keychainAccess: "off",
  windowPosition: null,
};
const currentWindow = getCurrentWindow();
const isSettingsWindow = currentWindow.label === "settings";

const app = document.querySelector<HTMLDivElement>("#app");

if (!app) {
  throw new Error("Missing #app root");
}

const appRoot = app;
let currentSettings: WidgetSettings = DEFAULT_SETTINGS;
let latestSnapshot: UsageSnapshot | null = null;
let settingsError: string | null = null;
let collectionError: string | null = null;
let refreshInProgress = false;

function clampPercent(value: number | null): number | null {
  if (value === null || !Number.isFinite(value)) {
    return null;
  }

  return Math.max(0, Math.min(100, Math.round(value)));
}

function escapeHtml(value: string): string {
  const escapes: Record<string, string> = {
    "&": "&amp;",
    "<": "&lt;",
    ">": "&gt;",
    '"': "&quot;",
    "'": "&#39;",
  };

  return value.replace(/[&<>"']/g, (char) => escapes[char] ?? char);
}

function isStale(updatedAt: string | null): boolean {
  if (!updatedAt) {
    return false;
  }

  const updatedMs = Date.parse(updatedAt);
  return Number.isFinite(updatedMs) && Date.now() - updatedMs > STALE_MS;
}

function formatUpdatedAt(updatedAt: string | null): string {
  if (!updatedAt) {
    return "never";
  }

  const date = new Date(updatedAt);
  if (Number.isNaN(date.getTime())) {
    return "invalid time";
  }

  return new Intl.DateTimeFormat(undefined, {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  }).format(date);
}

function formatAge(updatedAt: string | null): string {
  if (!updatedAt) {
    return "never";
  }

  const updatedMs = Date.parse(updatedAt);
  if (!Number.isFinite(updatedMs)) {
    return "invalid time";
  }

  const diffSeconds = Math.max(0, Math.round((Date.now() - updatedMs) / 1000));
  if (diffSeconds < 60) {
    return "just now";
  }

  const diffMinutes = Math.round(diffSeconds / 60);
  if (diffMinutes < 60) {
    return `${diffMinutes}m ago`;
  }

  const diffHours = Math.round(diffMinutes / 60);
  if (diffHours < 24) {
    return `${diffHours}h ago`;
  }

  const diffDays = Math.round(diffHours / 24);
  return `${diffDays}d ago`;
}

function providerTone(percent: number): string {
  if (percent <= 25) {
    return "low";
  }

  if (percent <= 50) {
    return "medium";
  }

  return "high";
}

function providerColor(name: string): string {
  switch (name.toLowerCase()) {
    case "codex":
      return "#49a3b0";
    case "claude":
      return "#cc7c5e";
    default:
      return "#67d391";
  }
}

function formatPercent(value: number | null): string {
  return value === null ? "n/a" : `${value}%`;
}

function formatUsagePercent(value: number | null, showsUsedPercent: boolean): string {
  if (value === null) {
    return "n/a";
  }

  if (showsUsedPercent) {
    return `${100 - value}% used`;
  }

  return `${value}% left`;
}

function meterWidth(value: number | null, showsUsedPercent: boolean): number {
  if (value === null) {
    return 0;
  }

  return showsUsedPercent ? 100 - value : value;
}

function sourceLabel(source: UsageSource | null): string {
  switch (source) {
    case "lilimitCollected":
      return "lilimit collector";
    default:
      return "local";
  }
}

function collectionWarning(): string {
  if (!collectionError) {
    return "";
  }

  const hasCodex = collectionError.includes("Codex:");
  const hasClaude = collectionError.includes("Claude:");
  const label = hasClaude && !hasCodex ? "Claude unavailable" : "Partial data";

  return `<span class="collection-warning" title="${escapeHtml(collectionError)}">${escapeHtml(label)}</span>`;
}

function sortProviders(providers: ProviderUsage[]): ProviderUsage[] {
  const order = new Map([
    ["codex", 0],
    ["claude", 1],
  ]);

  return [...providers].sort((a, b) => {
    const aOrder = order.get(a.name.toLowerCase()) ?? 99;
    const bOrder = order.get(b.name.toLowerCase()) ?? 99;
    return aOrder - bOrder || a.name.localeCompare(b.name);
  });
}

function formatUsd(value: number | null): string {
  if (value === null || !Number.isFinite(value)) {
    return "-";
  }

  return new Intl.NumberFormat(undefined, {
    style: "currency",
    currency: "USD",
    maximumFractionDigits: 2,
  }).format(value);
}

function formatTokens(value: number | null): string {
  if (value === null || !Number.isFinite(value)) {
    return "-";
  }

  const abs = Math.abs(value);
  if (abs >= 1_000_000_000) {
    return `${Math.round(value / 1_000_000_000)}B`;
  }
  if (abs >= 1_000_000) {
    return `${Math.round(value / 1_000_000)}M`;
  }
  if (abs >= 1_000) {
    return `${Math.round(value / 1_000)}K`;
  }
  return `${Math.round(value)}`;
}

function normalizeSettings(settings: Partial<WidgetSettings> | null): WidgetSettings {
  return {
    displayMode: settings?.displayMode === "full" ? "full" : "simple",
    background: settings?.background === "light" ? "light" : "dark",
    keychainAccess: settings?.keychainAccess === "allow" ? "allow" : "off",
    windowPosition: settings?.windowPosition ?? null,
  };
}

function fallbackRows(provider: ProviderUsage): UsageRow[] {
  const rows: UsageRow[] = [];

  if (provider.sessionLeftPercent !== null) {
    rows.push({
      id: "session",
      title: "Session",
      percentLeft: provider.sessionLeftPercent,
      resetText: provider.resetText || null,
    });
  }

  if (provider.weeklyLeftPercent !== null) {
    rows.push({
      id: "weekly",
      title: "Weekly",
      percentLeft: provider.weeklyLeftPercent,
      resetText: null,
    });
  }

  return rows;
}

function renderSettingsPanel(): string {
  const mode = currentSettings.displayMode;
  const background = currentSettings.background;
  const keychainAccess = currentSettings.keychainAccess;

  return `
    <div class="settings-panel">
      <div class="setting-group">
        <span>Display</span>
        <div class="segmented">
          <button type="button" data-display-mode="simple" class="${mode === "simple" ? "active" : ""}">Simple</button>
          <button type="button" data-display-mode="full" class="${mode === "full" ? "active" : ""}">Full</button>
        </div>
      </div>
      <div class="setting-group">
        <span>Background</span>
        <div class="segmented">
          <button type="button" data-background="dark" class="${background === "dark" ? "active" : ""}">Dark</button>
          <button type="button" data-background="light" class="${background === "light" ? "active" : ""}">Light</button>
        </div>
      </div>
      <div class="setting-group">
        <span>Keychain</span>
        <div class="segmented">
          <button type="button" data-keychain="off" class="${keychainAccess === "off" ? "active" : ""}">Off</button>
          <button type="button" data-keychain="allow" class="${keychainAccess === "allow" ? "active" : ""}">Allow</button>
        </div>
      </div>
      <button class="settings-action settings-refresh-button" type="button">Refresh now</button>
      ${settingsError ? `<p class="settings-error">${escapeHtml(settingsError)}</p>` : ""}
    </div>
  `;
}

function renderSettingsWindow(): void {
  appRoot.innerHTML = `
    <main class="surface settings-view ${currentSettings.background}-bg">
      <header class="settings-header">
        <h1>lilimit settings</h1>
      </header>
      ${renderSettingsPanel()}
      <button class="settings-action close-settings-button" type="button">Done</button>
    </main>
  `;
  bindInteractions();
}

function renderRefreshIcon(): string {
  return `
    <svg class="refresh-icon" viewBox="0 0 24 24" aria-hidden="true" focusable="false">
      <path d="M21 12a9 9 0 1 1-2.64-6.36"></path>
      <path d="M21 3v6h-6"></path>
    </svg>
  `;
}

function renderTitlebar(stale = false): string {
  return `
    <header class="titlebar" data-tauri-drag-region>
      <h1 data-tauri-drag-region>lilimit</h1>
      <div class="title-actions">
        ${stale ? '<span class="stale">stale</span>' : ""}
        <button class="icon-button refresh-button${refreshInProgress ? " refreshing" : ""}" type="button" aria-label="Refresh usage data" title="Refresh" ${refreshInProgress ? "disabled" : ""}>${renderRefreshIcon()}</button>
        <button class="icon-button settings-button" type="button" aria-label="Widget settings" title="Settings">...</button>
        <button class="icon-button close-button" type="button" aria-label="Close lilimit" title="Close">x</button>
      </div>
    </header>
  `;
}

function renderSimpleProvider(provider: ProviderUsage): string {
  const session = clampPercent(provider.sessionLeftPercent);
  const weekly = clampPercent(provider.weeklyLeftPercent);
  const tone = session === null ? "unknown" : providerTone(session);
  const name = escapeHtml(provider.name);
  const resetText = escapeHtml(provider.resetText || "reset n/a");
  const width = session ?? 0;

  return `
    <section class="provider" aria-label="${name} usage">
      <div class="provider-line">
        <strong>${name}</strong>
        <span class="numbers">
          <span>${formatPercent(session)} session</span>
          <span>${formatPercent(weekly)} week</span>
          <span>${resetText}</span>
        </span>
      </div>
      <div class="meter" aria-label="${name} session left">
        <div class="meter-fill ${tone}" style="width: ${width}%"></div>
      </div>
    </section>
  `;
}

function renderFullUsageRow(row: UsageRow, color: string, showsUsedPercent: boolean): string {
  const percent = clampPercent(row.percentLeft);
  const width = meterWidth(percent, showsUsedPercent);
  const title = escapeHtml(row.title);
  const reset = row.resetText ? escapeHtml(row.resetText) : "";

  return `
    <div class="full-usage-row">
      <div class="full-row-heading">
        <strong>${title}</strong>
        <span>${formatUsagePercent(percent, showsUsedPercent)}</span>
      </div>
      <div class="meter full-meter">
        <div class="meter-fill" style="width: ${width}%; background: ${color}"></div>
      </div>
      ${reset ? `<div class="row-reset">Reset ${reset}</div>` : ""}
    </div>
  `;
}

function renderMetric(title: string, value: string): string {
  return `
    <div class="metric">
      <span>${escapeHtml(title)}</span>
      <strong>${escapeHtml(value)}</strong>
    </div>
  `;
}

function renderTokenMetrics(tokenUsage: TokenUsageSummary | null): string {
  if (!tokenUsage) {
    return "";
  }

  return `
    <div class="metrics">
      ${renderMetric("Today", formatUsd(tokenUsage.sessionCostUSD))}
      ${renderMetric("30d cost", formatUsd(tokenUsage.last30DaysCostUSD))}
      ${renderMetric("Latest tokens", formatTokens(tokenUsage.sessionTokens))}
      ${renderMetric("30d tokens", formatTokens(tokenUsage.last30DaysTokens))}
    </div>
  `;
}

function renderHistory(points: DailyUsagePoint[], color: string): string {
  const visiblePoints = points.slice(-24);
  if (visiblePoints.length === 0) {
    return "";
  }

  const values = visiblePoints.map((point) => point.costUSD ?? point.totalTokens ?? 0);
  const maxValue = Math.max(...values, 0);

  return `
    <div class="history" aria-label="Daily usage history">
      ${visiblePoints
        .map((point, index) => {
          const value = values[index] ?? 0;
          const height = maxValue > 0 ? Math.max(8, Math.round((value / maxValue) * 100)) : 8;
          const label = `${point.dayKey}: ${formatUsd(point.costUSD)} / ${formatTokens(point.totalTokens)} tokens`;
          return `<span title="${escapeHtml(label)}" style="height: ${height}%; background: ${color}"></span>`;
        })
        .join("")}
    </div>
  `;
}

function renderFullProvider(provider: ProviderUsage, showsUsedPercent: boolean): string {
  const color = providerColor(provider.name);
  const rows = provider.usageRows.length > 0 ? provider.usageRows : fallbackRows(provider);
  const name = escapeHtml(provider.name);
  const updatedAt = provider.updatedAt ?? latestSnapshot?.updatedAt ?? null;
  const usageRows = [
    ...rows.map((row) => renderFullUsageRow(row, color, showsUsedPercent)),
    provider.codeReviewRemainingPercent !== null
      ? renderFullUsageRow(
          {
            id: "codeReview",
            title: "Code review",
            percentLeft: provider.codeReviewRemainingPercent,
            resetText: null,
          },
          color,
          false,
        )
      : "",
  ].join("");

  return `
    <section class="full-provider" style="--provider-color: ${color}">
      <div class="full-provider-header">
        <div>
          <h2>${name}</h2>
          <span>Updated ${formatAge(updatedAt)}</span>
        </div>
        ${
          provider.creditsRemaining !== null
            ? `<span class="provider-badge">${escapeHtml(formatTokens(provider.creditsRemaining))} credits</span>`
            : ""
        }
      </div>
      <div class="full-rows">
        ${usageRows || '<p class="empty">No usage rows</p>'}
      </div>
      ${renderTokenMetrics(provider.tokenUsage)}
      ${renderHistory(provider.dailyUsage, color)}
      ${provider.tokenUsage ? '<p class="usage-note">Estimated from local logs - may differ from your bill</p>' : ""}
    </section>
  `;
}

function renderState(snapshot: UsageSnapshot): string {
  const collectorError =
    snapshot.status === "missing" && collectionError
      ? `<span class="state-error">Collection failed: ${escapeHtml(collectionError)}</span>`
      : "";
  const detail = (() => {
    if (snapshot.status === "missing") {
      return {
        title: "No usage data yet",
        body: snapshot.path,
      };
    }

    if (snapshot.status === "invalidJson") {
      return {
        title: "Invalid usage JSON",
        body: snapshot.error ?? snapshot.path,
      };
    }

    return {
      title: "Unable to read usage data",
      body: snapshot.error ?? snapshot.path,
    };
  })();

  return `
    <main class="surface state ${currentSettings.displayMode}-view ${currentSettings.background}-bg" data-tauri-drag-region>
      ${renderTitlebar(false)}
      <div class="state-body" data-tauri-drag-region>
        <p>${escapeHtml(detail.title)}</p>
        <code>${escapeHtml(detail.body)}</code>
        ${collectorError}
      </div>
    </main>
  `;
}

function renderReady(snapshot: UsageSnapshot): string {
  const stale = isStale(snapshot.updatedAt);
  const providers = sortProviders(snapshot.providers);
  const mode = currentSettings.displayMode;

  if (mode === "full") {
    return `
      <main class="surface widget full-view ${currentSettings.background}-bg" data-tauri-drag-region>
        ${renderTitlebar(stale)}
        <div class="full-content">
          ${
            providers.length > 0
              ? providers.map((provider) => renderFullProvider(provider, snapshot.showsUsedPercent)).join("")
              : '<p class="empty">No provider data</p>'
          }
        </div>
        <footer data-tauri-drag-region>
          <span>Updated ${formatUpdatedAt(snapshot.updatedAt)} / ${sourceLabel(snapshot.source)}</span>
          ${collectionWarning()}
        </footer>
      </main>
    `;
  }

  return `
    <main class="surface widget simple-view ${currentSettings.background}-bg" data-tauri-drag-region>
      ${renderTitlebar(stale)}
      <div class="providers">
        ${
          providers.length > 0
            ? providers.map(renderSimpleProvider).join("")
            : '<p class="empty">No provider data</p>'
        }
      </div>
      <footer data-tauri-drag-region>
        <span>Updated ${formatUpdatedAt(snapshot.updatedAt)} / ${sourceLabel(snapshot.source)}</span>
        ${collectionWarning()}
      </footer>
    </main>
  `;
}

function renderLoading(): void {
  appRoot.innerHTML = `
    <main class="surface state ${currentSettings.displayMode}-view ${currentSettings.background}-bg" data-tauri-drag-region>
      ${renderTitlebar(false)}
      <div class="state-body" data-tauri-drag-region>
        <p>Loading usage data</p>
      </div>
    </main>
  `;
  bindInteractions();
}

function renderApp(snapshot: UsageSnapshot): void {
  latestSnapshot = snapshot;
  appRoot.innerHTML = snapshot.status === "ready" ? renderReady(snapshot) : renderState(snapshot);
  bindInteractions();
}

function renderCurrent(): void {
  if (latestSnapshot) {
    renderApp(latestSnapshot);
  } else {
    renderLoading();
  }
}

function errorSnapshot(error: unknown): UsageSnapshot {
  return {
    status: "ioError",
    path: "",
    source: null,
    updatedAt: null,
    showsUsedPercent: false,
    providers: [],
    error: error instanceof Error ? error.message : String(error),
  };
}

async function saveSettings(patch: Partial<WidgetSettings>): Promise<void> {
  const nextSettings = normalizeSettings({ ...currentSettings, ...patch });
  currentSettings = nextSettings;
  settingsError = null;
  renderAfterSettingsChange();

  try {
    currentSettings = normalizeSettings(
      await invoke<WidgetSettings>("save_widget_settings", { settings: nextSettings }),
    );
  } catch (error) {
    settingsError = error instanceof Error ? error.message : String(error);
  }

  renderAfterSettingsChange();
}

function bindInteractions(): void {
  appRoot.querySelector(".close-button")?.addEventListener("click", () => {
    void currentWindow.hide();
  });

  appRoot.querySelector(".settings-button")?.addEventListener("click", () => {
    void invoke("show_settings_window");
  });

  appRoot.querySelectorAll<HTMLButtonElement>("[data-display-mode]").forEach((button) => {
    button.addEventListener("click", () => {
      const displayMode = button.dataset.displayMode === "full" ? "full" : "simple";
      void saveSettings({ displayMode });
    });
  });

  appRoot.querySelectorAll<HTMLButtonElement>("[data-background]").forEach((button) => {
    button.addEventListener("click", () => {
      const background = button.dataset.background === "light" ? "light" : "dark";
      void saveSettings({ background });
    });
  });

  appRoot.querySelectorAll<HTMLButtonElement>("[data-keychain]").forEach((button) => {
    button.addEventListener("click", () => {
      const keychainAccess: KeychainAccess = button.dataset.keychain === "allow" ? "allow" : "off";
      void saveSettings({ keychainAccess });
    });
  });

  appRoot.querySelectorAll(".refresh-button").forEach((button) => {
    button.addEventListener("click", () => {
      void refreshUsage(true);
    });
  });

  appRoot.querySelector(".settings-refresh-button")?.addEventListener("click", () => {
    void refreshCollectedFromSettings();
  });

  appRoot.querySelector(".close-settings-button")?.addEventListener("click", () => {
    void currentWindow.hide();
  });
}

function renderAfterSettingsChange(): void {
  if (isSettingsWindow) {
    renderSettingsWindow();
  } else {
    renderCurrent();
  }
}

async function refreshCollectedFromSettings(): Promise<void> {
  try {
    const collected = await invoke<UsageSnapshot>("refresh_collected_usage_snapshot");
    settingsError = collected.error;
  } catch (error) {
    settingsError = error instanceof Error ? error.message : String(error);
  }
  renderSettingsWindow();
}

async function refreshUsage(manual = false): Promise<void> {
  if (refreshInProgress) {
    return;
  }

  refreshInProgress = true;
  renderCurrent();

  try {
    try {
      const collected = await invoke<UsageSnapshot>("refresh_collected_usage_snapshot");
      collectionError = collected.error;
      if (manual && collected.error) {
        settingsError = collected.error;
      } else if (manual) {
        settingsError = null;
      }
    } catch (error) {
      collectionError = error instanceof Error ? error.message : String(error);
      if (manual) {
        settingsError = collectionError;
      }
    }

    latestSnapshot = await invoke<UsageSnapshot>("get_usage_snapshot");
  } catch (error) {
    latestSnapshot = errorSnapshot(error);
  } finally {
    refreshInProgress = false;
    renderCurrent();
  }
}

async function initialize(): Promise<void> {
  try {
    currentSettings = normalizeSettings(await invoke<WidgetSettings>("get_widget_settings"));
  } catch {
    currentSettings = DEFAULT_SETTINGS;
  }

  await listen<WidgetSettings>("settings-changed", (event) => {
    currentSettings = normalizeSettings(event.payload);
    renderAfterSettingsChange();
  });

  if (isSettingsWindow) {
    renderSettingsWindow();
    return;
  }

  renderLoading();
  await refreshUsage();
  window.setInterval(() => {
    void refreshUsage();
  }, REFRESH_MS);
}

// Tauri's data-tauri-drag-region is the most portable frameless-drag path.
// GNOME on Wayland may still apply compositor-specific limits to drag,
// positioning, global shortcuts, and always-on-top behavior.
void initialize();
