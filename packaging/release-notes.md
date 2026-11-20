Glint now runs once: opening it again brings the window you already have forward.

New:

- Settings has a Shortcuts section. Click an action, press the key you want, and Backspace clears
  it. If another action already has that key, Glint says which one.
- Settings → Storage has "Delete downloaded content". It removes the downloaded tracks with their
  lyrics and the cover images in one step.
- The lyrics come in three sizes: small, medium and large. The animation and focus choices are gone.
- The player's reaction can be turned off. With it off, no rays are drawn and the flash does not
  follow the music. The "Wild" choice is gone: a look saved with it now reads "Lively".
- The settings list has no "Settings" title above it, and the title of the open section is smaller.
- The demo mode is gone. The sign-in screen has only the sign-in.

Fixed:

- Opening Glint a second time started another copy. It now brings the open window forward, also when
  the first one is hidden in the tray.
- When Glint started at login in the tray, the launch screen was over before you opened the window.
  It now plays when the window first appears.
- Big downloads stopped for stretches and then went on in batches. Eight tracks now download at
  once instead of three. The lyrics and the cover come while the file does, a lyrics service that
  does not answer holds a track for no more than 8 seconds, and tagging and saving a file no longer
  hold up the other downloads. A dropped connection is tried again twice before the track fails.
- The quality list opened from the More button of the mini player appeared in the wrong place. It
  now opens out of that button.
- A long track name made the first line of the tray menu, and the whole menu, very wide. The line
  is cut at 34 characters.

Which file to download:

- Windows: `Glint-0.8.0-windows-x64-setup.exe` (installs for the current user, no administrator
  rights needed) or `Glint-0.8.0-windows-x64-portable.zip` (data stays next to the program while a
  `glint.portable` file sits next to `glint.exe`).
- macOS 11 and newer: `Glint-0.8.0-macos.dmg`. Open the image and drag Glint to Applications. The
  app is not signed with an Apple certificate, so on first launch right-click Glint, choose Open and
  confirm. If there is no Open button, allow it in System Settings → Privacy & Security.
- Linux: `.AppImage` (run `chmod +x`, then the file), `.deb`, `.rpm` or `.tar.gz` with `install.sh`.
  You need a Vulkan driver and glibc 2.31 or newer.

The Windows installer and the AppImage update from inside the app: Settings → Updates.

Signing in uses your Yandex account's token. The app never gets your password. Glint is not
affiliated with Yandex. How to build it from source is in the README.
