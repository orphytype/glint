Cover art now loads once at 300x300, artist and album headers are cleaner, and social links show their network icons.

New:

- Artist links in the details modal now recognise TikTok, VK (`vk.com` and `vk.ru`), Telegram, YouTube, Instagram, X, Twitch, Spotify, SoundCloud, Bandcamp, Discord, Dzen, OK and other services, showing the network name and icon instead of raw paths.
- The button next to the close icon in the details modal now opens the track, album, playlist or artist in Yandex Music instead of copying its URL.

Fixed:

- Every cover size (list, card, header and player) fetched its own resolution and could stay blurred or reload on page switches. All covers now load and cache once at 300x300 without hover shadows or scale animations.
- Playlists, albums and singles had mismatched header covers and bottom-aligned metadata. Header covers now share one compact size with a border and tighter corners, and header text and buttons align vertically to the cover.
- Genres crowded artist and release headers, and release footers looked empty at the bottom of the page. Genres now live inside the details modal, and release footers show compact release details with a link to the modal.
- "Copy link" menu items used an external-link arrow while generic site links used a globe icon. "Copy link" now uses the copy icon and generic links use the external-link icon.
- In Now Playing, the gap between the player column and the lyrics or queue panel could shrink smaller than the left window margin. The gap now stays at least as wide as the left margin.
- Window scrolling and page-to-page cover transitions could drop toward 30 fps while running beside passive 30 Hz shader effects. UI springs and wheel scrolling now stay paced to the monitor's refresh rate.
- Resizing on Windows could lag behind the window edge or rebuild blur textures on every drag step. Glint now tracks the latest size and restores glass blur once the drag settles.
- Opening the download manager or switching settings could stall on disk I/O or hit race conditions on temporary files. Settings and playback saves now run sequentially on one background thread with unique temp files, and download lookups use an in-memory index.

Which file to download:

- Windows: `Glint-0.9.0-windows-x64-setup.exe` (installs for the current user, no administrator rights needed) or `Glint-0.9.0-windows-x64-portable.zip` (data stays next to the program while a `glint.portable` file sits next to `glint.exe`).
- macOS 11 and newer: `Glint-0.9.0-macos.dmg`. Open the image and drag Glint to Applications. The app is not signed with an Apple certificate, so on first launch right-click Glint, choose Open and confirm. If there is no Open button, allow it in System Settings -> Privacy & Security.
- Linux: `.AppImage` (run `chmod +x`, then the file), `.deb`, `.rpm` or `.tar.gz` with `install.sh`. You need a Vulkan driver and glibc 2.31 or newer.

The Windows installer and the AppImage update from inside the app: Settings -> Updates.

Signing in uses your Yandex account's token. The app never gets your password. Glint is not affiliated with Yandex. How to build it from source is in the README.
