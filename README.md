# lilimit

lilimit is a tiny Tauri 2 desktop widget for Codex and Claude usage.

By default, lilimit now refreshes its own local snapshot using the same narrow provider path CodexBar uses for Codex and Claude OAuth usage: local CLI credentials are read from disk, usage APIs are called, and the result is written to lilimit's config directory. If that snapshot is missing, auto mode can still read CodexBar's exported widget snapshot, then fall back to lilimit sample data.

It does not integrate Hermes, does not import browser cookies, and does not scrape Chrome cookies or Chrome's Keychain storage.

## Data file

lilimit collected snapshot on macOS:

```sh
~/Library/Application\ Support/lilimit/collected_snapshot.json
```

lilimit collected snapshot on Ubuntu Linux:

```sh
~/.config/lilimit/collected_snapshot.json
```

CodexBar fallback on macOS:

```sh
~/Library/Group\ Containers/Y5PE65HELJ.com.steipete.codexbar/widget-snapshot.json
```

Development/debug CodexBar builds may write:

```sh
~/Library/Group\ Containers/Y5PE65HELJ.com.steipete.codexbar.debug/widget-snapshot.json
```

lilimit standalone fallback on macOS:

```sh
~/Library/Application\ Support/lilimit/usage_snapshot.json
```

lilimit standalone fallback on Ubuntu Linux:

```sh
~/.config/lilimit/usage_snapshot.json
```

## Widget settings

lilimit stores display preferences next to its local fallback data:

macOS:

```sh
~/Library/Application\ Support/lilimit/settings.json
```

Ubuntu Linux:

```sh
~/.config/lilimit/settings.json
```

The in-widget settings button lets you choose:

- Display mode: `Simple` or `Full`
- Background: `Dark` or `Light`
- Data source: `Auto`, `lilimit`, or `CodexBar`
- Keychain: `Off` or `Allow`

`Simple` keeps the small 280x140 monitor view. `Full` expands the widget to show the richer CodexBar snapshot details lilimit can read locally, including usage rows, reset text, token/cost totals, credits when present, and recent daily usage bars. The last reported window position is saved automatically and restored on the next launch.

`Auto` refreshes lilimit's own collected snapshot first, then falls back to CodexBar and sample data. `lilimit` uses only lilimit's collected/sample files. `CodexBar` uses only CodexBar's exported `widget-snapshot.json`.

Keychain access is off by default. On macOS, `Allow` lets lilimit try a short, explicit read of Claude Code's `Claude Code-credentials` Keychain item if `~/.claude/.credentials.json` is missing. This is not Chrome cookie access. Ubuntu builds ignore macOS Keychain reads.

Standalone fallback JSON:

```json
{
  "updatedAt": "2026-05-18T20:44:00Z",
  "providers": [
    {
      "name": "Codex",
      "sessionLeftPercent": 74,
      "weeklyLeftPercent": 61,
      "resetText": "2h 11m"
    },
    {
      "name": "Claude",
      "sessionLeftPercent": 42,
      "weeklyLeftPercent": 80,
      "resetText": "4h 03m"
    }
  ]
}
```

## Create sample data

These commands are only needed when you want to test lilimit without the collector or CodexBar.

Cross-platform shortcut:

```sh
make sample-data
```

macOS:

```sh
mkdir -p "$HOME/Library/Application Support/lilimit"
UPDATED_AT="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
cat > "$HOME/Library/Application Support/lilimit/usage_snapshot.json" <<JSON
{
  "updatedAt": "$UPDATED_AT",
  "providers": [
    { "name": "Codex", "sessionLeftPercent": 74, "weeklyLeftPercent": 61, "resetText": "2h 11m" },
    { "name": "Claude", "sessionLeftPercent": 42, "weeklyLeftPercent": 80, "resetText": "4h 03m" }
  ]
}
JSON
```

Ubuntu Linux:

```sh
mkdir -p "$HOME/.config/lilimit"
UPDATED_AT="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
cat > "$HOME/.config/lilimit/usage_snapshot.json" <<JSON
{
  "updatedAt": "$UPDATED_AT",
  "providers": [
    { "name": "Codex", "sessionLeftPercent": 74, "weeklyLeftPercent": 61, "resetText": "2h 11m" },
    { "name": "Claude", "sessionLeftPercent": 42, "weeklyLeftPercent": 80, "resetText": "4h 03m" }
  ]
}
JSON
```

## Run on macOS

Install dependencies once:

```sh
pnpm install
```

Run the widget:

```sh
make dev
```

Build the frontend:

```sh
pnpm build
```

Build the Tauri app:

```sh
pnpm tauri:build
```

Makefile shortcuts are also available:

```sh
make install
make dev
make check
make build
make tauri-build
```

## Run on Ubuntu

Install the Tauri Linux prerequisites for your Ubuntu release, then run:

```sh
pnpm install
pnpm tauri:dev
```

Build:

```sh
pnpm tauri:build
```

The same Makefile shortcuts work on Ubuntu:

```sh
make install
make dev
make check
make build
make tauri-build
```

## Notes

- The window is frameless, non-resizable, and always on top where the OS/window manager supports it.
- Simple mode is 280x140. Full mode is 360x560 so the CodexBar-style details have room to breathe.
- macOS shows a lilimit status item in the menu bar. Use it to show, hide, or quit the widget.
- Press `Cmd+Shift+L` on macOS or `Ctrl+Shift+L` on Linux to show or hide the widget.
- In `Auto` or `lilimit` data mode, lilimit refreshes `collected_snapshot.json` every 30 seconds from local CLI OAuth credentials.
- Codex usage is read from `~/.codex/auth.json` and fetched from the ChatGPT usage endpoint.
- Claude usage is read from `~/.claude/.credentials.json` and fetched from Anthropic's OAuth usage endpoint. On macOS, optional Keychain access can read Claude Code's credential item when explicitly enabled.
- lilimit can still read CodexBar's `widget-snapshot.json`, but CodexBar is no longer required for the basic OAuth usage path.
- Browser cookie import and Chrome Keychain cookie decryption are intentionally not implemented. CodexBar has a larger browser-cookie subsystem; lilimit keeps this local widget narrower.
- Display preferences and the last reported window position are stored in lilimit's local `settings.json`.
- Always-on-top behavior is compositor-dependent on Ubuntu. GNOME on X11 usually honors it more consistently than GNOME on Wayland.
- Global shortcut behavior is compositor-dependent on Ubuntu. GNOME Wayland may restrict app-managed global shortcuts.
- Restoring a saved position is also compositor-dependent on Ubuntu Wayland; macOS and GNOME X11 are generally more predictable.
- Tray/status icons on Ubuntu GNOME are desktop-extension/session dependent. GNOME Wayland may require AppIndicator-style support for the icon to appear reliably.
- The window uses an opaque compact background because transparent frameless windows are less reliable across Linux desktop environments.
- The widget refreshes data every 30 seconds and marks data as stale when `updatedAt` is older than 15 minutes.
- Missing files show `No usage data yet` with the expected platform path. Invalid JSON is reported inside the widget.
- Hermes integration is intentionally not implemented yet.
