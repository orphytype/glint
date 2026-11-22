Audio quality gets a pill in the mini player and a picker for batch downloads.

New:

- The mini player has a rounded pill with the selected audio quality and its bitrate. Narrow windows show a quality icon button instead.
- Now Playing has a quality button beside the volume slider. Both buttons open the same quality menu.
- The quality menu shows an icon and the bitrate of each tier, with a check mark on the chosen tier.
- The batch download dialog has a quality picker. The picked quality applies to that batch only and leaves playback quality unchanged.
- Audio quality has three tiers: Saver, Balanced and Best. Best plays FLAC when the track has it and the highest lossy stream otherwise.
- The Now Playing cover loads at 600x600 and keeps the cached smaller cover on screen until it is ready.
- Covers load at 100, 300 or 600 pixels depending on where they appear. Track lists, queues, avatars and pins use the small size.
- Player time labels show hours when a track runs for an hour or longer.
- The Up Next count in the queue turns into a Clear button on hover, and tracks that already played are dimmed.
- A slim scrollbar on the right of the page follows scrolling and can be dragged.

Fixed:

- Changing the quality in the menu now updates the pill in the mini player right away. It no longer keeps the bitrate of the stream that was playing before.
- Removed the quality badge under the seek bar in Now Playing. The quality button beside the volume slider takes its place.
- The first item in the settings sidebar now sits level with the header. The header title uses the same font size as the sidebar labels.
- Muting in Now Playing brings back the previous volume, and the mouse wheel adjusts volume there.
- The mini player shows Like on the left and Dislike on the right.
- The queue panel no longer starts under the title bar.
- Opening or closing Now Playing keeps the cover flight above the player curtain and uses one backdrop snapshot.
- Covers are trimmed from memory after page switches and player transitions, and every 30 seconds while the app runs.
- Turning FkCensor on refreshes the library and any open detail pages right away.
- Downloaded tracks and tracks with a FkCensor replacement stay playable when Yandex marks them unavailable.
- Audio downloads no longer stop after 120 seconds. The downloader accepts files up to 4 GB.
- My Wave cards are shorter, with rounded corners and a smaller play icon.
- Dialogs, menus, the settings card and toasts use 2px borders and no drop shadow.
- The update check and install buttons show a disabled state while busy.

Which file to download:

- Windows: `Glint-0.9.2-windows-x64-setup.exe` (installs for the current user, no administrator rights needed) or `Glint-0.9.2-windows-x64-portable.zip` (data stays next to the program while a `glint.portable` file sits next to `glint.exe`).
- macOS 11 and newer: `Glint-0.9.2-macos.dmg`. Open the image and drag Glint to Applications. The app is not signed with an Apple certificate, so on first launch right-click Glint, choose Open and confirm. If there is no Open button, allow it in System Settings -> Privacy & Security.
- Linux: `.AppImage` (run `chmod +x`, then the file), `.deb`, `.rpm` or `.tar.gz` with `install.sh`. You need a Vulkan driver and glibc 2.31 or newer.

The Windows installer and the AppImage update from inside the app: Settings -> Updates.

Signing in uses your Yandex account's token. The app never gets your password. Glint is not affiliated with Yandex. How to build it from source is in the README.
