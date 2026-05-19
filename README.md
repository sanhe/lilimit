# lilimit

lilimit is a tiny Tauri 2 desktop widget for Codex and Claude usage.

By default, lilimit refreshes its own local snapshot: local CLI credentials are read from disk, usage APIs are called, and the result is written to lilimit's config directory.

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

## Widget settings

lilimit stores display preferences next to its collected data:

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
- Keychain: `Off` or `Allow`

`Simple` keeps the small 280x140 monitor view. `Full` expands the widget to show detailed lilimit provider data, including usage rows, reset text, token/cost totals, credits when present, and recent daily usage bars. The last reported window position is saved automatically and restored on the next launch.

Keychain access is off by default. On macOS, `Allow` lets lilimit try a short, explicit read of Claude Code's `Claude Code-credentials` Keychain item if `~/.claude/.credentials.json` is missing. This is not Chrome cookie access. Ubuntu builds ignore macOS Keychain reads.

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
- Simple mode is 280x140. Full mode is 360x560 so detailed usage rows have room to breathe.
- macOS shows a lilimit status item in the menu bar. When usage data is available, it shows compact session consumption like `Codex 29%  Claude 58%`; otherwise it falls back to the lilimit icon. Use the status item to show, hide, or quit the widget.
- Press `Cmd+Shift+L` on macOS or `Ctrl+Shift+L` on Linux to show or hide the widget.
- lilimit refreshes `collected_snapshot.json` every 30 seconds from local CLI OAuth credentials.
- Codex usage is read from the OAuth tokens in `~/.codex/auth.json` and fetched from the ChatGPT usage endpoint. API-key-only Codex auth cannot read these usage stats.
- Claude usage is read from `~/.claude/.credentials.json` and fetched from Anthropic's OAuth usage endpoint. On macOS, optional Keychain access can read Claude Code's credential item when explicitly enabled.
- Browser cookie import and Chrome Keychain cookie decryption are intentionally not implemented; lilimit uses local CLI OAuth credentials instead.
- Display preferences and the last reported window position are stored in lilimit's local `settings.json`.
- Always-on-top behavior is compositor-dependent on Ubuntu. GNOME on X11 usually honors it more consistently than GNOME on Wayland.
- Global shortcut behavior is compositor-dependent on Ubuntu. GNOME Wayland may restrict app-managed global shortcuts.
- Restoring a saved position is also compositor-dependent on Ubuntu Wayland; macOS and GNOME X11 are generally more predictable.
- Tray/status icons and tray text on Ubuntu GNOME are desktop-extension/session dependent. GNOME Wayland may require AppIndicator-style support for the icon to appear, and may not show the compact usage title even when lilimit updates it.
- The window uses an opaque compact background because transparent frameless windows are less reliable across Linux desktop environments.
- The widget refreshes data every 30 seconds and marks data as stale when `updatedAt` is older than 15 minutes.
- Missing collector output shows `No usage data yet` with the expected platform path. Invalid JSON is reported inside the widget.
- Hermes integration is intentionally not implemented yet.
