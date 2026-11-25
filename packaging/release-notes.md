Glint adds progressive audio streaming, lossless FLAC playback and downloads, personalized My Waves, broader synced lyrics, Liked genre and mood filters, and a non-repeating My Wave shader.

New:

- Tracks stream progressively over HTTP as they play instead of waiting for the whole file to download. Audio buffers live on disk with HTTP Range seeking and bounded decode-ahead so long lossless tracks use minimal memory.
- Home loads personalized My Waves from Yandex Music (`/wheel/new` and `/rotor/session`) alongside "Shake My Wave", using Yandex's own wave artwork, bold sans-serif labels, a slim seek bar along the top edge of the control pill, and a quality picker in the Home `"..."` menu.
- The Home My Wave backdrop uses a continuous block-indexed procedural shader (`wave.wgsl`) that never loops, renders sharply at screen resolution, and fades into a darker `#050505` canvas toward the sidebar and wave wheel.
- Butterflies return on the new Home screen with zero-allocation simulation and early shader culling, plus a toggle under Settings -> General -> Graphics.
- Synced lyrics now check lrc.red, Spicy Lyrics, Yandex Music (`LRC` and `TEXT`), Musixmatch (`richsync`, `LRC`, and plain text), and LRCLIB. Upcoming lines dim progressively with distance, while past lines stay hidden until you hover over the lyrics panel.
- Liked tracks include horizontal genre and mood filter pills ("Everything", "Energetic", top genres from your library, "Fun", "Sad", "Calm") and Yandex-style header controls (yellow Play button, Pin, More menu, and Download/Saved button).
- Hovering over an album or playlist card reveals Pin, Play/Pause, More, and Like buttons directly on the cover.

Fixed:

- Fixed the HMAC signing key and codec list (`flac-mp4`, `aac-mp4`) for `/get-file-info` and Yandex lyrics so lossless FLAC tracks stream and report their real codec and bitrate instead of falling back to MP3 320 kbps. Downloaded `flac-mp4` tracks remux cleanly into tagged `.flac` files.
- Dropdown menus and volume cards now blur the backdrop from the first frame of opening instead of flashing dark until the animation settles.
- Pre-warming Now Playing and Queue pipelines removes first-open stutter and keeps the blurred backdrop visible throughout the opening cover flight.
- Added soft edge fades to the pinned items strip in the sidebar and removed the redundant "Playlist" prefix from the Liked track count.
- Updated RUNE to the latest commit (`0d2cad2`).

Which file to download:

- Windows: `Glint-0.11.0-windows-x64-setup.exe` (installs for the current user, no administrator rights needed) or `Glint-0.11.0-windows-x64-portable.zip` (data stays next to the program while a `glint.portable` file sits next to `glint.exe`).
- macOS 11 and newer: `Glint-0.11.0-macos.dmg`. Open the image and drag Glint to Applications. The app is not signed with an Apple certificate, so on first launch right-click Glint, choose Open and confirm. If there is no Open button, allow it in System Settings -> Privacy & Security.
- Linux: `.AppImage` (run `chmod +x`, then the file), `.deb`, `.rpm` or `.tar.gz` with `install.sh`. You need a Vulkan driver and glibc 2.31 or newer.

The Windows installer and the AppImage update from inside the app: Settings -> About.

Signing in uses your Yandex account's token. The app never gets your password. Glint is not affiliated with Yandex. How to build it from source is in the README.
