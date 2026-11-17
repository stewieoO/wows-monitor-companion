# How to build

## Requirements

Install [Rust](https://www.rust-lang.org/tools/install), including Cargo.

## Build and run

From the repository root, run:

```sh
cargo run --release
```

## Cross-build for Windows from Linux

Install a MinGW-w64 linker and add the Rust GNU Windows target. On Debian or Ubuntu:

```sh
sudo apt install gcc-mingw-w64-x86-64
rustup target add x86_64-pc-windows-gnu
CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc \
  cargo build --release --target x86_64-pc-windows-gnu
```

The executable is `target/x86_64-pc-windows-gnu/release/wows-monitor-companion.exe`.
The Windows executable embeds the app icon from `assets/wows-monitor-icon.ico`.

On Fedora, install the cross-linker with `sudo dnf install mingw64-gcc`. On Arch Linux, install it with `sudo pacman -S mingw-w64-gcc`.

The Windows GNU target produces an executable with the GNU ABI. To build with the MSVC ABI, build on Windows with the MSVC toolchain.

On Linux, tray menus use the StatusNotifier/KStatusNotifier backend, so the desktop environment needs a compatible tray host.

## Publish a GitHub release

Push a version tag such as `v0.1.0` to start the GitHub Actions release workflow:

```sh
git tag v0.1.0
git push origin v0.1.0
```

Keep the tag version in sync with the package version in `Cargo.toml`. The workflow builds on native Linux and Windows runners, then creates a GitHub Release with generated release notes and attaches the Linux `.tar.gz` archive and Windows `.exe`.
