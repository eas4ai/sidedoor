# Sidedoor Windows test build

Run the setup EXE or MSI to install into `Program Files\Sidedoor` with a Start
menu shortcut and an uninstall entry. Both installers require administrator
access and include Bun and the complete plugin payload. For the portable build,
extract the whole ZIP to a folder, then run `Sidedoor.exe`. Keep `bun.exe` and
the `resources` folder beside it. No separate Bun installation or administrator
access is required for the ZIP. This is an unsigned test build, so Windows may show a
publisher warning.

The dock starts at the right screen edge. Its notification-area icon opens
Settings with a left click; right-click for Settings, launch at login, Reload,
and Quit. Clipboard History opens with **Ctrl+Alt+V**.

Please check:

- Install with each installer, upgrade from an older numeric version, and
  uninstall. Check that the shortcut and application files are removed while
  settings and plugins remain. Test the portable ZIP separately.
- Reveal and hide the dock at the left, right, and bottom edges.
- Hover Weather, Stats, and Clipboard; check rounded cards, arrows, text spacing,
  blur, and transitions against the Mac version.
- Copy text, a link, an image, and a file. Open History, search, then copy an item
  back into another app. Check that closing History restores keyboard focus.
- Drop an executable or Start-menu shortcut onto the dock and launch it.
- Open Settings, switch light/dark/system appearance, reorder items, and restart.
- Create a plugin, edit its TSX, and check reloads and plugin windows.
- Try Windows display scaling at 100%, 150%, and 200%; turn Windows transparency
  and animation effects off and check that the app remains readable and usable.
- Enable launch at login, sign out and in, then disable it again.

Include your Windows version, display scale, and a screenshot when reporting a
visual issue. The Windows build and automated tests do not prove desktop visual
or focus behavior; these checks still need a real Windows session.

Settings, history, and saved clipboard images are stored in `%APPDATA%\Sidedoor`;
cached app icons are stored in `%LOCALAPPDATA%\Sidedoor\Cache`. Quit before replacing the extracted
application folder. Keep your data folders when updating.
