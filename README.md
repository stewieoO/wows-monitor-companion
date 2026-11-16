# WoWs Replay Watcher

A small Iced desktop app for Linux and Windows. It resolves the replay directory from a selected World of Warships game folder and copies `tempArenaInfo.json` to a selected destination whenever the file is created or changed.

## Use

1. Start the app and select the game installation folder that contains `preferences.xml` and `bin/`.
2. Select the folder where copied `tempArenaInfo.json` files should be placed.
3. Choose **Save and start monitoring**.

The app loads regional client and folder versions from `https://wows-monitor.com/api/game-version/`. If the API cannot be reached, it falls back to the installed `bin/` folders and `preferences.xml`, matching the browser app's fallback behavior.

The polling interval is 500 ms. The app watches the resolved replay folder and the relevant game configuration entries. If replay settings change, it resolves the replay folder again. An existing `tempArenaInfo.json` is copied when monitoring starts.

Closing the window leaves the app in the system tray. Right-click the tray icon to open the window or quit; reconfigure folders from the window. **Start this app when I log in** enables the platform's per-user startup entry and launches the app minimized.

## Build

Install Rust and Cargo, then run:

```sh
cargo run --release
```

### Cross-build for Windows from Linux

Use Rust's GNU Windows target and a MinGW-w64 linker. On Debian or Ubuntu:

```sh
sudo apt install gcc-mingw-w64-x86-64
rustup target add x86_64-pc-windows-gnu
CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc \
  cargo build --release --target x86_64-pc-windows-gnu
```

The executable is `target/x86_64-pc-windows-gnu/release/wows-replay-watcher.exe`. Fedora provides the cross-linker as `mingw64-gcc` (`sudo dnf install mingw64-gcc`); Arch Linux provides it in `mingw-w64-gcc` (`sudo pacman -S mingw-w64-gcc`).

The Windows GNU target produces a Windows executable with the GNU ABI. For an MSVC ABI build, build on Windows with the MSVC toolchain. Linux tray menus use the StatusNotifier/KStatusNotifier backend; the desktop environment must provide a compatible tray host.
