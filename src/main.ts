import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import "./styles.css";

type UsageStatus = "ready" | "missing" | "invalidJson" | "ioError";

type ProviderUsage = {
  name: string;
  sessionLeftPercent: number;
  weeklyLeftPercent: number;
  resetText: string;
};

type UsageSnapshot = {
  status: UsageStatus;
  path: string;
  updatedAt: string | null;
  providers: ProviderUsage[];
  error: string | null;
};

const REFRESH_MS = 30_000;
const STALE_MS = 15 * 60 * 1000;
const currentWindow = getCurrentWindow();

const app = document.querySelector<HTMLDivElement>("#app");

if (!app) {
  throw new Error("Missing #app root");
}

const appRoot = app;

function clampPercent(value: number): number {
  if (!Number.isFinite(value)) {
    return 0;
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

function providerTone(percent: number): string {
  if (percent <= 25) {
    return "low";
  }

  if (percent <= 50) {
    return "medium";
  }

  return "high";
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

function renderProvider(provider: ProviderUsage): string {
  const session = clampPercent(provider.sessionLeftPercent);
  const weekly = clampPercent(provider.weeklyLeftPercent);
  const tone = providerTone(session);
  const name = escapeHtml(provider.name);
  const resetText = escapeHtml(provider.resetText);

  return `
    <section class="provider" aria-label="${name} usage">
      <div class="provider-line">
        <strong>${name}</strong>
        <span class="numbers">
          <span>${session}% session</span>
          <span>${weekly}% week</span>
          <span>${resetText}</span>
        </span>
      </div>
      <div class="meter" aria-label="${name} session left">
        <div class="meter-fill ${tone}" style="width: ${session}%"></div>
      </div>
    </section>
  `;
}

function renderStatus(snapshot: UsageSnapshot): string {
  if (snapshot.status === "missing") {
    return `
      <main class="state" data-tauri-drag-region>
        <header class="titlebar" data-tauri-drag-region>
          <h1 data-tauri-drag-region>lilimit</h1>
          <button class="close-button" type="button" aria-label="Close lilimit" title="Close">×</button>
        </header>
        <p>No usage data yet</p>
        <code>${escapeHtml(snapshot.path)}</code>
      </main>
    `;
  }

  if (snapshot.status === "invalidJson") {
    return `
      <main class="state" data-tauri-drag-region>
        <header class="titlebar" data-tauri-drag-region>
          <h1 data-tauri-drag-region>lilimit</h1>
          <button class="close-button" type="button" aria-label="Close lilimit" title="Close">×</button>
        </header>
        <p>Invalid usage JSON</p>
        <code>${escapeHtml(snapshot.error ?? snapshot.path)}</code>
      </main>
    `;
  }

  if (snapshot.status === "ioError") {
    return `
      <main class="state" data-tauri-drag-region>
        <header class="titlebar" data-tauri-drag-region>
          <h1 data-tauri-drag-region>lilimit</h1>
          <button class="close-button" type="button" aria-label="Close lilimit" title="Close">×</button>
        </header>
        <p>Unable to read usage data</p>
        <code>${escapeHtml(snapshot.error ?? snapshot.path)}</code>
      </main>
    `;
  }

  const stale = isStale(snapshot.updatedAt);
  const providers = sortProviders(snapshot.providers);

  return `
    <main class="widget" data-tauri-drag-region>
      <header class="titlebar" data-tauri-drag-region>
        <h1 data-tauri-drag-region>lilimit</h1>
        <div class="title-actions">
          ${stale ? '<span class="stale">stale</span>' : ""}
          <button class="close-button" type="button" aria-label="Close lilimit" title="Close">×</button>
        </div>
      </header>
      <div class="providers">
        ${
          providers.length > 0
            ? providers.map(renderProvider).join("")
            : '<p class="empty">No provider data</p>'
        }
      </div>
      <footer data-tauri-drag-region>
        Updated ${formatUpdatedAt(snapshot.updatedAt)}
      </footer>
    </main>
  `;
}

function renderApp(snapshot: UsageSnapshot): void {
  appRoot.innerHTML = renderStatus(snapshot);
  appRoot.querySelector(".close-button")?.addEventListener("click", () => {
    void currentWindow.hide();
  });
}

function renderAppError(error: unknown): void {
  renderApp({
    status: "ioError",
    path: "",
    updatedAt: null,
    providers: [],
    error: error instanceof Error ? error.message : String(error),
  });
}

async function refreshUsage(): Promise<void> {
  try {
    const snapshot = await invoke<UsageSnapshot>("get_usage_snapshot");
    renderApp(snapshot);
  } catch (error) {
    renderAppError(error);
  }
}

// Tauri's data-tauri-drag-region is the most portable frameless-drag path.
// GNOME on Wayland may still apply compositor-specific limits to drag and
// always-on-top behavior, while macOS and X11 usually honor it predictably.
void refreshUsage();
window.setInterval(() => {
  void refreshUsage();
}, REFRESH_MS);
