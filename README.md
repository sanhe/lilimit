# lilimit

lilimit is a tiny Tauri 2 desktop widget that reads local Codex and Claude usage data from a JSON file. It does not fetch real usage yet and does not contact Codex, Claude, Hermes, or any network service.

## Data file

macOS:

```sh
~/Library/Application\ Support/lilimit/usage_snapshot.json
```

Ubuntu Linux:

```sh
~/.config/lilimit/usage_snapshot.json
```

Expected JSON:

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
npm install
```

Run the widget:

```sh
npm run dev
```

Build the frontend:

```sh
npm run build
```

Build the Tauri app:

```sh
npm run tauri:build
```

## Run on Ubuntu

Install the Tauri Linux prerequisites for your Ubuntu release, then run:

```sh
npm install
npm run dev
```

Build:

```sh
npm run tauri:build
```

## Notes

- The window is configured as frameless, non-resizable, 280x140, and always on top.
- macOS shows a lilimit status item in the menu bar. Use it to show, hide, or quit the widget.
- Press `Cmd+Shift+L` on macOS or `Ctrl+Shift+L` on Linux to show or hide the widget.
- Always-on-top behavior is compositor-dependent on Ubuntu. GNOME on X11 usually honors it more consistently than GNOME on Wayland.
- Global shortcut behavior is compositor-dependent on Ubuntu. GNOME Wayland may restrict app-managed global shortcuts.
- Tray/status icons on Ubuntu GNOME are desktop-extension/session dependent. GNOME Wayland may require AppIndicator-style support for the icon to appear reliably.
- The window uses an opaque compact background because transparent frameless windows are less reliable across Linux desktop environments.
- The widget refreshes the local JSON file every 30 seconds and marks data as stale when `updatedAt` is older than 15 minutes.
- Missing files show `No usage data yet` with the expected platform path. Invalid JSON is reported inside the widget.
- Hermes integration is intentionally not implemented yet.
