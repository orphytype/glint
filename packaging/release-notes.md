A floating mini player that stays on top of other apps, plus playlist search, track reordering and local audio files in playlists.

New:

- A mini player in its own small window, on top of other apps. Open it from the player bar, the tray menu or Settings -> On the desktop.
- Opening the mini player hides the main window. The mini player's expand button brings it back.
- The mini player opens where you last left it.
- The mini player has three layouts (Bar, Card and Cover) and an opacity slider. It can also open by itself when you close the main window to the tray.
- Playlists have a search field above the track list and an Upload track button for local audio files (MP3, FLAC, M4A, WAV and OGG). You can also drop audio files onto a playlist.
- Tracks in playlists and in the queue show a grip on hover. Drag it to change the order.
- Downloaded tracks keep their green mark on hover, with a trash button beside it to delete the download.
- A track added to a playlist goes to the top of the list instead of the end.
- The download dialog shows each quality as a card with its format, bitrate and approximate size. The track count and total length sit in a badge at the top right.
- Opening and closing the now playing screen counts as a step for the Back and Forward buttons.

Fixed:

- A hidden main window gives its graphics memory back, so the app takes less memory in the tray and with the mini player open.
- The mini player appeared in a different place every time it opened.
- Unpinning a page showed "Открепить" in the notification instead of "Откреплено". Clearing the download history showed "Очистить историю загрузок" instead of "История загрузок очищена".
- Most settings lost their notes under the label. The ones left are shorter and only say what the label doesn't.
- Hovered cards no longer jump in their scrolling title: the scroll starts from the beginning.
- Unmuting in the player bar, the now playing screen, the mini player or the tray brings back the volume you had set instead of 80%.
- On macOS the profile picture in the left rail sits below the window buttons instead of under them.
- The Back and Forward buttons are smaller, with a thin border, and sit closer to the left rail.
- The artist page no longer repeats the artist's bio. It is still in the About the artist window.
- The header gradient on album, playlist and artist pages no longer fades in.

Which file to download:

- Windows: `Glint-0.9.3-windows-x64-setup.exe` (installs for the current user, no administrator rights needed) or `Glint-0.9.3-windows-x64-portable.zip` (data stays next to the program while a `glint.portable` file sits next to `glint.exe`).
- macOS 11 and newer: `Glint-0.9.3-macos.dmg`. Open the image and drag Glint to Applications. The app is not signed with an Apple certificate, so on first launch right-click Glint, choose Open and confirm. If there is no Open button, allow it in System Settings -> Privacy & Security.
- Linux: `.AppImage` (run `chmod +x`, then the file), `.deb`, `.rpm` or `.tar.gz` with `install.sh`. You need a Vulkan driver and glibc 2.31 or newer.

The Windows installer and the AppImage update from inside the app: Settings -> Updates.

Signing in uses your Yandex account's token. The app never gets your password. Glint is not affiliated with Yandex. How to build it from source is in the README.
