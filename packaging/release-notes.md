Glint 0.12.0 redesigns Collection to match Yandex Music, switches the default interface font to Mustica Pro, optimizes Home's wave shader and large lists, fixes lossless FLAC streaming and download memory use, and refines Home, Explore, Now Playing, and the tray menu.

New:

- Mustica Pro is now bundled and set as the default interface font across the main window and mini player.
- Collection opens on a hub screen with a two-column preview of Liked tracks, a Favorite artists strip showing listening hours and rank changes, My playlists ("Created by you" and "You liked", plus a "+ New playlist" card), Favorite albums, Downloaded tracks, and a My dislikes screen with Tracks and Artists tabs.
- Own playlists now include a "Private playlist" switch at the bottom of the `"..."` menu to toggle public/private visibility.
- Album pages show the release type in small text above the title and keep the year, track count, and duration beside the artists on one line.
- Switching waves on Home or clicking the yellow close button on a wave pill reloads personalized My Waves with skeleton placeholders while rotating in fresh vibes and unseen artists from your collection.
- Synced lyrics scale smoothly with window size, rest slightly lower in the panel for a more balanced view, let you scroll freely with the mouse wheel, brighten nearby lines while browsing, and return to the active line after 4 seconds of inactivity. Past lines fade out gradually above the current line instead of disappearing abruptly.
- Player look and Settings list the three bundled lyric fonts (Commissioner, Mustica Pro, and Cormorant) with Mustica Pro as the default.
- The seek line uses a plain progress bar by default, with a "Waveform bars" toggle under Player look. Default Player look (and Reset) now uses large centered lyrics with background effects turned off.
- Track lists and the queue use taller rows (60 pt) with 46 pt covers, even padding around the cover, and slightly larger titles and artist names.
- Settings lets you adjust the number of butterflies on Home from 1 to 10 while butterflies are enabled.
- The system tray menu uses a minimal layout with a short truncated track title, Minimize/Show, Mini player, Check for updates, transport controls, and Close app.

Fixed:

- Fixed covers reloading across Collection when switching tracks by eliminating the mid-view cache eviction race, keeping active-page covers warm, and avoiding full downloaded-list clones in the Collection overview.
- Removed hover background highlights on Collection section headers in favor of sliding the chevron arrow right on hover.
- Added an invisible hover bridge between the volume button and floating volume popup so resting the pointer in the gap keeps the popup open without flickering.
- Fixed a UI freeze when opening Downloaded tracks and lag/cover flickering in large Dislikes and Downloaded lists by replacing cubic row indexing with O(1) cached slices and virtualized lazy cover loading.
- Cut Home wave shader GPU load roughly in half by precomputing all 13 B-spline motion lanes on CPU once per frame, clipping the transparent right 20% of the quad, and stepping the procedural wave at 30 Hz.
- Lossless `flac-mp4` tracks delivered over `encraw` now decrypt with AES-128-CTR during both streaming and downloads, reporting their true FLAC codec and bitrate instead of falling back to MP3.
- Batch downloads stream straight to disk (3 concurrent workers) and remux/tag files directly on disk instead of buffering whole tracks in RAM, preventing memory spikes during large downloads.
- Yandex editorial and personal playlists (`yamusic-*`) now show "Yandex Music" as the author and omit the redundant track/duration line.
- Compilations with synthetic "сборник" / "Various Artists" entries no longer display "сборник" as an artist name.
- Switching genre or mood filters in Liked tracks no longer scrolls down the list, and the stray back button beside the "Liked" title is removed.
- Fixed the last item ("Details") in long track context menus staying dimmed after opening.
- Switching sidebar tabs or opening an artist, album, or playlist resets scroll position to the top instead of reopening mid-page.
- Explore shelf headings use static vector icons aligned with the cards below instead of regenerating cached shapes, eliminating icon flicker on page open.
- Centered the Play/Pause icon inside album and playlist card hover buttons, indented track section headings like "Popular tracks" to align with row covers, and normalized raw `*genre` filter tags (such as `hyperpopgenre`).

Which file to download:

- Windows: `Glint-0.12.0-windows-x64-setup.exe` (installs for the current user, no administrator rights needed) or `Glint-0.12.0-windows-x64-portable.zip` (data stays next to the program while a `glint.portable` file sits next to `glint.exe`).
- macOS 11 and newer: `Glint-0.12.0-macos.dmg`. Open the image and drag Glint to Applications. The app is not signed with an Apple certificate, so on first launch right-click Glint, choose Open and confirm. If there is no Open button, allow it in System Settings -> Privacy & Security.
- Linux: `.AppImage` (run `chmod +x`, then the file), `.deb`, `.rpm` or `.tar.gz` with `install.sh`. You need a Vulkan driver and glibc 2.31 or newer.

The Windows installer and the AppImage update from inside the app: Settings -> About.

Signing in uses your Yandex account's token. The app never gets your password. Glint is not affiliated with Yandex. How to build it from source is in the README.
