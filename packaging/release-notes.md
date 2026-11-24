Glint switches to the new Yandex Music address for stream links, and back and forward work again after you close the player.

New:

- Album, playlist and artist pages show placeholder rows while their tracks load.
- Rows of albums and playlists that scroll sideways fade out at both edges.
- A FckCensor track's details show its artist, album and length, with a note that Yandex Music has no credits for it.

Fixed:

- Yandex Music moved stream links to a new address. Glint asks the new one first and falls back to the old one.
- After you went forward to the open player and closed it, back only reopened the player. Now closing the player steps back to the page under it, and forward opens the player again.
- A long source name above the cover in the player was pushed right by the close button and ran up to the cover's edge. Now it stays centered over the cover and shortens on both sides.
- Opening Glint from the tray opened the main window next to the mini player. Now it turns the mini player back into the window.
- The search field in Settings kept a blinking caret after Settings closed.
- Album and artist names under covers are smaller and lighter.

Which file to download:

- Windows: `Glint-0.9.5-windows-x64-setup.exe` (installs for the current user, no administrator rights needed) or `Glint-0.9.5-windows-x64-portable.zip` (data stays next to the program while a `glint.portable` file sits next to `glint.exe`).
- macOS 11 and newer: `Glint-0.9.5-macos.dmg`. Open the image and drag Glint to Applications. The app is not signed with an Apple certificate, so on first launch right-click Glint, choose Open and confirm. If there is no Open button, allow it in System Settings -> Privacy & Security.
- Linux: `.AppImage` (run `chmod +x`, then the file), `.deb`, `.rpm` or `.tar.gz` with `install.sh`. You need a Vulkan driver and glibc 2.31 or newer.

The Windows installer and the AppImage update from inside the app: Settings -> About.

Signing in uses your Yandex account's token. The app never gets your password. Glint is not affiliated with Yandex. How to build it from source is in the README.
