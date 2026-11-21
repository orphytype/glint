Interface cleanup, liquid glass updates, and playback and GPU fixes.

New:

- Local anonymized diagnostic logs (capped at 1 MB) and a one-time hardware summary in Settings -> About, with a toggle, an Open folder button and a Clear button.
- Clicking the update button in the title bar now opens a compact update menu with the Glint icon, new version number, a release notes preview and an install button.
- Liked tracks in Library now support sorting by original order, title, artist, album and duration.
- The queue panel now has an icon button to clear Up next and a hover remove button on upcoming tracks.
- Albums and playlists now show track numbers beside each row.
- The sign-in screen now has a Paste button in the token field.
- The settings modal now shows a vertical scrollbar indicator when a tab scrolls.
- Minimum window size is now 840x560 instead of 960x640.

Fixed:

- Redesigned the My Wave tuning menu with a cleaner minimalist layout, standard UI font and no repeated subtitle under the heading.
- Fixed the update indicator overlapping the Windows minimize button; it is now a circular dashed-ring download icon sitting cleanly to the left of window controls without a red dot.
- Changed the Other shapes button on Home into a circular liquid glass button matching the rest of the app chrome.
- Darkened the center of the main My Wave shape shader so white text stays legible over bright covers, and made the Mood line underneath smaller and cleaner.
- Replaced the bouncing pre-lyrics countdown digits with a three-bead loader whose particles fall onto and bounce along the actual letter shapes of the first lyric line.
- Removed the duplicate Audio quality entry from the bottom mini-player More menu (it remains in the full Now Playing More menu).
- Reduced memory and D3D12 upload/layer usage by removing nested offscreen layers in Settings, packing decoded cover art cleanly into 256x256 atlas tiles (16 per page instead of 9), and trimming idle decoded cover caches on rest.
- Simplified album, artist, playlist, Liked and Now Playing headers by moving secondary actions (pin, download, dislike, audio quality and player look) into the More menu and aligning titles, transport controls and utility icons to clean vertical edges.
- Aligned track numbers and right-edge track durations with the Sort button above and Details link below; hovering a row now swaps the duration for the More button in the same slot.
- Restoring the window from minimized without moving the cursor or pressing a key stopped updating the playback timer and seek bar.
- Idle windows with effects turned off (or paused playback) kept redrawing on the GPU instead of resting at zero frames.
- Opening Now Playing shifted the colour of the glass Close button and source pill when the top and bottom veils faded in; lyrics and queue now fade their own edges and share the live Now Playing backdrop smoothly.
- The source pill at the top of Now Playing could overlap the Close button on narrower windows or with long titles.
- Redesigned liquid glass chips and bars with cleaner refraction and light-theme contrast, and shared blur pyramids across overlapping top and bottom glass surfaces.
- The Top result card in Search could clip the top of the cover art when fewer than four tracks matched.
- Clicking the currently playing track in a list or in the queue restarted it from 0:00 instead of toggling play and pause.
- The inline volume slider in the bottom player bar never appeared because the bar's maximum width stayed below its threshold. Wide windows now show the inline slider, and narrow windows support mouse-wheel volume and click-to-mute on the speaker button.
- The vertical volume popup overlapped the top border of the bottom player bar.
- Search showed an empty blank area while results or a category tab were loading; it now shows placeholder skeletons.
- Switching to Playlists, Albums or Artists in Library kept the full Liked playlist header pinned at the top; those tabs now show a compact Collection heading.
- The profile menu opened over the Back and Forward buttons and the page title; it now opens below the avatar and links directly to Account settings.
- Step 1 on the sign-in screen repeated the button label right below it.

Which file to download:

- Windows: `Glint-0.9.1-windows-x64-setup.exe` (installs for the current user, no administrator rights needed) or `Glint-0.9.1-windows-x64-portable.zip` (data stays next to the program while a `glint.portable` file sits next to `glint.exe`).
- macOS 11 and newer: `Glint-0.9.1-macos.dmg`. Open the image and drag Glint to Applications. The app is not signed with an Apple certificate, so on first launch right-click Glint, choose Open and confirm. If there is no Open button, allow it in System Settings -> Privacy & Security.
- Linux: `.AppImage` (run `chmod +x`, then the file), `.deb`, `.rpm` or `.tar.gz` with `install.sh`. You need a Vulkan driver and glibc 2.31 or newer.

The Windows installer and the AppImage update from inside the app: Settings -> Updates.

Signing in uses your Yandex account's token. The app never gets your password. Glint is not affiliated with Yandex. How to build it from source is in the README.
