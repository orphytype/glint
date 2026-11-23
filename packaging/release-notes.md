Search that forgives typos, Phosphor icons everywhere, and a queue that slides to the next track.

New:

- Settings has a search field at the top of the sidebar. It opens the page with the setting and lights the row up for a moment.
- Search in the library, in playlists and in Settings finds titles typed with a typo, with letters left out, in transliteration or on the wrong keyboard layout.
- When the main search finds nothing, it tries the query on the other keyboard layout and says which query it shows results for.
- Buttons that belong together on album, playlist, artist and Liked pages are joined into one group with thin dividers. They are all the same height.
- The app uses Phosphor icons. The player keeps its own glyphs, and play and pause use them everywhere.

Fixed:

- The queue jumped to the new track. Now the tracks slide to their places and the headings fade while they pass.
- The blurred cover behind the open player switched at once. Now it fades into the new cover.
- The player bar hid like and dislike in a narrow window. They stay now.
- Albums no longer show a "Tracks" heading over their list. The sort button moved up next to the other buttons.
- The My Wave button has a darker shade, so its menu arrow is readable, and a smaller play glyph.
- Titles and headings use Commissioner. Cormorant is left for lyrics and wave names.

Which file to download:

- Windows: `Glint-0.9.4-windows-x64-setup.exe` (installs for the current user, no administrator rights needed) or `Glint-0.9.4-windows-x64-portable.zip` (data stays next to the program while a `glint.portable` file sits next to `glint.exe`).
- macOS 11 and newer: `Glint-0.9.4-macos.dmg`. Open the image and drag Glint to Applications. The app is not signed with an Apple certificate, so on first launch right-click Glint, choose Open and confirm. If there is no Open button, allow it in System Settings -> Privacy & Security.
- Linux: `.AppImage` (run `chmod +x`, then the file), `.deb`, `.rpm` or `.tar.gz` with `install.sh`. You need a Vulkan driver and glibc 2.31 or newer.

The Windows installer and the AppImage update from inside the app: Settings -> About.

Signing in uses your Yandex account's token. The app never gets your password. Glint is not affiliated with Yandex. How to build it from source is in the README.
