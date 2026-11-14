# Notice and credits

Glint is licensed under the **GNU General Public License, version 3 only** (see `LICENSE`).

## Disclaimer

Glint is an unofficial, community-made client. It is not affiliated with, authorised, maintained,
sponsored or endorsed by Yandex or any of its affiliates. "Yandex", "Yandex Music" and related names and
marks are the property of their respective owners and are used here only to say what the software
works with. All music, covers and metadata you see when signed in belong to their rights holders and are
delivered by the service under your own account's terms. The Demo mode catalogue (artists, albums,
covers, lyrics and music) is fictional and was generated for this project.

## Request signing

The request-signing salts and keys in `crates/ym-api/src/sign.rs` are public constants that every
open-source client of the service ships; they are not credentials.

## Third-party components

| Component | Use | Licence |
|---|---|---|
| [RUNE](https://github.com/anomalyco/rune) by Anomaly (`rune`, `rune-fx`, `rune-motion`) | The GPU UI framework: window, layout, text, animation, the SVG icon renderer, and the shimmer layer behind the loading skeletons | MIT, `licenses/RUNE-MIT.txt` |
| [Hugeicons](https://github.com/hugeicons/hugeicons) | The SVG icon set used throughout the Glint interface | MIT, `licenses/HUGEICONS-MIT.txt` |
| [IBM Plex Mono](https://github.com/IBM/plex) by IBM Corp. | The typeface for small capitals, numbers and timestamps, bundled inside RUNE and therefore inside the built app | SIL Open Font License 1.1, `licenses/IBMPlexMono-OFL-1.1.txt` |
| [Inter](https://github.com/rsms/inter) by The Inter Project Authors | The fallback sans-serif, bundled inside RUNE, used where Bahnschrift is missing (on Linux, always) | SIL Open Font License 1.1, `licenses/Inter-OFL-1.1.txt` |
| Bahnschrift and Sitka Text | The headline and body face (Bahnschrift) and the italic for asides (Sitka Text). They are Windows system fonts: the app asks Windows for them by name and does not ship or copy them. Linux has neither, so Inter stands in | Microsoft, part of Windows |
| [LRCLIB](https://lrclib.net) | Fallback source of line-synced lyrics (queried by artist, title and duration only) | Public service, see its own terms |
| [lrc.red](https://lrc.red) | Optional source of lyrics timed word by word. It is enabled by default and can be switched off in Settings; when enabled, the title and artist of each track you play are sent to it, with Glint's name and GitHub link as the contact. Every lyric it supplies is credited on the Now Playing page and links back to its page there | Public service, see its [API terms](https://lrc.red/api) |
| [FckCensor](https://github.com/Hazzz895/FckCensor) | Reference implementation for clean-track replacement behavior | MIT; no FckCensor code is bundled |
| [FkCensorData](https://github.com/fcensor-data/FkCensorData) | Community-maintained map of alternative clean audio versions; Glint checks it when FckCensor is enabled | Public data source; see its repository terms |
| Rust crates (`wgpu`, `rodio`, `rustfft`, `reqwest`, `tokio`, `serde`, `image`, `windows`, `windows-sys`, `winresource`, and their dependencies) | Rendering, audio output and decoding, FFT, networking, async runtime, serialisation, image decoding, Windows DPAPI, the Windows media controls (SMTC), and (at build time) embedding the icon and version details in the exe | Each under its own licence; the full list is in `Cargo.lock` |

The licence texts that have to travel with the built application (RUNE's MIT licence, the OFL texts for
IBM Plex Mono and Inter) are kept in the `licenses/` folder.

## How to apply the GPL to this program

Glint is free software: you can redistribute it and/or modify it under the terms of the GNU General
Public License as published by the Free Software Foundation, version 3 of the License.

Glint is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the
implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public
License for more details. You should have received a copy of the GNU General Public License along with
this program (the file `LICENSE`). If not, see <https://www.gnu.org/licenses/>.
