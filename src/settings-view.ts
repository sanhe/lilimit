function escapeHtml(value: string): string {
  const escapes: Record<string, string> = {
    "&": "&amp;",
    "<": "&lt;",
    ">": "&gt;",
    '"': "&quot;",
    "'": "&#39;",
  };

  return value.replace(/[&<>"']/g, (character) => escapes[character] ?? character);
}

export function renderSettingsHeader(appVersion: string): string {
  return `
    <header class="settings-header" data-tauri-drag-region>
      <div class="settings-title-copy" data-tauri-drag-region>
        <h1 data-tauri-drag-region>lilimit settings</h1>
        <span class="settings-version" data-tauri-drag-region>Version ${escapeHtml(appVersion)}</span>
      </div>
      <button
        class="icon-button settings-close-button close-settings-button"
        type="button"
        aria-label="Close settings"
        title="Close settings"
      >&times;</button>
    </header>
  `;
}
