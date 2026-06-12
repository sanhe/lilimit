import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import "./styles.css";

type UsageStatus = "ready" | "missing" | "invalidJson" | "ioError";
type UsageSource = "lilimitCollected";
type DisplayMode = "simple" | "full";
type WidgetBackground = "dark" | "light";
type KeychainAccess = "off" | "allow";
type ToolbarDisplay = "text" | "bars";
type FullTab = "overview" | "codex" | "claude";

type WindowPosition = {
  x: number;
  y: number;
};

type WidgetSettings = {
  displayMode: DisplayMode;
  background: WidgetBackground;
  keychainAccess: KeychainAccess;
  toolbarDisplay: ToolbarDisplay;
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
  topModel: string | null;
};

type DailyUsagePoint = {
  dayKey: string;
  totalTokens: number | null;
  costUSD: number | null;
};

type ProviderUsage = {
  name: string;
  accountEmail: string | null;
  planText: string | null;
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
  stale: boolean;
  error: string | null;
};

type UsageSnapshot = {
  status: UsageStatus;
  path: string;
  source: UsageSource | null;
  updatedAt: string | null;
  providers: ProviderUsage[];
  error: string | null;
};

const REFRESH_MS = 5 * 60 * 1000;
const STALE_MS = 15 * 60 * 1000;
const DEFAULT_SETTINGS: WidgetSettings = {
  displayMode: "simple",
  background: "dark",
  keychainAccess: "off",
  toolbarDisplay: "bars",
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
let currentFullTab: FullTab = "overview";

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

function formatUsagePercent(value: number | null): string {
  return value === null ? "n/a" : `${value}% left`;
}

function formatDuration(ms: number): string {
  const totalMinutes = Math.max(1, Math.ceil(ms / 60_000));
  const days = Math.floor(totalMinutes / (24 * 60));
  const hours = Math.floor((totalMinutes / 60) % 24);
  const minutes = totalMinutes % 60;

  if (days > 0) {
    return hours > 0 ? `${days}d ${hours}h` : `${days}d`;
  }
  if (hours > 0) {
    return minutes > 0 ? `${hours}h ${minutes}m` : `${hours}h`;
  }
  return `${totalMinutes}m`;
}

function rateWindowForRow(provider: ProviderUsage, row: UsageRow): RateWindowDetail | null {
  switch (row.id) {
    case "session":
    case "primary":
      return provider.primary;
    case "weekly":
    case "secondary":
      return provider.secondary;
    case "tertiary":
      return provider.tertiary;
    default:
      return null;
  }
}

function expectedUsedPercent(window: RateWindowDetail | null): number | null {
  if (!window?.windowMinutes || !window.resetsAt) {
    return null;
  }

  const resetMs = Date.parse(window.resetsAt);
  if (!Number.isFinite(resetMs)) {
    return null;
  }

  const totalMs = window.windowMinutes * 60_000;
  if (!Number.isFinite(totalMs) || totalMs <= 0) {
    return null;
  }

  const remainingMs = resetMs - Date.now();
  const elapsedMs = Math.max(0, Math.min(totalMs, totalMs - remainingMs));
  return Math.max(0, Math.min(100, (elapsedMs / totalMs) * 100));
}

function renderMeterMarkers(window: RateWindowDetail | null): string {
  if (!window) {
    return "";
  }

  const thresholdMarkers = [50, 25]
    .map(
      (position) =>
        `<span class="meter-marker threshold-marker" style="left: ${position}%"></span>`,
    )
    .join("");
  const expectedUsed = expectedUsedPercent(window);
  const paceMarker =
    expectedUsed === null
      ? ""
      : `<span class="meter-marker pace-marker" style="left: ${100 - expectedUsed}%"></span>`;

  return thresholdMarkers + paceMarker;
}

function rowPaceDetails(
  window: RateWindowDetail | null,
  percentLeft: number | null,
): { pace: string | null; projection: string | null } {
  const expectedUsed = expectedUsedPercent(window);
  const actualUsed =
    window?.usedPercent !== null && window?.usedPercent !== undefined
      ? window.usedPercent
      : percentLeft === null
        ? null
        : 100 - percentLeft;

  if (expectedUsed === null || actualUsed === null || !window?.windowMinutes || !window.resetsAt) {
    return { pace: null, projection: null };
  }

  const delta = Math.round(expectedUsed - actualUsed);
  const pace =
    Math.abs(delta) < 2
      ? "On pace"
      : delta > 0
        ? `${delta}% in reserve`
        : `${Math.abs(delta)}% over pace`;

  const resetMs = Date.parse(window.resetsAt);
  const totalMs = window.windowMinutes * 60_000;
  const remainingMs = resetMs - Date.now();
  const elapsedMs = Math.max(0, Math.min(totalMs, totalMs - remainingMs));
  let projection = "Lasts until reset";

  if (actualUsed > 0 && elapsedMs > 0) {
    const msUntilEmpty = (elapsedMs / actualUsed) * (100 - actualUsed);
    if (Number.isFinite(msUntilEmpty) && msUntilEmpty < remainingMs) {
      projection = `Runs out in ${formatDuration(msUntilEmpty)}`;
    }
  }

  return { pace, projection };
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
  const providerErrors =
    latestSnapshot?.status === "ready"
      ? latestSnapshot.providers
          .filter((provider) => provider.error)
          .map((provider) => `${provider.name}: ${provider.error}`)
      : [];
  const warning = collectionError || providerErrors.join(" / ");

  if (!warning) {
    return "";
  }

  const hasCodex = warning.includes("Codex:");
  const hasClaude = warning.includes("Claude:");
  const label =
    hasClaude && !hasCodex
      ? providerErrors.length > 0 && !collectionError
        ? "Claude stale"
        : "Claude unavailable"
      : "Partial data";

  return `<span class="collection-warning" title="${escapeHtml(warning)}">${escapeHtml(label)}</span>`;
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
    toolbarDisplay: settings?.toolbarDisplay === "text" ? "text" : "bars",
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
  const toolbarDisplay = currentSettings.toolbarDisplay;

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
      <div class="setting-group">
        <span>Toolbar</span>
        <div class="segmented">
          <button type="button" data-toolbar-display="bars" class="${toolbarDisplay === "bars" ? "active" : ""}">Bars</button>
          <button type="button" data-toolbar-display="text" class="${toolbarDisplay === "text" ? "active" : ""}">Text</button>
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

function renderDisplayModeIcon(): string {
  if (currentSettings.displayMode === "simple") {
    return `
      <svg viewBox="0 0 24 24" aria-hidden="true" focusable="false">
        <path d="M8 3H3v5"></path>
        <path d="M3 3l7 7"></path>
        <path d="M16 21h5v-5"></path>
        <path d="M21 21l-7-7"></path>
      </svg>
    `;
  }

  return `
    <svg viewBox="0 0 24 24" aria-hidden="true" focusable="false">
      <path d="M10 3v7H3"></path>
      <path d="M3 10l7-7"></path>
      <path d="M14 21v-7h7"></path>
      <path d="M21 14l-7 7"></path>
    </svg>
  `;
}

function renderTitlebar(stale = false): string {
  const nextMode = currentSettings.displayMode === "simple" ? "full" : "simple";
  const toggleLabel =
    currentSettings.displayMode === "simple" ? "Expand to full view" : "Collapse to mini view";

  return `
    <header class="titlebar" data-tauri-drag-region>
      <h1 data-tauri-drag-region>lilimit</h1>
      <div class="title-actions">
        ${stale ? '<span class="stale">stale</span>' : ""}
        <button class="icon-button refresh-button${refreshInProgress ? " refreshing" : ""}" type="button" aria-label="Refresh usage data" title="Refresh" ${refreshInProgress ? "disabled" : ""}>${renderRefreshIcon()}</button>
        <button
          class="icon-button display-mode-button"
          type="button"
          data-next-display-mode="${nextMode}"
          aria-label="${toggleLabel}"
          title="${toggleLabel}"
        >${renderDisplayModeIcon()}</button>
        <button class="icon-button settings-button" type="button" aria-label="Widget settings" title="Settings">...</button>
        <button class="icon-button close-button" type="button" aria-label="Close lilimit" title="Close">x</button>
      </div>
    </header>
  `;
}

function renderSimpleProvider(provider: ProviderUsage): string {
  const session = clampPercent(provider.sessionLeftPercent);
  const weekly = clampPercent(provider.weeklyLeftPercent);
  const sessionTone = session === null ? "unknown" : providerTone(session);
  const weeklyTone = weekly === null ? "unknown" : providerTone(weekly);
  const name = escapeHtml(provider.name);
  const resetText = escapeHtml(provider.resetText || "reset n/a");
  const sessionWidth = session ?? 0;
  const weeklyWidth = weekly ?? 0;
  const providerIsStale = provider.stale || isStale(provider.updatedAt);
  const stateTitle = provider.error ? ` title="${escapeHtml(provider.error)}"` : "";
  const stateBadge = providerIsStale
    ? `<span class="provider-state"${stateTitle}>stale</span>`
    : "";

  return `
    <section class="provider${providerIsStale ? " stale-provider" : ""}" aria-label="${name} usage"${stateTitle}>
      <div class="provider-line">
        <strong>${name}</strong>
        <span class="numbers">
          <span>${formatPercent(session)} session</span>
          <span>${formatPercent(weekly)} week</span>
          <span>${resetText}</span>
          ${stateBadge}
        </span>
      </div>
      <div class="simple-meters">
        <div class="meter" aria-label="${name} session left">
          <div class="meter-fill ${sessionTone}" style="width: ${sessionWidth}%"></div>
        </div>
        <div class="meter week-meter" aria-label="${name} weekly left">
          <div class="meter-fill ${weeklyTone}" style="width: ${weeklyWidth}%"></div>
        </div>
      </div>
    </section>
  `;
}

function renderFullUsageRow(provider: ProviderUsage, row: UsageRow, color: string): string {
  const percent = clampPercent(row.percentLeft);
  const width = percent ?? 0;
  const title = escapeHtml(row.title);
  const reset = row.resetText ? escapeHtml(row.resetText) : "";
  const window = rateWindowForRow(provider, row);
  const details = rowPaceDetails(window, percent);
  const secondaryRows =
    percent !== null || reset || details.pace || details.projection
      ? `
        <div class="row-details">
          <span>${formatUsagePercent(percent)}</span>
          ${reset ? `<span>Resets ${reset}</span>` : "<span></span>"}
          ${details.pace ? `<span>${escapeHtml(details.pace)}</span>` : "<span></span>"}
          ${details.projection ? `<span>${escapeHtml(details.projection)}</span>` : "<span></span>"}
        </div>
      `
      : "";

  return `
    <div class="full-usage-row">
      <div class="full-row-heading">
        <strong>${title}</strong>
      </div>
      <div class="meter full-meter">
        <div class="meter-fill" style="width: ${width}%; background: ${color}"></div>
        ${renderMeterMarkers(window)}
      </div>
      ${secondaryRows}
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

function localDayKey(date = new Date()): string {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

function latestDailyUsage(points: DailyUsagePoint[]): DailyUsagePoint | null {
  return points.length > 0 ? points[points.length - 1] : null;
}

function renderTokenMetrics(provider: ProviderUsage): string {
  const tokenUsage = provider.tokenUsage;
  if (!tokenUsage) {
    return "";
  }

  const latest = latestDailyUsage(provider.dailyUsage);
  const latestCost = latest?.costUSD ?? tokenUsage.sessionCostUSD;
  const latestTokens = latest?.totalTokens ?? tokenUsage.sessionTokens;
  const latestCostTitle = latest?.dayKey === localDayKey() ? "Today" : "Latest cost";

  return `
    <div class="metrics">
      ${renderMetric(latestCostTitle, formatUsd(latestCost))}
      ${renderMetric("30d cost", formatUsd(tokenUsage.last30DaysCostUSD))}
      ${renderMetric("30d tokens", formatTokens(tokenUsage.last30DaysTokens))}
      ${renderMetric("Latest tokens", formatTokens(latestTokens))}
    </div>
  `;
}

function renderUsageNotes(provider: ProviderUsage): string {
  if (!provider.tokenUsage) {
    return "";
  }
  const tokenUsage = provider.tokenUsage;
  const providerName = provider.name.toLowerCase();
  const estimateText =
    providerName === "claude"
      ? "Estimated from local Claude logs at API rates; token totals include cache"
      : providerName === "codex"
        ? "Estimated from local Codex logs at API rates; token totals include cache"
        : "Estimated from local logs at API rates; token totals include cache";

  return `
    <div class="usage-notes">
      ${tokenUsage.topModel ? `<p>Top model: ${escapeHtml(tokenUsage.topModel)}</p>` : ""}
      <p>${escapeHtml(estimateText)}</p>
    </div>
  `;
}

function renderProviderMeta(provider: ProviderUsage): string {
  const rows = [
    provider.accountEmail,
    provider.planText ??
      (provider.creditsRemaining !== null
        ? `${formatTokens(provider.creditsRemaining)} credits`
        : null),
  ].filter((value): value is string => Boolean(value));

  if (rows.length === 0) {
    return "";
  }

  return `
    <div class="provider-meta">
      ${rows.map((row) => `<span>${escapeHtml(row)}</span>`).join("")}
    </div>
  `;
}

function renderHistory(points: DailyUsagePoint[], color: string): string {
  const visiblePoints = points.slice(-24);
  if (visiblePoints.length === 0) {
    return "";
  }

  // Chart a single metric: mixing dollars and token counts in one scale
  // makes the cost bars invisible next to million-token days.
  const hasCost = visiblePoints.some((point) => point.costUSD !== null);
  const values = visiblePoints.map((point) =>
    hasCost ? (point.costUSD ?? 0) : (point.totalTokens ?? 0),
  );
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

function renderExtraUsageSection(row: UsageRow, color: string): string {
  const percentLeft = clampPercent(row.percentLeft);
  const usedPercent = percentLeft === null ? null : 100 - percentLeft;
  const width = usedPercent === null ? 0 : usedPercent;
  // The backend already reports amounts in major currency units.
  const resetText = escapeHtml(row.resetText ?? "");
  const usedText = usedPercent === null ? "" : `${usedPercent}% used`;

  return `
    <div class="usage-section extra-usage-section">
      <h3>${escapeHtml(row.title)}</h3>
      <div class="meter full-meter">
        <div class="meter-fill" style="width: ${width}%; background: ${color}"></div>
      </div>
      <div class="section-details">
        <span>${resetText}</span>
        <span>${escapeHtml(usedText)}</span>
      </div>
    </div>
  `;
}

function formatCostAndTokens(cost: number | null, tokens: number | null): string {
  const parts = [
    cost === null ? null : formatUsd(cost),
    tokens === null ? null : `${formatTokens(tokens)} tokens`,
  ].filter((value): value is string => Boolean(value));

  return parts.length > 0 ? parts.join(" &middot; ") : "-";
}

function renderClaudeCostSummary(provider: ProviderUsage): string {
  if (provider.name.toLowerCase() !== "claude" || !provider.tokenUsage) {
    return "";
  }

  const tokenUsage = provider.tokenUsage;
  const latest = latestDailyUsage(provider.dailyUsage);
  const latestCost = latest?.costUSD ?? tokenUsage.sessionCostUSD;
  const latestTokens = latest?.totalTokens ?? tokenUsage.sessionTokens;
  const latestLabel = latest?.dayKey === localDayKey() ? "Today" : "Latest";

  return `
    <div class="usage-section cost-summary">
      <div>
        <h3>Cost</h3>
        <p>${latestLabel}: ${formatCostAndTokens(latestCost, latestTokens)}</p>
        <p>Last 30 days: ${formatCostAndTokens(tokenUsage.last30DaysCostUSD, tokenUsage.last30DaysTokens)}</p>
      </div>
      <span class="cost-chevron" aria-hidden="true">&rsaquo;</span>
    </div>
  `;
}

function renderFullProvider(provider: ProviderUsage): string {
  const color = providerColor(provider.name);
  const rows = provider.usageRows.length > 0 ? provider.usageRows : fallbackRows(provider);
  const standardRows = rows.filter((row) => row.id !== "extra-usage");
  const extraUsageRows = rows.filter((row) => row.id === "extra-usage");
  const name = escapeHtml(provider.name);
  const updatedAt = provider.updatedAt ?? latestSnapshot?.updatedAt ?? null;
  const providerIsStale = provider.stale || isStale(updatedAt);
  const stateTitle = provider.error ? ` title="${escapeHtml(provider.error)}"` : "";
  const stateBadge = providerIsStale
    ? `<span class="provider-state"${stateTitle}>stale</span>`
    : "";
  const usageRows = [
    ...standardRows.map((row) => renderFullUsageRow(provider, row, color)),
    provider.codeReviewRemainingPercent !== null
      ? renderFullUsageRow(
          provider,
          {
            id: "codeReview",
            title: "Code review",
            percentLeft: provider.codeReviewRemainingPercent,
            resetText: null,
          },
          color,
        )
      : "",
  ].join("");

  return `
    <section class="full-provider${providerIsStale ? " stale-provider" : ""}" style="--provider-color: ${color}"${stateTitle}>
      <div class="full-provider-header">
        <div>
          <h2>${name}</h2>
          <span>Updated ${formatAge(updatedAt)} ${stateBadge}</span>
        </div>
        ${renderProviderMeta(provider)}
      </div>
      <div class="full-rows">
        ${usageRows || '<p class="empty">No usage rows</p>'}
      </div>
      ${renderTokenMetrics(provider)}
      ${renderHistory(provider.dailyUsage, color)}
      ${renderUsageNotes(provider)}
      ${extraUsageRows.map((row) => renderExtraUsageSection(row, color)).join("")}
      ${renderClaudeCostSummary(provider)}
    </section>
  `;
}

function providerForFullTab(provider: ProviderUsage): FullTab | null {
  switch (provider.name.toLowerCase()) {
    case "codex":
      return "codex";
    case "claude":
      return "claude";
    default:
      return null;
  }
}

function fullTabLabel(tab: FullTab): string {
  switch (tab) {
    case "overview":
      return "Overview";
    case "codex":
      return "Codex";
    case "claude":
      return "Claude";
  }
}

function renderFullTabIcon(tab: FullTab): string {
  switch (tab) {
    case "overview":
      return `
        <svg viewBox="0 0 24 24" aria-hidden="true" focusable="false">
          <rect x="4" y="4" width="6" height="6" rx="1.2"></rect>
          <rect x="14" y="4" width="6" height="6" rx="1.2"></rect>
          <rect x="4" y="14" width="6" height="6" rx="1.2"></rect>
          <rect x="14" y="14" width="6" height="6" rx="1.2"></rect>
        </svg>
      `;
    case "codex":
      return `
        <svg viewBox="0 0 24 24" aria-hidden="true" focusable="false">
          <path d="M12 3.5a8.5 8.5 0 0 1 7.4 12.7"></path>
          <path d="M18 7.2a8.5 8.5 0 0 1-7.2 13.2"></path>
          <path d="M12 20.5A8.5 8.5 0 0 1 4.6 7.8"></path>
          <path d="M6 16.8A8.5 8.5 0 0 1 13.2 3.6"></path>
        </svg>
      `;
    case "claude":
      return `
        <svg viewBox="0 0 24 24" aria-hidden="true" focusable="false">
          <path d="M12 3v18"></path>
          <path d="M3 12h18"></path>
          <path d="M5.6 5.6l12.8 12.8"></path>
          <path d="M18.4 5.6 5.6 18.4"></path>
          <path d="M8.5 3.8 15.5 20.2"></path>
          <path d="M15.5 3.8 8.5 20.2"></path>
        </svg>
      `;
  }
}

function renderFullTabs(providers: ProviderUsage[]): string {
  const availableTabs = new Set<FullTab>(["overview"]);
  providers.forEach((provider) => {
    const tab = providerForFullTab(provider);
    if (tab) {
      availableTabs.add(tab);
    }
  });

  return `
    <nav class="full-tabs" aria-label="Full view provider tabs">
      ${(["overview", "codex", "claude"] as FullTab[])
        .map((tab) => {
          const active = currentFullTab === tab;
          const disabled = !availableTabs.has(tab);
          return `
            <button
              type="button"
              class="${active ? "active" : ""}"
              data-full-tab="${tab}"
              aria-selected="${active}"
              ${disabled ? "disabled" : ""}
            >
              ${renderFullTabIcon(tab)}
              <span>${fullTabLabel(tab)}</span>
            </button>
          `;
        })
        .join("")}
    </nav>
  `;
}

function fullTabProviders(providers: ProviderUsage[]): ProviderUsage[] {
  if (currentFullTab === "overview") {
    return providers;
  }

  return providers.filter((provider) => providerForFullTab(provider) === currentFullTab);
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
    const visibleProviders = fullTabProviders(providers);
    return `
      <main class="surface widget full-view ${currentSettings.background}-bg" data-tauri-drag-region>
        ${renderTitlebar(stale)}
        ${renderFullTabs(providers)}
        <div class="full-content">
          ${
            visibleProviders.length > 0
              ? visibleProviders.map((provider) => renderFullProvider(provider)).join("")
              : `<p class="empty">No ${escapeHtml(fullTabLabel(currentFullTab))} data</p>`
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
      await invoke<WidgetSettings>("save_widget_settings", {
        // The backend owns the window position; echoing a cached value back
        // could move the window to where it sat before the user dragged it.
        settings: { ...nextSettings, windowPosition: null },
      }),
    );
  } catch (error) {
    settingsError = error instanceof Error ? error.message : String(error);
  }

  renderAfterSettingsChange();
}

function bindInteractions(): void {
  appRoot.querySelector(".close-button")?.addEventListener("click", () => {
    void invoke("hide_widget_window");
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

  appRoot.querySelectorAll<HTMLButtonElement>("[data-toolbar-display]").forEach((button) => {
    button.addEventListener("click", () => {
      const toolbarDisplay: ToolbarDisplay =
        button.dataset.toolbarDisplay === "text" ? "text" : "bars";
      void saveSettings({ toolbarDisplay });
    });
  });

  appRoot.querySelectorAll(".refresh-button").forEach((button) => {
    button.addEventListener("click", () => {
      void refreshUsage(true);
    });
  });

  appRoot.querySelectorAll<HTMLButtonElement>(".display-mode-button").forEach((button) => {
    button.addEventListener("click", () => {
      const displayMode: DisplayMode =
        button.dataset.nextDisplayMode === "full" ? "full" : "simple";
      void saveSettings({ displayMode });
    });
  });

  appRoot.querySelectorAll<HTMLButtonElement>("[data-full-tab]").forEach((button) => {
    button.addEventListener("click", () => {
      const tab = button.dataset.fullTab;
      if (tab === "overview" || tab === "codex" || tab === "claude") {
        currentFullTab = tab;
        renderCurrent();
      }
    });
  });

  appRoot.querySelector(".settings-refresh-button")?.addEventListener("click", () => {
    void refreshCollectedFromSettings();
  });

  appRoot.querySelector(".close-settings-button")?.addEventListener("click", () => {
    hideSettingsWindow();
  });
}

function hideSettingsWindow(): void {
  void currentWindow.hide();
}

function bindSettingsWindowShortcuts(): void {
  window.addEventListener("keydown", (event) => {
    if (event.key !== "Escape") {
      return;
    }

    event.preventDefault();
    hideSettingsWindow();
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
    const collected = await invoke<UsageSnapshot>("refresh_collected_usage_snapshot", {
      force: true,
    });
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
      const collected = await invoke<UsageSnapshot>("refresh_collected_usage_snapshot", {
        force: manual,
      });
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

  try {
    await listen<WidgetSettings>("settings-changed", (event) => {
      currentSettings = normalizeSettings(event.payload);
      renderAfterSettingsChange();
    });
  } catch (error) {
    settingsError = error instanceof Error ? error.message : String(error);
  }

  if (isSettingsWindow) {
    bindSettingsWindowShortcuts();
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
