# Contributing

Glint talks to Yandex Music's private API and to a few outside services (lrc.red, FkCensorData).
Before you change anything that touches them, read [`NOTICE.md`](NOTICE.md).

## Set up

1. Install [rustup](https://rustup.rs). It picks the Rust version from `rust-toolchain.toml`.
2. On Linux, install the system packages:

   ```bash
   sudo apt install build-essential pkg-config libasound2-dev libfontconfig1-dev libwayland-dev
   ```

3. Clone the repository and check that it builds:

   ```bash
   git clone https://github.com/orphytype/glint.git
   cd glint
   cargo build --workspace
   ```

## Run

```bash
cargo run --release
```

To keep your own settings and token out of it, give the development copy its own data folder:

```bash
GLINT_DATA_DIR=/tmp/glint-dev cargo run --release
```

### Environment variables

| Variable | What it does |
|---|---|
| `GLINT_DATA_DIR=<folder>` | Keeps settings, token and cache in this folder |
| `GLINT_LANG=ru` or `en` | Interface language instead of the system one |
| `GLINT_STARTUP=1` | Prints how long each launch step took; `=exit` quits after the first frame |
| `GLINT_AUTOPLAY=1` | Plays Liked right away; `=np` also opens the player |
| `GLINT_DEBUG=1` | Prints desktop integration diagnostics to stderr |
| `GLINT_FXOFF=glass,veil,…` | Turns single effects off to measure their cost; `fps` shows GPU time |

## Scenes and screenshots

The `tooling` feature (on by default) adds RUNE's headless tools. They draw the app's screens
without a window, from a made-up catalogue in `crates/glint/src/fixtures.rs`. The catalogue has no
audio, and the release build does not include it.

```bash
cargo run --release -- --list-scenes
cargo run --release -- --snap out.png --scene nowplaying --size 1320x820
```

If you change the interface, attach a before and after picture of the scene to your pull request.

## Where things are

| Path | What is in it |
|---|---|
| `crates/glint/src/app.rs` | App state and what changes it |
| `crates/glint/src/ui/` | Screens and components: `pages.rs`, `np.rs` (the player), `settings.rs`, `kit.rs` (shared parts) |
| `crates/glint/src/keys.rs` | Keyboard shortcuts and how they are rebound |
| `crates/glint/src/theme.rs` | Colours, fonts, animation springs |
| `crates/glint/src/fx.rs`, `fx/`, `shaders/` | GPU effects: glass, blur, covers |
| `crates/glint/src/i18n.rs` | Every interface string, in Russian and English |
| `crates/glint/src/fixtures.rs`, `scenes.rs` | The made-up catalogue and the scenes for screenshots |
| `crates/ym-api` | API client |
| `crates/ym-audio` | Audio engine |
| `crates/glint-update` | Updates |

## Code

- Format with `cargo fmt`. The line length is 180 (`rustfmt.toml`): the interface is built from
  long chains of calls and they read better that way.
- Add each interface string to `i18n.rs` in both languages. A missing translation does not compile,
  on purpose.
- Take colours and sizes from `theme.rs` and `ui/kit.rs` instead of writing numbers in place. A new
  shared element goes in `kit.rs`.
- A comment says why the code is the way it is, not what it does.
- Write text (interface strings, the README, release notes) the way one person explains something to
  another. The rules are in [`AGENTS.md`](AGENTS.md).

## Checks

There are no tests, and you should not add any (see `AGENTS.md`). Before a pull request,
`cargo clippy --release --workspace --all-targets` must pass with no warnings. Look at an interface
change in the app or in a scene (`--scene`, `--snap`).

## Pull requests

- One pull request, one change.
- Say what changed and how to check it. For the interface, add pictures.
- If a change touches outside services, licences or what the release build does, update `README.md`,
  `NOTICE.md` or `packaging/release-notes.md`.
- Leave the version and release tags alone in an ordinary pull request.

## What not to send

- OAuth tokens, `token.bin` or `settings.json` files, and logs that may contain them. This goes for
  issues and CI too.
- Build output, and anything from `dist/` or `target/`.
- Audio or covers from Yandex Music.

## Reporting a problem

Open an [issue](https://github.com/orphytype/glint/issues). Give the Glint version (it is in
Settings → About), your system, and the steps that lead to the problem. For a security hole, write
to the repository owner directly instead of opening a public issue.
