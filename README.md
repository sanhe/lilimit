# lilimit

lilimit is a tiny Tauri 2 desktop widget for Codex and Claude usage.

By default, lilimit refreshes its own local snapshot: local CLI credentials are read from disk, usage APIs are called, and the result is written to lilimit's config directory.

It does not integrate Hermes, does not import browser cookies, and does not scrape Chrome cookies or Chrome's Keychain storage.

## Local data sources

For token and cost history, lilimit reads local data that already exists on disk:

- On macOS, it reuses CodexBar's cost caches (`~/Library/Caches/CodexBar/cost-usage`) when present.
- For Codex, it otherwise falls back to scanning Codex CLI session logs in `~/.codex/sessions`.

Codex credentials are read from `~/.codex/auth.json` (honoring `CODEX_HOME`). When the stored OAuth tokens are more than 8 days old or rejected, lilimit refreshes them against OpenAI's token endpoint and writes the result back to `~/.codex/auth.json` — the same file the Codex CLI maintains. The write is atomic (temp file + rename).

Claude credentials are read from `~/.claude/.credentials.json`, or from the `LILIMIT_CLAUDE_OAUTH_TOKEN` environment variable if set. lilimit never refreshes or rewrites Claude credentials; when they expire, run `claude` to refresh them.

The Accounts section in settings can start the official browser login flows for both providers. lilimit runs `codex login` for ChatGPT/Codex and `claude auth login --claudeai` for a Claude subscription, then immediately fetches account and usage information from the providers' usage APIs. Login URLs, codes, and tokens are never returned to the webview or written to lilimit logs.

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

- Accounts: inspect Codex and Claude sign-in state, sign in, or re-authenticate through the installed official CLI
- Display mode: `Simple` or `Full`
- Background: `Dark` or `Light`
- Keychain: `Off` or `Allow`
- Toolbar: `Bars` or `Text`
- Scale: `80%` to `200%` (stepper in 10% steps, or drag the widget's corner grip)

`Simple` keeps the small 280x140 Codex + Claude overview, showing each provider's first available usage row and an explicit placeholder when one has no data. `Full` adds a CodexBar-style overview with the first two provider-specific rows, plus separate Codex and Claude tabs for reset text, token/cost totals, credits when present, and recent daily usage bars. The last reported window position is saved automatically and restored on the next launch.

`Scale` grows or shrinks the whole widget — window, fonts, meters, and charts together — which is handy when the default size is too small on high-resolution Ubuntu displays. It resizes the window and applies a matching webview zoom, so the layout stays sharp. You can step the scale from settings, or drag the resize grip in the widget's bottom-right corner to size it directly — the window and its contents grow together. The effective scale is capped to the monitor work area so the widget never grows past the visible screen, and the settings window itself stays at native size. Webview zoom needs macOS 11+ (Linux is fine); on older macOS the window only resizes.

Keychain access is off by default. On macOS, `Allow` lets lilimit try a short, explicit read of Claude Code's `Claude Code-credentials` Keychain item if `~/.claude/.credentials.json` is missing. This is not Chrome cookie access. Ubuntu builds ignore macOS Keychain reads.

Claude retry/backoff state is stored locally as `collector_state.json` next to `settings.json` and `collected_snapshot.json`.

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

Run a local secret scan when `gitleaks` is installed:

```sh
make secret-scan
```

Makefile shortcuts are also available:

```sh
make install
make dev
make check
make build
make tauri-build
```

## Install on Ubuntu

Download the latest Ubuntu artifacts from the [GitHub release page](https://github.com/sanhe/lilimit/releases).

Prefer the `.deb` package on Ubuntu:

```sh
sudo apt install ./lilimit_*_amd64.deb
lilimit
```

If you use the AppImage fallback:

```sh
chmod +x lilimit_*.AppImage
./lilimit_*.AppImage
```

The application icon (in the GNOME dash, app grid, and dock) comes from the installed `.deb`, which registers `lilimit.desktop` and the hicolor icon set. It does not appear when running `pnpm tauri:dev` or a bare AppImage in place, since those do not install a desktop entry. If the icon is missing or stale right after installing the `.deb`, refresh the caches and re-login (or restart GNOME Shell):

```sh
sudo gtk-update-icon-cache -f /usr/share/icons/hicolor
sudo update-desktop-database
```

You can sign in from lilimit settings. The equivalent terminal commands are:

```sh
codex login
claude auth login --claudeai
```

Lilimit reads Codex credentials from `~/.codex/auth.json` and Claude credentials from `~/.claude/.credentials.json`. Its own Ubuntu data lives in `~/.config/lilimit`.

## Build on Ubuntu

Install Node.js, pnpm, and Rust first. Then install the Tauri Linux prerequisites:

```sh
sudo apt update
sudo apt install -y \
  libwebkit2gtk-4.1-dev \
  build-essential \
  curl \
  wget \
  file \
  libgtk-3-dev \
  libxdo-dev \
  libssl-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev \
  patchelf \
  pkg-config
```

Then run the app from source:

```sh
pnpm install
pnpm tauri:dev
```

Build Ubuntu packages:

```sh
pnpm tauri build --bundles deb,appimage
```

The generated artifacts are written under `src-tauri/target/release/bundle/deb` and `src-tauri/target/release/bundle/appimage`.

The `release-linux` GitHub Actions workflow builds these artifacts on Ubuntu 22.04, uploads them to the workflow run as `lilimit-ubuntu-packages`, and attaches them to a draft GitHub release. Run it manually from Actions, or push a tag like `lilimit-v0.1.1`, then publish the draft after checking the assets.

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
- macOS shows a lilimit status item in the menu bar. The toolbar display can be compact bars or text like `Codex 29%  Claude 58%`; otherwise it falls back to the lilimit icon. Use the status item to show, hide, or quit the widget.
- Press `Cmd+Shift+L` on macOS or `Ctrl+Shift+L` on Linux to show or hide the widget.
- lilimit refreshes `collected_snapshot.json` at most every 5 minutes from local CLI OAuth credentials.
- Codex usage is read from the OAuth tokens in `~/.codex/auth.json` and fetched from the ChatGPT usage endpoint. API-key-only Codex auth cannot read these usage stats.
- Claude usage is read from `~/.claude/.credentials.json` and fetched from Anthropic's OAuth usage endpoint. On macOS, optional Keychain access can read Claude Code's credential item when explicitly enabled. Claude Code owns these credentials, so lilimit does not refresh expired Claude OAuth tokens directly; use the settings sign-in action or run `claude auth login --claudeai` to re-authenticate. Claude refreshes are rate-limited to 5 minutes, and Anthropic `429` responses use exponential backoff while keeping the last successful Claude data visible as stale.
- Browser cookie import and Chrome Keychain cookie decryption are intentionally not implemented; lilimit uses local CLI OAuth credentials instead.
- Display preferences and the last reported window position are stored in lilimit's local `settings.json`.
- Always-on-top behavior is compositor-dependent on Ubuntu. GNOME on X11 usually honors it more consistently than GNOME on Wayland.
- Global shortcut behavior is compositor-dependent on Ubuntu. GNOME Wayland may restrict app-managed global shortcuts.
- Restoring a saved position is also compositor-dependent on Ubuntu Wayland; macOS and GNOME X11 are generally more predictable.
- Tray/status icons and tray text on Ubuntu GNOME are desktop-extension/session dependent. GNOME Wayland may require AppIndicator-style support for the icon to appear, and may not show the compact text title even when lilimit updates it. Toolbar bars are rendered as an icon and are usually more portable than tray text.
- The window uses an opaque compact background because transparent frameless windows are less reliable across Linux desktop environments.
- The widget refreshes data every 5 minutes and marks data as stale when `updatedAt` is older than 15 minutes.
- Missing collector output shows `No usage data yet` with the expected platform path. Invalid JSON is reported inside the widget.
- Hermes integration is intentionally not implemented yet.
