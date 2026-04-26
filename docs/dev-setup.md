# Development setup

Per-platform prerequisites for building Actio from source. The repo is currently shipped Windows-only — macOS and Linux builds need additional work tracked in `COMPATIBILITY.md`, but local development should work on all three.

## All platforms

- **Rust** — stable, current edition. Install via [rustup](https://rustup.rs/).
- **Node.js** — 20 or newer.
- **pnpm** — 10 or newer (`npm install -g pnpm`).
- **Git** — for the source checkout. Configure `core.autocrlf=false` if you're on Windows; the repo's `.gitattributes` enforces LF for source files.

## Windows

The default `winget`/MSVC environment is sufficient. Install:

```powershell
winget install Microsoft.VisualStudio.2022.BuildTools
winget install Rustlang.Rustup
winget install OpenJS.NodeJS.LTS
npm install -g pnpm
```

When building Visual Studio Build Tools, enable the **"Desktop development with C++"** workload. This brings in the MSVC toolchain that `sherpa-onnx-sys` and `llama-cpp-sys-2` need.

WebView2 ships with Windows 11 by default; on Windows 10 you may need the [Evergreen runtime installer](https://developer.microsoft.com/microsoft-edge/webview2/).

## macOS

```bash
xcode-select --install   # Command Line Tools — clang, libtool, etc.
brew install rustup-init node pnpm cmake
rustup-init -y
```

`cmake` is needed by `llama-cpp-sys-2`. Apple Silicon machines should pick the `aarch64-apple-darwin` Rust target by default — verify with `rustc -vV`.

When you first run a debug build, macOS will prompt twice:
1. **Microphone access** — required for ASR. Grant it. If you accidentally deny, you can fix it in System Settings → Privacy & Security → Microphone.
2. **Accessibility access** — required for the dictation paste feature (simulates Cmd+V). Without it, paste silently no-ops.

If you skip both prompts, the app still launches but transcription and paste are non-functional.

## Linux (Debian / Ubuntu)

```bash
sudo apt update
sudo apt install -y \
  build-essential \
  curl \
  git \
  pkg-config \
  libssl-dev \
  libasound2-dev \
  libgtk-3-dev \
  libwebkit2gtk-4.1-dev \
  librsvg2-dev \
  libayatana-appindicator3-dev \
  libnotify-bin \
  cmake

curl https://sh.rustup.rs -sSf | sh -s -- -y
curl -fsSL https://deb.nodesource.com/setup_20.x | sudo -E bash -
sudo apt install -y nodejs
sudo npm install -g pnpm
```

### Why each package matters

| Package | Used by |
|---|---|
| `libasound2-dev` | `cpal` audio capture backend |
| `libgtk-3-dev`, `libwebkit2gtk-4.1-dev` | Tauri webview |
| `librsvg2-dev` | Tauri SVG icon rendering |
| `libayatana-appindicator3-dev` | Tauri tray icon + notifications |
| `libssl-dev`, `pkg-config` | `reqwest`, `sherpa-onnx-sys` |
| `cmake` | `llama-cpp-sys-2` |
| `libnotify-bin` (runtime, not dev) | Tauri notification fallback when AppIndicator is absent |

WebKitGTK 2.40+ is recommended for full CSS `:has()` support. Ubuntu 22.04 LTS ships 2.38; either upgrade to 24.04 or accept that some standby-tray styling will fall back to opaque (see COMPATIBILITY.md #23).

### AppImage runtime

Tauri's default Linux bundle target includes `.AppImage`, which requires FUSE2 at runtime (Ubuntu 22.04+ only ships FUSE3 by default):

```bash
sudo apt install libfuse2
```

Or skip AppImage in favor of `.deb`:

```bash
cd backend/src-tauri
cargo tauri build --bundles deb
```

## Linux (Fedora)

```bash
sudo dnf install -y \
  gcc gcc-c++ make pkgconfig openssl-devel alsa-lib-devel \
  gtk3-devel webkit2gtk4.1-devel librsvg2-devel \
  libayatana-appindicator-gtk3-devel libnotify cmake \
  curl git nodejs npm
sudo npm install -g pnpm
curl https://sh.rustup.rs -sSf | sh -s -- -y
```

## First build

```bash
git clone <repo-url>
cd Actio
cd frontend && pnpm install && cd ..
cd backend && cargo run --bin actio-asr   # backend on :3000
# in a separate terminal:
cd frontend && pnpm dev                     # vite on :5173
```

For the full desktop shell:

```bash
cd backend/src-tauri && cargo tauri dev
```

Cold builds take 5–15 min depending on machine — `sherpa-onnx-sys` and `llama-cpp-sys-2` are large C++ dependencies. Warm builds finish in seconds thanks to `[profile.dev.package."*"] opt-level = 3` (do not remove this from `Cargo.toml`; CPU inference is 50–100× slower without it).

## Running tests

```bash
cd backend && cargo test -p actio-core --lib
cd frontend && pnpm test
```

`pnpm tsc --noEmit` for a typecheck without running tests.

## Common pitfalls

**"Could not find sherpa-onnx-c-api.dll" on Windows.** The first `cargo build` populates `backend/target/release/`. If you start the Tauri dev shell before a release build has populated those binaries, `tauri.conf.json:bundle.resources` fails. Run `cargo build --release` once first, or run `cargo tauri dev` which builds the release-mode helper binaries on demand.

**ALSA "default" device not found on Linux.** Some headless or container environments have no ALSA devices. Either install PulseAudio (`sudo apt install pulseaudio`) or use `cpal::default_host()` device enumeration to pick a specific device in Settings → Audio.

**`Error: failed to acquire microphone permission` on macOS.** Open the app once with `cargo tauri dev`, hit a dictation hotkey, click "Allow" on the permission prompt, then restart the dev shell. Tauri's dev mode rebuilds the binary signature every launch so the OS occasionally requires re-granting.

**Tauri build fails with "Could not find icon icons/icon.icns" on macOS.** Generate the `.icns` from the existing PNG: `sips -s format icns icons/icon.png --out icons/icon.icns` and add it to `bundle.icon` in `tauri.conf.json` (currently omitted to avoid breaking Windows builds). See COMPATIBILITY.md #2.
