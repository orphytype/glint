# Notice and credits

Glint is licensed under the **GNU General Public License, version 3 only** (see [LICENSE](LICENSE)).

## Disclaimer

Glint is an unofficial, community-made client. It is not affiliated with, authorised by, maintained by or endorsed by Yandex or any of its affiliates. "Yandex", "Yandex Music" and related names belong to their respective owners and are mentioned only to identify the service Glint connects to.

All music, covers and metadata loaded when you sign in belong to their rights holders and are delivered under your own account's terms. The artists, albums, tracks and covers in the snapshot fixtures (`crates/glint/src/fixtures.rs`) are fictional.

## Request signing

The signing salts and keys in `crates/ym-api/src/sign.rs` are public client constants shared across open-source Yandex Music clients. They are not private credentials.

## Third-party components

Licence files shipped with release builds live in the `licenses/` directory.

| Component | Purpose | Licence |
|---|---|---|
| [RUNE](https://github.com/anomalyco/rune) (`rune`, `rune-fx`, `rune-motion`, `rune-kit`) | GPU UI framework, text layout, animation and SVG icon rendering | MIT (`licenses/RUNE-MIT.txt`) |
| [Hugeicons](https://github.com/hugeicons/hugeicons) | Interface SVG icons | MIT (`licenses/HUGEICONS-MIT.txt`) |
| [Commissioner](https://fonts.google.com/specimen/Commissioner) | Primary UI sans-serif font | OFL 1.1 (`licenses/Commissioner-OFL-1.1.txt`) |
| [Cormorant](https://fonts.google.com/specimen/Cormorant) | Display serif font for titles and lyrics | OFL 1.1 (`licenses/Cormorant-OFL-1.1.txt`) |
| [IBM Plex Mono](https://github.com/IBM/plex) | Monospace font (bundled via RUNE) | OFL 1.1 (`licenses/IBMPlexMono-OFL-1.1.txt`) |
| [Inter](https://github.com/rsms/inter) | Fallback sans-serif glyphs (bundled via RUNE) | OFL 1.1 (`licenses/Inter-OFL-1.1.txt`) |
| [LRCLIB](https://lrclib.net) | Fallback line-synced lyrics (queried by artist, title and duration) | Public service (see site terms) |
| [lrc.red](https://lrc.red) | Optional word-synced lyrics (toggleable in Settings; sends track title, artist and Glint's GitHub URL as contact; credited on Now Playing) | Public service ([API terms](https://lrc.red/api)) |
| [FckCensor](https://github.com/Hazzz895/FckCensor) | Reference behaviour for clean-track replacement (no code bundled) | MIT |
| [FkCensorData](https://github.com/fcensor-data/FkCensorData) | Community map of clean audio replacements when FckCensor is enabled | Public repository terms |
| Rust crate dependencies (`wgpu`, `rodio`, `rustfft`, `reqwest`, `tokio`, `serde`, `image`, `windows`, `windows-sys`, `zbus`, `winresource` and transitive crates) | GPU rendering, audio playback, FFT, HTTP, async runtime, serialisation, image decoding and OS integration | Individual crate licences (see `Cargo.lock`) |

## GPL-3.0 notice

Glint is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, version 3 of the License.

Glint is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details. You should have received a copy of the GNU General Public License along with this program in [LICENSE](LICENSE). If not, see <https://www.gnu.org/licenses/>.
