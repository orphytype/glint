Glint gets a redesigned My Wave Home screen, a reworked floating mini player, streamlined six-tab Settings, smoother transitions across the app, and faster startup with lower memory use.

New:

- Home opens on a full-bleed My Wave stage with a 3D fluid wave behind the album cover and an infinitely scrolling Cormorant preset wheel on the right. The stage follows whatever is playing (My Wave, album, single, playlist, artist, liked tracks, or history) with a short clickable source pill above the cover, and a corner button at the bottom right opens the mini player.
- The Explore page holds catalog shelves (new releases, charts, playlists) next to Home in the sidebar.
- The floating mini player has cover-tinted backdrops, hover-revealed window controls, interactive seeking in all three layouts (including the compact bar), a full-bleed cover card layout, and keyboard shortcuts (`Space`, arrows, `M`, `L`, `P`, `Tab`, `Esc`).
- Settings is rebuilt into six tabs (General, Sound, Player, Shortcuts, Storage, About), removing unused toggles and redundant notes, collapsing the equalizer when off, and putting Reset in the header when changed.
- The Player look popover uses compact single-line rows and a toggle switch for background effects without nested boxes.
- Opening and closing Now Playing, the bottom dock bar, Settings, dialogs, and navigation pills animate cleanly across the app.

Fixed:

- Shaders compile per screen and the wave field builds off the main thread, cutting startup from several seconds to under a second and keeping idle memory below 100 MB on every page.
- Covers no longer reload or flash while scrolling or when opening and closing Now Playing.
- Removed the top blur veil that smeared over artist headers and buttons under the title bar.
- Track rows, lists, and menus now use the same heart and dislike glyphs as the player instead of Phosphor hearts.
- Singles show as Single instead of Album in source pills and track details, and source pills stay compact so they do not crowd the track title.
- Artist headers drop the redundant "Artist" label and highlight monthly listeners.
- Section icons load immediately on Explore, and cover-tinted backdrops show reliably on album, playlist, and artist pages in GPU mode.
- Queue rows use even padding around the cover on hover, search fields clip long text cleanly, and text inputs only blink a caret when focused.
- Updated RUNE to the latest commit (`570cac5`).

Which file to download:

- Windows: `Glint-0.10.0-windows-x64-setup.exe` (installs for the current user, no administrator rights needed) or `Glint-0.10.0-windows-x64-portable.zip` (data stays next to the program while a `glint.portable` file sits next to `glint.exe`).
- macOS 11 and newer: `Glint-0.10.0-macos.dmg`. Open the image and drag Glint to Applications. The app is not signed with an Apple certificate, so on first launch right-click Glint, choose Open and confirm. If there is no Open button, allow it in System Settings -> Privacy & Security.
- Linux: `.AppImage` (run `chmod +x`, then the file), `.deb`, `.rpm` or `.tar.gz` with `install.sh`. You need a Vulkan driver and glibc 2.31 or newer.

The Windows installer and the AppImage update from inside the app: Settings -> About.

Signing in uses your Yandex account's token. The app never gets your password. Glint is not affiliated with Yandex. How to build it from source is in the README.
