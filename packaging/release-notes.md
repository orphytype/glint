Resizing the window no longer leaks memory, and Glint uses less GPU and RAM while playing.

Fixed:

- Resizing the window repeatedly caused memory usage to keep growing. Glint now paces frames in
  flight and drops old blur textures on resize, so memory stays flat.
- Passive effects (the player glow, the Home garden, the seek bars and the Home butterflies) kept
  the window drawing at the monitor's full refresh rate. They now run at 30 frames per second.
  Input, scrolling, transitions, lyrics and scrolling titles still run at full refresh rate, and
  hidden windows stop drawing.
- Glass panes and edge blurs rebuilt their blur pyramids on every frame of an unrelated animation,
  such as a scrolling track title in the player bar. Pyramids now refresh only when what sits behind
  them changes, and the glass, garden and stage shaders skip work that is not visible.
- The cover cache kept an uncompressed 256×256 copy for every loaded cover and held up to 48 MiB in
  memory. Only the current and previous playing covers keep that sharp copy now, and the cache limit
  is 24 MiB.

Which file to download:

- Windows: `Glint-0.8.1-windows-x64-setup.exe` (installs for the current user, no administrator
  rights needed) or `Glint-0.8.1-windows-x64-portable.zip` (data stays next to the program while a
  `glint.portable` file sits next to `glint.exe`).
- macOS 11 and newer: `Glint-0.8.1-macos.dmg`. Open the image and drag Glint to Applications. The
  app is not signed with an Apple certificate, so on first launch right-click Glint, choose Open and
  confirm. If there is no Open button, allow it in System Settings → Privacy & Security.
- Linux: `.AppImage` (run `chmod +x`, then the file), `.deb`, `.rpm` or `.tar.gz` with `install.sh`.
  You need a Vulkan driver and glibc 2.31 or newer.

The Windows installer and the AppImage update from inside the app: Settings → Updates.

Signing in uses your Yandex account's token. The app never gets your password. Glint is not
affiliated with Yandex. How to build it from source is in the README.
