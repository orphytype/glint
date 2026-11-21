<div align="center">

<img src="docs/glint.png" width="112" alt="Glint">

# Glint

An unofficial Yandex Music desktop client for Windows, macOS and Linux, written in Rust.

<img src="docs/screenshots/player.jpg" width="860" alt="The player with lyrics">

</div>

## What it does

- Plays tracks, albums, playlists, charts and My Wave stations with mood and seed tuning.
- Displays line-synced and word-synced lyrics from Yandex Music, LRCLIB and optional lrc.red.
- Downloads tracks for offline playback with embedded ID3 or FLAC metadata and cover art.
- Includes a 10-band equaliser, dark and light themes, custom player backdrops and rebindable shortcuts.
- Integrates with system media controls (Windows SMTC, Linux MPRIS), the system tray and optional FckCensor clean-track replacement.

## Download

Get a build from the [releases page](https://github.com/orphytype/glint/releases).

- Windows 10 and 11: `setup.exe` (per-user install, no admin rights) or portable `.zip`.
- macOS 11 and newer: `.dmg`. The app is not signed, so right-click it on first launch and choose Open.
- Linux x86_64: AppImage, `.deb`, `.rpm` or `.tar.gz`. Requires a Vulkan driver and glibc 2.31 or newer.

## Sign in

Glint never asks for your Yandex password. It authenticates with an OAuth token from your browser.

1. Click "Open Yandex sign-in" and sign in on Yandex.
2. Copy the URL you land on, or just the `access_token` value.
3. Paste it into Glint and click "Sign in".

On Windows, the token is encrypted with DPAPI in your user data folder. On macOS and Linux, it is written to an owner-only file (`0600`).

## Build from source

Install [rustup](https://rustup.rs); it picks the exact toolchain from `rust-toolchain.toml`.

- Windows: install Visual Studio Build Tools with the "Desktop development with C++" workload.
- macOS: install Xcode Command Line Tools (`xcode-select --install`).
- Debian and Ubuntu:
  ```bash
  sudo apt install build-essential pkg-config libasound2-dev libfontconfig1-dev libwayland-dev libxkbcommon-dev
  ```

Build a release binary:

```bash
git clone https://github.com/orphytype/glint.git
cd glint
cargo build --release --no-default-features
```

The binary lands at `target/release/glint` (`target\release\glint.exe` on Windows). If you want to work on the code, read [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Licensed under [GPL-3.0-only](LICENSE). Third-party notices and bundled font licences are in [NOTICE.md](NOTICE.md). Glint is not affiliated with Yandex.
