# Sidedoor

A dock that hides at a screen edge, with app launchers and native widget cards.
Built with Rust and GPUI Kit. Weather, Stats, and Clipboard are bundled TSX
plugins in [`crates/desktop/src/builtins`](crates/desktop/src/builtins); custom plugins use the same
[`SDK`](sdk/README.md).

## Workspace

The root is a virtual Cargo workspace. `cargo run` starts the `desktop` crate's
`sidedoor` binary. Built-in plugins live with the desktop app; the TypeScript
SDK remains its own Bun package at the repository root.

| Crate | Responsibility |
| --- | --- |
| `desktop` | Startup, app state, shared GPUI views, built-in TSX plugins, icons |
| `domain` | Pure models, config schema and migrations, clipboard rules, geometry, motion |
| `platform` | macOS/Windows/Linux APIs, native windows, clipboard, shortcuts, tray, paths |
| `services` | Config/history persistence, weather requests, system sampling, image downloads |
| `plugin-host` | Plugin manifests, protocol, discovery, installs from links, reload, Bun supervisor, SDK setup |

See [architecture](docs/architecture.md) for dependency boundaries and source layout.

## macOS

Run `cargo run`, or build and install the app with
`./scripts/package/macos.sh --install`. Bundles include Bun for the plugin runtime.

## Linux

Sidedoor runs on X11 desktops, and on Wayland sessions through XWayland
(it switches to X11 itself, since Wayland doesn't let an app follow the
pointer or place a dock). It looks the same as on macOS: the views are
shared, and text uses [Inter](https://rsms.me/inter/) when it is installed.
Windows keep the desktop's own title bar, as they do on Windows; on macOS
the traffic lights sit in the window's toolbar.

Install the build libraries and Inter, then run `cargo run`:

```sh
sudo apt install libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev \
  libvulkan-dev libfontconfig-dev libx11-xcb-dev libxcb-xkb-dev fonts-inter
```

`./scripts/package/linux.sh` builds a portable folder and tarball with Bun,
the SDK and the built-in plugins; `--install` also installs it for the
current user (`~/.local/opt/sidedoor`, a launcher entry and an icon) and
starts it. Settings are under `~/.local/share/sidedoor`. The tray icon
(Settings, Launch at Login, Reload, Quit) appears where the desktop shows
StatusNotifierItems; the dock's context menu always has **Dock Settings…**.
**Ctrl+Alt+V** opens Clipboard History in a new setup.

## Windows test builds

The [Windows workflow](.github/workflows/windows.yml) builds an x64 portable ZIP
containing `Sidedoor.exe`, Bun, the SDK, and built-in plugins. Extract the whole
folder before launching. Use the tray icon for Settings; **Ctrl+Alt+V** opens
Clipboard History.

To build on Windows, install Rust with the MSVC toolchain, Visual Studio C++
Build Tools with a Windows SDK (including `fxc.exe`), and Bun. Then run:

```powershell
cd sdk
bun install --frozen-lockfile
cd ..
./scripts/package/windows.ps1
```

The Windows port is undergoing desktop validation. See the
[Windows test checklist](docs/windows-testing.md) for installation, data paths,
and the interactions to check. Native blur and window decorations follow the
capabilities of the Windows version; widget content uses the shared UI.

## Checks

```sh
cargo fmt --all --check
cargo test --workspace --locked
cargo clippy --workspace --locked --all-targets -- -D warnings
cd sdk
bun test
bun run check
```

On an isolated Windows test session, the workflow also checks native clipboard
round trips and launches the packaged app. The clipboard test is ignored by
default because it replaces the session's clipboard contents.
