# Contributing

Glint talks to Yandex Music's private API and to outside services (LRCLIB, lrc.red, FkCensorData). Before changing code that touches those services, read [NOTICE.md](NOTICE.md).

## Set up

1. Install [rustup](https://rustup.rs). It picks the compiler version and components (`clippy`, `rustfmt`) from `rust-toolchain.toml`.
2. Install platform build tools:
   - Windows: Visual Studio Build Tools (Desktop development with C++).
   - macOS: Xcode Command Line Tools (`xcode-select --install`).
   - Debian and Ubuntu:
     ```bash
     sudo apt install build-essential pkg-config libasound2-dev libfontconfig1-dev libwayland-dev libxkbcommon-dev
     ```
3. Clone the repository and build the workspace:
   ```bash
   git clone https://github.com/orphytype/glint.git
   cd glint
   cargo build --workspace
   ```

## Run locally

```bash
cargo run --release
```

To keep your real account, token and settings separate while developing, point `GLINT_DATA_DIR` at a temporary folder.

On Linux and macOS:

```bash
GLINT_DATA_DIR=/tmp/glint-dev cargo run --release
```

On Windows PowerShell:

```powershell
$env:GLINT_DATA_DIR = "$env:TEMP\glint-dev"
cargo run --release
```

### Environment variables

| Variable | Effect |
|---|---|
| `GLINT_DATA_DIR=<dir>` | Stores token, settings, cache and downloads in `<dir>` and skips the single-instance lock |
| `GLINT_LANG=ru` or `en` | Overrides the system interface language |
| `GLINT_STARTUP=1` | Prints startup phase timings on the first frame; `=exit` quits after printing |
| `GLINT_AUTOPLAY=1` | Starts playing Liked on launch; `=np` also opens the player screen |
| `GLINT_DEBUG=1` | Prints desktop integration diagnostics (SMTC, MPRIS, tray) to stderr |
| `GLINT_FXOFF=<list>` | Disables individual GPU passes (`glass,veil,snap,field,morph,bars,shapes,stage`) or logs GPU frame time (`fps`) |
| `GLINT_PIPE_TIMES=1` | Prints shader pipeline compilation times to stderr |
| `GLINT_DIAG_SCENE=<name>` | Opens a real window populated with a fixture scene (requires `--features diag`) |

## Scenes and screenshots

The default `tooling` feature enables RUNE's headless snapshot runner. It renders any screen off-screen using fictional data from `crates/glint/src/fixtures.rs`. Release builds turn this feature off (`--no-default-features`).

```bash
cargo run --release -- --list-scenes
cargo run --release -- --scene nowplaying --frames 40 --snap out.png
```

When you change anything on screen, inspect the relevant scene snapshots and attach before/after images to your pull request.

## Repository layout

| Path | Responsibility |
|---|---|
| `crates/glint/src/main.rs` | Window setup, event loop, keyboard dispatch and frame pacing |
| `crates/glint/src/app/` | Application state, navigation, session boot, search, cover loading and overlays |
| `crates/glint/src/player/` | Playback queue, shuffle/repeat, gapless preload, My Wave and offline downloads |
| `crates/glint/src/store/` | Token storage, settings, snapshots and download index on disk |
| `crates/glint/src/net.rs` | Async runtime and backend bridge between UI state and `ym-api` or fixtures |
| `crates/glint/src/art.rs` | Cover art cache (300x300 single-size cache, decode throttling, PNG encoding) |
| `crates/glint/src/fx/`, `shaders/` | Custom wgpu compositor passes, blur pyramids, audio-reactive visuals and WGSL shaders |
| `crates/glint/src/desktop/` | Windows SMTC, Linux MPRIS, tray integration, window placement and resize throttling |
| `crates/glint/src/ui/pages/` | Main screens: Home, mood garden, Search, Library, Album, Artist and Playlist |
| `crates/glint/src/ui/np/` | Now Playing overlay: cover stage, synced lyrics, countdown animation and queue |
| `crates/glint/src/ui/details/` | Details modal, metadata sections and artist social link matching |
| `crates/glint/src/ui/menu/` | Context menus, popovers, stage look panel and confirmation dialogs |
| `crates/glint/src/ui/settings/` | Settings tabs: account, look, playback, lyrics, shortcuts, desktop, EQ, storage, updates, about |
| `crates/glint/src/ui/kit/` | Shared widgets, typography helpers, buttons, covers, track rows and marquee |
| `crates/glint/src/ui/dock.rs`, `shell.rs` | Left navigation rail, bottom player bar and window chrome |
| `crates/glint/src/i18n.rs`, `theme.rs`, `keys.rs` | Russian/English strings, colour/typography tokens and keyboard shortcuts |
| `crates/glint/src/fixtures.rs`, `scenes.rs` | Fictional catalog and named states for headless scene snapshots |
| `crates/ym-api/` | UI-free async client for Yandex Music, LRCLIB, lrc.red and FkCensorData |
| `crates/ym-audio/` | Audio engine: gapless decoding, 10-band equaliser, FFT analyser and disk cache |
| `crates/glint-update/` | GitHub release checker, SHA-256 verification and platform installer |

## Code conventions

- Format with `cargo fmt --all`. Line width is 180 (`rustfmt.toml`) so UI builder chains stay compact.
- Follow the workspace lints configured in root `Cargo.toml`: no `.unwrap()` in production code, no wildcard imports, no `dbg!`, and a `// SAFETY:` comment above every `unsafe` block.
- Put every user-facing string in `crates/glint/src/i18n.rs` in both English and Russian. Missing translations fail to compile.
- Reuse colours, typography and spacing from `theme.rs` and `ui/kit/` instead of hardcoding new values.
- Keep comments focused on why the code works a certain way, not what each line does.
- Keep files focused; split modules into a folder before a single file grows hard to navigate.
- Follow the plain-language writing rules in [AGENTS.md](AGENTS.md) for UI strings, docs and release notes.

## Checks before a pull request

This repository has no automated test suite on purpose (see [AGENTS.md](AGENTS.md)); do not add tests or test steps. Before opening a pull request, run:

```bash
cargo fmt --all -- --check
cargo clippy --release --workspace --all-targets -- -D warnings
cargo clippy --release -p glint --no-default-features -- -D warnings
cargo build --release
```

For interface changes, verify them in the running app or with `--scene <name> --frames 40 --snap out.png`.

## Pull requests

- Keep one pull request to one clear change.
- Describe what changed and how you verified it. Attach screenshots for visual changes.
- Update `README.md`, `NOTICE.md` or `packaging/release-notes.md` when a change affects users, third-party services or licences.
- Leave crate versions and release tags untouched in ordinary pull requests.

## Do not commit or share

- OAuth tokens, `token.bin`, `settings.json` or logs that might expose them.
- Build artifacts (`target/`, `dist/`).
- Audio files or cover art downloaded from Yandex Music.

## Reporting problems

Open an [issue](https://github.com/orphytype/glint/issues) with your Glint version (Settings -> About), operating system and steps to reproduce. Report security issues directly to the repository owner instead of opening a public issue.
