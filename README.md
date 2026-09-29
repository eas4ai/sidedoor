# Sidedoor

A dock that hides at a screen edge, with app launchers and native widget cards.
Built with Rust and GPUI Kit. Weather, Stats, and Clipboard are bundled TSX
plugins in [`src/builtins`](src/builtins); custom plugins use the same
[`SDK`](sdk/README.md).

## macOS

Run `cargo run`, or build and install the app with
`./scripts/bundle.sh --install`. Bundles include Bun for the plugin runtime.

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
./scripts/bundle-windows.ps1
```

The Windows port is undergoing desktop validation. See the
[Windows test checklist](docs/windows-testing.md) for installation, data paths,
and the interactions to check. Native blur and window decorations follow the
capabilities of the Windows version; widget content uses the shared UI.

## Checks

```sh
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cd sdk
bun test
bun run check
```

On an isolated Windows test session, the workflow also checks native clipboard
round trips and launches the packaged app. The clipboard test is ignored by
default because it replaces the session's clipboard contents.
