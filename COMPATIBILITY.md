# Platform Compatibility — Issues & TODOs

Current state: **Windows-only**. The app builds and ships for Windows. macOS and Linux targets require the work listed here before they are viable.

## Recent fixes (loop iteration 3)

- **#34** — `audio_capture.rs` now dispatches on `SampleFormat::F32 | I16 | U16`, converting i16/u16 to f32 in the callback before processing. macOS built-in mics (i16) and most Linux ALSA defaults (i16) now work; an explicit error message surfaces unknown formats instead of silent `SampleFormatNotSupported`. Hot path extracted into `process_chunk()` helper.
- **#36** — `useKeyboardShortcuts.ts` matcher now normalizes `e.key === ' '` to `'space'`; `KeyboardSettings.tsx` recorder substitutes `'Space'` when the bare-key value is `' '`. Persisted combos and runtime matching now agree on Space-bar bindings.

`cargo check -p actio-core --tests` ✓, `pnpm test` ✓ (151 tests), `pnpm tsc --noEmit` ✓.

## Recent fixes (loop iteration 2)

- **#10** — `docs/dev-setup.md` created with per-platform prerequisites (Windows MSVC, macOS Xcode CLT + brew, Debian/Ubuntu apt list, Fedora dnf list); explains AppImage/FUSE2, ALSA, microphone permission flow, and common pitfalls
- **#30** — `useGlobalShortcuts.ts` no longer force-overrides on every launch; fetches persisted shortcuts from `GET /settings` and merges with platform-aware defaults from `primaryMod`
- **#31** — `useKeyboardShortcuts.ts` matcher now checks `metaKey` alongside `ctrlKey`; treats `meta`/`cmd`/`command`/`super` as a single modifier slot. `ctrl`/`control`, `alt`/`option` similarly aliased.
- **`platform.ts`** — new `frontend/src/utils/platform.ts` exports `isMac` and `primaryMod` ("Super" on macOS, "Ctrl" elsewhere); used across the three shortcut-related files

`pnpm test` (151 tests) and `pnpm tsc --noEmit` both green after the changes.

## Recent fixes (loop iteration 1)

The following issues now have code-level fixes landed; some still need follow-up (cert plumbing, icon generation, real CI testing):

- **#3** — `paste_text` now uses `Cmd` modifier on macOS, `Ctrl` elsewhere (`backend/src-tauri/src/main.rs:520`)
- **#4** — `default_shortcuts()` emits `Super+...` on macOS, `Ctrl+...` elsewhere (`backend/actio-core/src/engine/app_settings.rs:324`)
- **#12** — `.gitattributes` now covers all text files with LF + binary file declarations
- **#13** — Font stack uses `system-ui`/`-apple-system`/`BlinkMacSystemFont` chain
- **#16** — `backend/src-tauri/Info.plist` created with `NSMicrophoneUsageDescription`, `NSAccessibilityUsageDescription`, `LSUIElement`
- **#17** *(partial)* — `tauri.conf.json` has `bundle.macOS` scaffold (`signingIdentity`/`providerShortName` left null pending Apple Developer ID); CI/secret wiring still needed
- **#18** — `backend/src-tauri/entitlements.plist` created with audio-input + JIT + library-validation entitlements
- **#19** — `app.set_activation_policy(Accessory)` added to setup() under `cfg(target_os = "macos")`; `macOSPrivateApi: true` set in `tauri.conf.json`
- **#21** *(partial)* — `tauri.conf.json` `bundle.linux.deb.depends` lists webkit/gtk/appindicator runtime deps; `bundle.macOS` scaffold present; per-platform `latest.json` payload still needs CI work
- **#27** — `Card.tsx:293` adds standard `lineClamp: 2` alongside the WebKit-prefixed form

`cargo check -p actio-core --tests` passes after the changes.

**Note on #11:** A `.github/workflows/release.yml` does exist but is Windows-only (`runs-on: windows-latest`, `--target x86_64-pc-windows-msvc`). The original audit said "no CI pipeline at all" — that was wrong. The real issue is the workflow needs macOS + Linux jobs added; see #33 below.

---

## Critical — blocks builds / core features on other platforms

### 1. Shared library bundling is Windows-only (`tauri.conf.json`)

`backend/src-tauri/tauri.conf.json:41-44` hard-codes four `.dll` paths in `bundle.resources`:

```json
"../target/release/onnxruntime.dll": "onnxruntime.dll",
"../target/release/sherpa-onnx-c-api.dll": "sherpa-onnx-c-api.dll",
...
```

macOS needs `.dylib` and Linux needs `.so`. Tauri `bundle.resources` doesn't support conditional paths natively.

**Fix:** Add a `build.rs` in `src-tauri` that emits `cargo:rustc-env=PLATFORM_LIBS=...` or use a Tauri build script to conditionally populate resources per `cfg!(target_os)`. Alternatively manage it in CI (copy correct libs before `tauri build`).

---

### 2. macOS icon (`.icns`) missing (`tauri.conf.json`)

`backend/src-tauri/tauri.conf.json:36-39` only lists `icon.png` and `icon.ico`. Tauri's macOS bundle requires `icon.icns` or it falls back to a generic icon.

**Fix:** Generate `icons/icon.icns` from `icon.png` (e.g. `sips -s format icns icon.png --out icon.icns`). Add `"icons/icon.icns"` to the icons array.

---

### 3. `paste_text` uses `Key::Control` — broken on macOS (`main.rs:520`)

`backend/src-tauri/src/main.rs:517-527` simulates `Ctrl+V` to paste via `enigo`:

```rust
enigo.key(Key::Control, Direction::Press)...
enigo.key(Key::Unicode('v'), Direction::Click)...
```

On macOS the paste shortcut is `Cmd+V` (`Key::Meta`), not `Ctrl+V`. The function silently succeeds (no error) but the paste never reaches the target input.

**Fix:**

```rust
#[cfg(target_os = "macos")]
let modifier = Key::Meta;
#[cfg(not(target_os = "macos"))]
let modifier = Key::Control;
enigo.key(modifier, Direction::Press)?;
enigo.key(Key::Unicode('v'), Direction::Click)?;
enigo.key(modifier, Direction::Release)?;
```

---

### 4. Default keyboard shortcuts use `Ctrl` — non-idiomatic on macOS (`app_settings.rs:327-329`)

```rust
m.insert("toggle_board_tray".into(), "Ctrl+\\".into());
m.insert("start_dictation".into(), "Ctrl+Shift+Space".into());
m.insert("new_todo".into(), "Ctrl+N".into());
```

`tauri-plugin-global-shortcut` interprets `Ctrl` as the literal Control key on macOS (not Command). App-level global shortcuts on macOS conventionally use `Cmd` (`Super`/`Meta`). `Ctrl+N`, `Ctrl+Shift+Space` collide with common macOS terminal/system bindings.

**Fix:** Ship platform-aware defaults:

```rust
#[cfg(target_os = "macos")]
m.insert("toggle_board_tray".into(), "Super+\\".into());
#[cfg(not(target_os = "macos"))]
m.insert("toggle_board_tray".into(), "Ctrl+\\".into());
// similarly for start_dictation and new_todo
```

Or use `CmdOrCtrl` if the shortcut parser supports it (check tauri-plugin-global-shortcut docs — v2 may not).

---

### 5. `sherpa-onnx` prebuilt shared libs unverified for macOS/Linux

`backend/actio-core/Cargo.toml:28` uses `sherpa-onnx = { version = "1.12.36", features = ["shared"] }`. The `shared` feature dynamically links against pre-built `.dll`/`.dylib`/`.so` files from the sherpa-onnx release. Availability for Linux (especially musl / aarch64) and macOS (Intel + Apple Silicon) must be confirmed.

**TODO:** Attempt `cargo build -p actio-core` on macOS arm64 and Linux x86_64. If the sherpa-onnx crate can't locate pre-built binaries for those targets, switch to static linking (`default-features = false, features = ["static"]`) or vendor the C source.

---

### 6. `llama-cpp-2` FFI bindings unverified for macOS/Linux

`backend/actio-core/Cargo.toml:64` pins `llama-cpp-2 = "=0.1.143"`. This crate wraps llama.cpp via FFI. On macOS it needs Metal/Accelerate support; on Linux it needs BLAS or CUDA. The build may also require `cmake` and C++ toolchain in the CI environment.

**TODO:** Build with `--features local-llm` on macOS and Linux. Document any required system packages (cmake, libclang, etc.) in the build README.

---

## High — features degraded or broken at runtime

### 7. `launchAtLogin` setting is stored but never acted on

The UI shows a "Launch at login" toggle (`PreferencesSection.tsx:82-83`) and the preference is persisted in `settings.json`, but no backend code registers or removes the app from the OS auto-start list.

Platform-specific implementations needed:
- **Windows:** `HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run`
- **macOS:** `~/Library/LaunchAgents/<bundle-id>.plist`
- **Linux:** `~/.config/autostart/<app>.desktop` (XDG) or a systemd user unit

The [tauri-plugin-autostart](https://github.com/tauri-apps/tauri-plugin-autostart) crate handles all three. Add it, wire it to the preferences update handler in `use-store.ts`.

---

### 8. `enigo` requires X11 on Linux — breaks under Wayland

`backend/src-tauri/Cargo.toml:19` depends on `enigo = "0.3"`. On Linux, enigo's keyboard simulation backend uses X11 (`xdotool`-style input). Under a pure Wayland session (no XWayland), `Enigo::new()` fails or the simulated keypresses have no effect.

The `paste_text` Tauri command is the only current call site — it would silently fail or error at runtime on Wayland.

**Fix:** Check enigo 0.3 changelog for Wayland support status. If unavailable, use `wtype`/`ydotool` as a fallback, or detect Wayland (`WAYLAND_DISPLAY` env var) and return a user-facing error with instructions to paste manually.

---

### 9. Transparent window on Wayland may render opaque or crash

`backend/src-tauri/tauri.conf.json:22` sets `"transparent": true`. Tauri v2 on Wayland uses the `wgpu` backend; compositor-level window transparency depends on the Wayland compositor supporting the `xdg-decoration-unstable` and `ext-session-lock` protocols. On some compositors (especially bare sway/river) the window may appear with a solid black background.

**TODO:** Test on GNOME Wayland, KDE Wayland, and sway. Document workaround (`WEBKIT_DISABLE_COMPOSITING_MODE=1` or `--no-sandbox` equivalent if needed).

---

## Medium — build-time friction / developer experience

### 10. Linux build requires undocumented system packages

`cpal` on Linux links against ALSA. A clean Debian/Ubuntu dev machine needs:

```
sudo apt install libasound2-dev libssl-dev pkg-config
```

For Tauri on Linux, additional packages are needed:

```
sudo apt install libgtk-3-dev libwebkit2gtk-4.1-dev librsvg2-dev
```

Neither is documented anywhere in the repo.

**Fix:** Add a `docs/dev-setup.md` (or a section in the root `README.md`) with per-platform prerequisites.

---

### 11. No CI pipeline for any platform

There is no `.github/workflows/` directory. All building and testing happens locally.

**TODO:** Add at minimum:
- `ci.yml` — `cargo check + test` and `pnpm test` on `ubuntu-latest`, `macos-latest`, `windows-latest`
- `release.yml` — `tauri build` artifacts for all three platforms on tag push

Reference: [Tauri's multi-platform build action](https://v2.tauri.app/distribute/github-actions/).

---

### 12. `.gitattributes` only enforces LF for `.sql` files

`.gitattributes` normalizes line endings for `*.sql` but not for Rust or TypeScript files. On Windows, git may check out `.rs`/`.ts` files with CRLF, causing `cargo fmt` and `prettier` diffs for macOS/Linux contributors.

**Fix:**

```gitattributes
* text=auto eol=lf
*.sql text eol=lf
*.bat text eol=crlf
```

---

## Low — cosmetic / minor

### 13. Font stack includes Segoe UI (Windows-only font)

`frontend/src/styles/globals.css:60`:

```css
--font-sans: 'Plus Jakarta Sans', 'Avenir Next', 'Segoe UI', sans-serif;
```

`Plus Jakarta Sans` is bundled as a WOFF2 so it loads correctly everywhere. `Segoe UI` is listed as a fallback but is not present on macOS or Linux — harmless since the chain proceeds to `sans-serif`. Still worth replacing with `system-ui` for the fallback slot:

```css
--font-sans: 'Plus Jakarta Sans', system-ui, sans-serif;
```

---

### 14. `skipTaskbar: true` is Windows/X11 only

`tauri.conf.json:26` sets `"skipTaskbar": true`. On macOS this key is silently ignored; dock presence is controlled separately via `"activationPolicy": "accessory"` in `tauri.conf.json` or via the `NSApp.setActivationPolicy(.accessory)` API. If the intent is a tray-only app with no Dock icon on macOS, that needs to be set explicitly.

**TODO:** Add `"macOSPrivateApi": true` and `activationPolicy` configuration for macOS if a dock-less experience is desired.

---

### 15. `windows_subsystem` attribute is harmless but misleading

`backend/src-tauri/src/main.rs:1`:

```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
```

This attribute is ignored by the compiler on non-Windows targets, so it doesn't break anything. It's fine as-is.

---

---

# Second-pass additions

The items below were found in a follow-up audit focused on macOS bundle metadata, OS permission flows, the Tauri updater, WebView quirks, and DPI scaling.

---

### 16. macOS microphone permission — silent failure without `NSMicrophoneUsageDescription`

There is no `Info.plist` anywhere under `backend/src-tauri/`. On macOS 10.14+, an app that opens a microphone stream without `NSMicrophoneUsageDescription` declared:

1. Receives an empty/zeroed audio stream from CoreAudio
2. Triggers no permission prompt
3. Logs nothing user-visible

The user will press the dictation hotkey and just see "listening…" forever with no transcript ever arriving. This is the single highest-impact macOS issue — every other ASR/dictation problem is downstream of it.

**Severity:** Critical · **Platform:** macOS

**Fix:** Create `backend/src-tauri/Info.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>NSMicrophoneUsageDescription</key>
  <string>Actio uses your microphone to transcribe and translate spoken audio.</string>
  <key>NSAccessibilityUsageDescription</key>
  <string>Actio simulates Cmd+V to paste dictated text into the focused window.</string>
</dict>
</plist>
```

Reference it from `tauri.conf.json` under `bundle.macOS.infoPlist`.

---

### 17. macOS code signing & notarization not configured

`tauri.conf.json` has no `bundle.macOS` section at all. A `.app`/`.dmg` produced today will be Gatekeeper-blocked on every machine other than the one that built it ("Actio is damaged and can't be opened" or "from an unidentified developer"). Apple Notarization is also required for distribution outside the App Store on macOS 10.15+.

**Severity:** Critical · **Platform:** macOS

**Fix:** Add to `tauri.conf.json`:

```json
"bundle": {
  "macOS": {
    "signingIdentity": "Developer ID Application: <Name> (<TeamID>)",
    "providerShortName": "<TeamID>",
    "entitlements": "entitlements.plist",
    "minimumSystemVersion": "10.15"
  }
}
```

Then wire `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` secrets into a macOS CI job that runs `tauri build --target universal-apple-darwin` followed by notarization. See [Tauri's macOS notarization guide](https://v2.tauri.app/distribute/sign/macos/).

---

### 18. macOS Hardened Runtime entitlements missing

Once code signing is enabled (#17), the Hardened Runtime is on by default. Without an `entitlements.plist`, three things break:

- Microphone access is rejected (`com.apple.security.device.audio-input`)
- `enigo`'s synthetic keystrokes are blocked unless Accessibility is granted (separate from the entitlement, but the entitlement is what allows it to be granted)
- llama-cpp-2 may fail to load at runtime if it allocates JIT pages without `com.apple.security.cs.allow-jit`

**Severity:** High · **Platform:** macOS

**Fix:** Create `backend/src-tauri/entitlements.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>com.apple.security.device.audio-input</key>
  <true/>
  <key>com.apple.security.cs.allow-jit</key>
  <true/>
  <key>com.apple.security.cs.allow-unsigned-executable-memory</key>
  <true/>
  <key>com.apple.security.cs.disable-library-validation</key>
  <true/>
</dict>
</plist>
```

`disable-library-validation` is needed because the bundled sherpa-onnx and llama-cpp-2 dylibs are signed with a different identity than the main app binary.

---

### 19. macOS Dock icon shows for a tray-only app — no `activationPolicy` set

The app intent is "tray-only" (`skipTaskbar: true`, custom standby tray UI). On Windows that hides the taskbar entry; on macOS it's a no-op and the app shows up in the Dock and Cmd+Tab list anyway. The fix is to set the activation policy to `Accessory`.

**Severity:** High · **Platform:** macOS

**Fix:** Tauri v2 has a Rust API for this — add to `setup` in `backend/src-tauri/src/main.rs`:

```rust
#[cfg(target_os = "macos")]
app.set_activation_policy(tauri::ActivationPolicy::Accessory);
```

The window also needs `"macOSPrivateApi": true` in `tauri.conf.json` for transparency + decorations: false to render correctly.

---

### 20. No runtime detection of denied microphone permission

Even after #16 is fixed, users can still *deny* the prompt or revoke the permission later in System Settings. Today the app has no detection — the failure mode is identical to #16 (silent empty stream).

**Severity:** High · **Platform:** macOS (also Windows 10/11 with privacy settings)

**Fix:** Add a `check_microphone_permission` Tauri command that uses `AVCaptureDevice.authorizationStatus(for: .audio)` via `objc2`/`block2` crates on macOS. Wire it into the dictation start flow — if denied, surface a toast linking to System Settings → Privacy → Microphone.

---

### 21. Tauri updater config is Windows-only

`backend/src-tauri/tauri.conf.json:55-57` has `plugins.updater.windows.installMode` but nothing for macOS or Linux. The updater will still run on those platforms (it just lacks per-platform install hints), but the bigger issue is the **`latest.json` endpoint format** — it must include keys like `darwin-x86_64`, `darwin-aarch64`, `linux-x86_64` with signed bundle URLs. The current Windows-only release flow won't produce those.

**Severity:** High · **Platform:** macOS + Linux

**Fix:**
1. Once macOS/Linux release artifacts exist, update the release pipeline to upload them and to write a `latest.json` with all platform keys.
2. Add to `tauri.conf.json`:
   ```json
   "plugins": {
     "updater": {
       "endpoints": ["..."],
       "pubkey": "...",
       "windows": { "installMode": "passive" }
     }
   }
   ```
   The macOS/Linux variants don't need explicit `installMode` — Tauri uses sensible defaults for those platforms (replace .app, replace AppImage).

---

### 22. Missing `gen/schemas/macos-schema.json`

`backend/src-tauri/gen/schemas/` contains `desktop-schema.json`, `linux-schema.json`, and `windows-schema.json`, but no `macos-schema.json`. These are emitted by `tauri build` per platform and are needed for capability validation.

**Severity:** Medium · **Platform:** macOS

**Fix:** Run `tauri build --target aarch64-apple-darwin` (or `x86_64-apple-darwin`) on a macOS machine or in macOS CI; commit the resulting `macos-schema.json`.

---

### 23. CSS `:has()` selector breaks on older WebKitGTK

`frontend/src/styles/globals.css:95,113,4084` uses `:has()`:

```css
html:has(body.body--standby) { background: transparent; }
html:has(body.body--quickadd) { ... }
.model-list__item:has(input[type="radio"]:checked) { ... }
```

WebKitGTK only added `:has()` in 2.40 (March 2023). Ubuntu 22.04 LTS ships 2.38 by default, Debian stable lags further. On those distros the standby tray will render with an opaque background instead of being transparent.

**Severity:** Medium · **Platform:** Linux

**Fix:** Either bump the minimum Linux requirement to a distro shipping WebKitGTK ≥ 2.40, or switch to JS-driven class toggling on `<html>` so the rule becomes `html.is-standby { ... }`. The latter is the safer path — `StandbyTray.tsx` already knows when it's in standby mode.

---

### 24. Linux notification / tray needs `libayatana-appindicator`

`tauri-plugin-notification = "2"` and Tauri's tray icon both rely on `libayatana-appindicator3-1` at runtime on most Linux desktops (GNOME, KDE, Cinnamon, XFCE under Ubuntu/Debian). On a barebones Wayland session without the indicator daemon, notifications silently no-op and the tray icon doesn't render.

**Severity:** Medium · **Platform:** Linux

**Fix:** Add to the build prerequisites doc (#10):

```
sudo apt install libayatana-appindicator3-dev
```

For Tauri to fall back to dbus notifications cleanly, also document `libnotify-bin` as a runtime dep.

---

### 25. AppImage bundling needs `appimagetool` in PATH

`tauri.conf.json` has `"targets": "all"`, which on Linux includes `.AppImage`. Tauri downloads `appimagetool` automatically on first build, but in sandboxed/offline CI it'll fail. Also, AppImages need FUSE2 at runtime; on Ubuntu 22.04+ FUSE2 isn't installed by default (only FUSE3).

**Severity:** Medium · **Platform:** Linux

**Fix:** Either narrow `targets` per platform in CI:

```yaml
- name: Build (Linux)
  run: cd backend/src-tauri && tauri build --bundles deb,rpm
```

(skipping AppImage for now), or document the FUSE2 runtime requirement and pre-install `libfuse2` in CI.

---

### 26. GPU acceleration features not opted into

`sherpa-onnx` and `llama-cpp-2` both have hardware-acceleration features that aren't enabled:

- `sherpa-onnx` ships `cuda`, `directml`, `coreml`, `tcuda` features. Currently only `shared` is enabled. On Apple Silicon, `coreml` would hand inference off to ANE and dramatically improve latency.
- `llama-cpp-2 = "=0.1.143"` has `metal` (macOS), `cuda` (Windows/Linux NVIDIA), `vulkan` features. None are enabled, so LLM inference falls back to CPU on every platform.

For Actio's primary loop (live transcription + translation + window action extraction), CPU-only is workable but slow on Apple Silicon and noticeably slower than competitors on Windows with NVIDIA GPUs.

**Severity:** Medium · **Platform:** All (per-platform features)

**Fix:** Add platform-conditional Cargo features:

```toml
[target.'cfg(target_os = "macos")'.dependencies]
sherpa-onnx = { version = "1.12.36", default-features = false, features = ["shared", "coreml"] }
llama-cpp-2 = { version = "=0.1.143", optional = true, features = ["metal"] }

[target.'cfg(target_os = "linux")'.dependencies]
sherpa-onnx = { version = "1.12.36", default-features = false, features = ["shared"] }
llama-cpp-2 = { version = "=0.1.143", optional = true, features = ["vulkan"] }
```

Verify each accelerated build before shipping — sherpa-onnx CoreML in particular has model-compatibility caveats.

---

### 27. WebKit `-webkit-line-clamp` without standard fallback

`frontend/src/components/Card.tsx:293-296` uses the legacy `-webkit-box`/`-webkit-line-clamp` combo for multi-line ellipsis. The standard `line-clamp` property has been in CSS since 2022 but isn't shipped to Card.tsx. Currently fine on all three WebView engines, but worth adding the unprefixed property as a forward-compat fallback.

**Severity:** Low · **Platform:** All

**Fix:** Just add `lineClamp: 2` alongside the existing `WebkitLineClamp: 2`. No behavioral change today, future-proofs against WebKit deprecating the prefixed form.

---

### 28. Fractional display scaling rounding errors on Linux

`backend/src-tauri/src/main.rs:375-376, 388-391` divides physical screen coordinates by `scale_factor` to convert to logical pixels. On Linux Wayland with fractional scaling (1.25, 1.5, 1.75 are common in GNOME/KDE), repeated round-trips between physical and logical accumulate rounding errors of ±1 pixel, causing the standby tray position to jitter by a pixel across launches.

**Severity:** Low · **Platform:** Linux

**Fix:** Round to nearest logical pixel before persisting:

```rust
let saved_x = (logical_x).round() as i32;
```

Test on KDE Wayland with 125% scale.

---

### 29. `Ctrl+\` default shortcut is awkward on non-US keyboard layouts

The default `toggle_board_tray` shortcut is `Ctrl+\` (and on macOS, per #4 above, should be `Cmd+\`). On AZERTY (French), QWERTZ (German), and Japanese JIS layouts, the backslash key requires `Alt Gr` or a multi-key sequence, making the shortcut effectively unreachable for those users. It works (they can rebind in settings) but the default is biased toward US keyboard users.

**Severity:** Low · **Platform:** All (UX concern, not technical)

**Fix:** Pick a default that works on all common layouts — e.g. `Ctrl+Shift+A` or `Ctrl+Space`. This is debatable; a settings-tour first-run experience may be a better answer than swapping defaults globally.

---

---

# Third-pass additions (loop iteration 1)

After applying the fixes above, a follow-up scan turned up four more compatibility issues focused on the frontend keyboard-shortcut layer and the existing CI workflow.

---

### 30. Frontend force-overrides backend shortcuts with hardcoded `Ctrl+...` on every launch

`frontend/src/hooks/useGlobalShortcuts.ts:9-14, 72` defines:

```ts
const DEFAULT_GLOBAL_SHORTCUTS: Record<string, string> = {
  toggle_board_tray: 'Ctrl+\\',
  start_dictation: 'Ctrl+Shift+Space',
  new_todo: 'Ctrl+N',
  toggle_listening: 'Ctrl+Shift+M',
};
// ...
invoke('reregister_shortcuts', { shortcuts: DEFAULT_GLOBAL_SHORTCUTS })
```

This re-registers **hardcoded** `Ctrl+...` shortcuts on every app launch, completely overriding the platform-aware backend defaults from #4. On macOS, the user gets `Ctrl+\` registered every time they open the app, regardless of what they bound in settings — until the settings page reads stored values and `patchAllShortcuts` fires.

This effectively voids the #4 fix for end users on macOS.

**Severity:** Critical · **Platform:** macOS

**Fix:** Either:
- Detect platform in JS (`navigator.platform`/`navigator.userAgent` includes `Mac`) and use `Super+...` strings, or
- Better: don't re-register on every launch; let the backend register the persisted shortcut map at startup. The frontend should only `invoke('reregister_shortcuts', ...)` after the user changes a binding in settings, not on every mount.

The same hardcoded strings exist in `frontend/src/components/settings/KeyboardSettings.tsx:9-27` as the fallback when the GET `/settings` request hasn't returned yet — which on slow startups means the settings panel briefly shows "Ctrl+..." labels even on macOS.

---

### 31. Frontend keyboard matcher checks only `ctrlKey`, never `metaKey`

`frontend/src/hooks/useKeyboardShortcuts.ts:16-30`:

```ts
function matchesShortcut(e: KeyboardEvent, combo: string): boolean {
  const parts = combo.split('+').map((p) => p.trim().toLowerCase());
  const key = parts[parts.length - 1];
  const needCtrl = parts.includes('ctrl');
  // ...
  return (
    eventKey === key &&
    e.ctrlKey === needCtrl &&
    e.shiftKey === needShift &&
    e.altKey === needAlt
  );
}
```

This handles **in-process** shortcuts (tab navigation: `Ctrl+1` … `Ctrl+5`, card up/down, etc.). Two problems on macOS:

1. The combo strings stored in settings are `Ctrl+1`, but a Mac user pressing `Cmd+1` sets `e.metaKey = true`, `e.ctrlKey = false`. No match.
2. Even if the user rebinds in settings via the recorder in `KeyboardSettings.tsx:127-130` (which DOES record `Meta`), the matcher above doesn't parse `meta`/`cmd`/`super` — it only inspects `ctrlKey`, `shiftKey`, `altKey`.

Net effect: tab navigation is **completely broken on macOS** even with the backend fix.

**Severity:** Critical · **Platform:** macOS

**Fix:**

```ts
function matchesShortcut(e: KeyboardEvent, combo: string): boolean {
  const parts = combo.split('+').map((p) => p.trim().toLowerCase());
  const key = parts[parts.length - 1];
  const needMeta = parts.some((p) => p === 'meta' || p === 'cmd' || p === 'super');
  const needCtrl = parts.includes('ctrl');
  const needShift = parts.includes('shift');
  const needAlt = parts.includes('alt');

  return (
    e.key.toLowerCase() === key &&
    e.metaKey === needMeta &&
    e.ctrlKey === needCtrl &&
    e.shiftKey === needShift &&
    e.altKey === needAlt
  );
}
```

Also update `DEFAULT_SHORTCUTS` in the same file to use a platform-aware constant.

---

### 32. No Windows code signing config — SmartScreen warning on first launch

`tauri.conf.json` has no `bundle.windows.certificateThumbprint` or signing config. The `.msi`/`.exe` produced by the existing release workflow is **unsigned**, so:

1. Windows SmartScreen shows "Windows protected your PC — Microsoft Defender SmartScreen prevented an unrecognized app from starting" on the first download.
2. Users must click "More info" → "Run anyway" to install.
3. The Tauri auto-updater also requires more user interaction for unsigned `.exe` updates (UAC prompt every time on `installMode: passive`).

For a 0.x app this is tolerable; for production it costs significant install conversions.

**Severity:** Medium · **Platform:** Windows

**Fix:** Acquire a Code Signing Certificate (~$70-200/yr for OV, ~$200-400/yr for EV from Sectigo/SSL.com/DigiCert), then add to `tauri.conf.json`:

```json
"bundle": {
  "windows": {
    "certificateThumbprint": null,
    "digestAlgorithm": "sha256",
    "timestampUrl": "http://timestamp.digicert.com",
    "tsp": false,
    "wix": { "language": ["en-US"] }
  }
}
```

Wire `WINDOWS_CERTIFICATE` and `WINDOWS_CERTIFICATE_PASSWORD` GitHub secrets into `release.yml` and have `tauri-action` consume them. EV certs can be Azure Key Vault-hosted to avoid storing the cert on disk.

---

### 33. Release workflow is Windows-only; no PR-time CI on any platform

`.github/workflows/release.yml` exists, but:

1. Single job `runs-on: windows-latest` — no macOS or Linux release artifacts produced.
2. Only triggers on `push: tags: 'v*.*.*'` — there is **no PR-time CI** running `cargo test`, `cargo clippy`, `pnpm test`, or `pnpm tsc --noEmit`. Regressions are caught only at release time.
3. The build target is hardcoded `x86_64-pc-windows-msvc` — no aarch64 Windows, no universal Apple Silicon.

This contradicts the original audit's "no CI pipeline" claim — there is one, just narrow.

**Severity:** High · **Platform:** All

**Fix:** Two new workflow files:

`.github/workflows/ci.yml` (PR-time validation):

```yaml
on: { pull_request: {}, push: { branches: [main] } }
jobs:
  test:
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: pnpm/action-setup@v3
        with: { version: 10 }
      - name: Install Linux deps
        if: matrix.os == 'ubuntu-latest'
        run: |
          sudo apt update
          sudo apt install -y libgtk-3-dev libwebkit2gtk-4.1-dev libayatana-appindicator3-dev libasound2-dev librsvg2-dev libssl-dev pkg-config
      - run: cd backend && cargo test -p actio-core --lib
      - run: cd frontend && pnpm install --frozen-lockfile && pnpm test && pnpm tsc --noEmit
```

Update `release.yml` to add macOS and Linux release jobs in matrix, each producing platform-appropriate artifacts and updating a single `latest.json`.

---

---

# Fourth-pass additions (loop iteration 2)

After landing the iteration-2 fixes, a follow-up scan turned up three more issues, including one critical audio-pipeline bug that would prevent ASR from starting at all on macOS or Linux even after the mic permission and entitlement work in iterations 1 and 2.

---

### 34. `cpal` stream callback hardcodes `f32` sample format — fails on most macOS / Linux devices

`backend/actio-core/src/engine/audio_capture.rs:97-126` reads the device's native config:

```rust
let supported = device.default_input_config()?;   // device-native format
// ...
let config: cpal::StreamConfig = supported.into(); // discards format info
let stream = device.build_input_stream(
    &config,
    move |data: &[f32], _info: &cpal::InputCallbackInfo| { /* f32 callback */ },
    /* error cb */, None,
)?;
```

`build_input_stream`'s `T: SizedSample` generic is monomorphized to `f32` here. cpal returns `BuildStreamError::SampleFormatNotSupported` if the device's native format isn't `f32`. In practice:

- **macOS:** built-in mics on most Macs deliver `i16` (CoreAudio's default for AirPods/MacBook mics is f32 in newer macOS but can fall back to i16 for legacy devices and some external USB mics). USB Class 1.0 mics commonly deliver i16.
- **Linux ALSA:** depends on the device; `i16` is the most common default for built-in laptops.
- **Windows WASAPI:** typically `f32`, which is why this works today.

The result on macOS / Linux: `start_capture()` returns `Err(SampleFormatNotSupported)`, the pipeline supervisor logs a warning and idles, dictation never produces transcripts. This is downstream of #16 (mic permission) but happens **even with permission granted and the entitlement set**.

**Severity:** Critical · **Platform:** macOS + Linux

**Fix:** Dispatch on `supported.sample_format()`:

```rust
use cpal::SampleFormat;
let stream = match supported.sample_format() {
    SampleFormat::F32 => device.build_input_stream::<f32, _, _>(
        &config,
        move |data, _| { handle_chunk(data) },
        err_cb, None,
    )?,
    SampleFormat::I16 => device.build_input_stream::<i16, _, _>(
        &config,
        move |data: &[i16], _| {
            let f: Vec<f32> = data.iter().map(|s| *s as f32 / i16::MAX as f32).collect();
            handle_chunk(&f);
        },
        err_cb, None,
    )?,
    SampleFormat::U16 => device.build_input_stream::<u16, _, _>(
        &config,
        move |data: &[u16], _| {
            let f: Vec<f32> = data.iter()
                .map(|s| (*s as f32 - 32768.0) / 32768.0)
                .collect();
            handle_chunk(&f);
        },
        err_cb, None,
    )?,
    fmt => return Err(anyhow::anyhow!("Unsupported sample format: {fmt:?}")),
};
```

Or pass `SupportedStreamConfig` to a dispatcher and let cpal's higher-level helpers do the conversion. Either way, the f32-only path will silently break on most non-Windows hardware.

---

### 35. Backend logs to stderr only — no log file, lost when bundled on any platform

`backend/actio-core/src/lib.rs:99-104` initializes tracing:

```rust
tracing_subscriber::fmt()
    .with_env_filter(...)
    .init();
```

`fmt()` writes to **stderr** by default. When the app runs:

- **Windows bundled** (`windows_subsystem = "windows"` set on release): no console attached, stderr is discarded.
- **macOS .app**: stderr goes to `Console.app`'s system log only if the user knows to look there; per-process logs aren't kept.
- **Linux**: depends on launcher — desktop-file launches to `~/.xsession-errors` or systemd-journald, terminal launches to terminal.

In all bundled cases, finding out *why* a release build of the backend failed (e.g., the #34 audio crash, an LLM model load failure, a migration error) requires either rebuilding from source with a console-attached binary or attaching a debugger.

**Severity:** Medium · **Platform:** All (worst when bundled)

**Fix:** Layer a rolling file appender alongside stderr. Add `tracing-appender = "0.2"` to `actio-core/Cargo.toml`, then:

```rust
let log_dir = config.data_dir.join("logs");
std::fs::create_dir_all(&log_dir).ok();
let file_appender = tracing_appender::rolling::daily(&log_dir, "actio.log");
let (file_writer, _guard) = tracing_appender::non_blocking(file_appender);
// _guard must live for the program's lifetime — return it to caller or leak

tracing_subscriber::fmt()
    .with_env_filter(...)
    .with_writer(file_writer.and(std::io::stderr))
    .init();
```

Add a "Show logs" button in Settings that calls `Tauri::shell::open` on `app_data_dir/logs/` so users can include logs in bug reports. On macOS this opens Finder; on Linux it opens xdg-open's default file manager.

---

### 36. `useKeyboardShortcuts.ts` matcher can't match `Space`-named shortcuts

After fixing #31, the matcher still has a latent bug: combo strings like `"Ctrl+Shift+Space"` (used in the `start_dictation` global shortcut and potentially future in-process bindings) compare `eventKey` to `"space"` lowercase, but `KeyboardEvent.key` for the spacebar is the literal space character `" "`. They never match.

```ts
const key = parts[parts.length - 1];        // "space"
return e.key.toLowerCase() === key && ...   // " " === "space" → false
```

This isn't currently triggered for `start_dictation` because that one is a global shortcut handled by Tauri (which uses `Code` rather than `key`). But if a future settings change makes any in-process binding land on Space (e.g. card-expand), it'd silently never fire.

**Severity:** Low · **Platform:** All

**Fix:** Normalize the comparison:

```ts
function normalizeKey(k: string): string {
  if (k === ' ') return 'space';
  if (k === '\\') return '\\';
  return k.toLowerCase();
}
// ...
return normalizeKey(e.key) === key && ...
```

Or just enumerate the known cases the user can record: `KeyboardSettings.tsx:135` already special-cases `key.length === 1`. Add a second case for `' '` → `'Space'` so the recorded combo string always uses `"Space"`, then teach the matcher the inverse.

---

---

# Fifth-pass additions (loop iteration 3)

After landing the iteration-3 fixes, I scanned the network/CORS surface and bundle origin handling — areas not covered in earlier passes.

---

### 37. CORS allow-list omits Tauri webview origins — production fetches likely blocked on macOS / Linux

`backend/actio-core/src/lib.rs:336-342`:

```rust
let cors = CorsLayer::new()
    .allow_origin([
        "http://localhost:1420".parse().unwrap(),
        "http://127.0.0.1:1420".parse().unwrap(),
        "http://localhost:5173".parse().unwrap(),
        "http://127.0.0.1:5173".parse().unwrap(),
    ])
    .allow_methods([Method::GET, Method::POST, Method::PATCH, Method::DELETE])
    .allow_headers([
        HeaderName::from_static("content-type"),
        HeaderName::from_static("x-tenant-id"),
    ]);
```

These are the **dev-server** origins — vite on 5173, Tauri-dev wrapper on 1420. In a production bundle, the WebView serves the frontend from a custom protocol scheme and the `Origin` header on requests to `http://127.0.0.1:3000` becomes:

| Platform | WebView | Origin header in production |
|---|---|---|
| Windows | WebView2 (Chromium) | `https://tauri.localhost` |
| macOS | WKWebView | `tauri://localhost` |
| Linux | WebKitGTK | `tauri://localhost` |

None of those are in `allow_origin`. tower-http's `CorsLayer` returns no `Access-Control-Allow-Origin` header for unmatched origins, and the browser blocks the response. Every `fetch('/...')` from the bundled frontend — including the `getApiBaseUrl()` health probe in `frontend/src/api/backend-url.ts:17`, the `KeyboardSettings` GET `/settings` call, and all the `actio-api.ts` mutations — would 0-byte fail with a CORS error.

The Windows production build apparently works today, suggesting WebView2's localhost-to-localhost fetches bypass CORS via a Chrome/Edge-specific exemption. WebKit-family WebViews (macOS WKWebView, Linux WebKitGTK) historically enforce CORS strictly even for localhost.

**Severity:** High · **Platform:** macOS + Linux (probable; not yet tested on hardware)

**Fix:** Either add the production origins explicitly:

```rust
.allow_origin([
    "http://localhost:1420".parse().unwrap(),
    "http://127.0.0.1:1420".parse().unwrap(),
    "http://localhost:5173".parse().unwrap(),
    "http://127.0.0.1:5173".parse().unwrap(),
    "tauri://localhost".parse().unwrap(),
    "https://tauri.localhost".parse().unwrap(),
])
```

Or use a predicate that admits any localhost-bound origin:

```rust
use tower_http::cors::AllowOrigin;
let cors = CorsLayer::new().allow_origin(AllowOrigin::predicate(|origin, _req| {
    let s = origin.to_str().unwrap_or("");
    s.starts_with("http://localhost")
        || s.starts_with("http://127.0.0.1")
        || s == "tauri://localhost"
        || s == "https://tauri.localhost"
}));
```

The predicate approach is safer because it admits the production scheme without hardcoding it everywhere.

---

### 38. Audio device names with non-ASCII characters may not round-trip cleanly

`frontend/src/api/actio-api.ts` settings round-trip stores the audio input device name as a JSON string. cpal returns device names as `String` from the OS:
- Windows: WASAPI returns names from the registry, often UTF-16 surrogate pairs (Japanese kana, Cyrillic, etc.)
- macOS: CoreAudio returns CFString, normalized to UTF-8
- Linux ALSA: returns whatever the device descriptor strings contain — which can be ASCII for built-in mics but raw bytes for some USB mics

For users with non-ASCII device names (Japanese: "内蔵マイク", Chinese: "内置麦克风", Cyrillic, etc.), the device-picker UI is fine (JSON handles UTF-8), but the OS-side device matching uses byte-equality comparison in `audio_capture.rs:84-86`:

```rust
.find(|d| d.name().ok().as_deref() == Some(name))
```

If the device name was stored with one Unicode normalization (NFC) and the OS now reports a different normalization (NFD on macOS HFS+), the match fails and falls back to "Audio device not found".

**Severity:** Low · **Platform:** All (more likely on macOS due to NFC↔NFD differences)

**Fix:** Normalize both sides to NFC before comparison. Add the `unicode-normalization` crate and:

```rust
use unicode_normalization::UnicodeNormalization;
let target: String = name.nfc().collect();
.find(|d| d.name().ok().map(|n| n.nfc().collect::<String>()) == Some(target.clone()))
```

Worth doing only after macOS testing reveals an actual mismatch.

---

### 39. WebSocket URL hardcoded to `ws://127.0.0.1:3000/ws` ignores `getApiBaseUrl()` port discovery

`frontend/src/hooks/useGlobalShortcuts.ts:180`:

```ts
const ws = new WebSocket('ws://127.0.0.1:3000/ws');
```

The HTTP layer probes ports 3000-3009 in `backend-url.ts:1-2` (`FALLBACK_PORTS`) so a developer can run the app while another process holds 3000. But the WebSocket is hardcoded to 3000 — if the backend bound to 3001, dictation can't open its WebSocket.

The `useVoiceStore` has the same pattern (likely — see `frontend/src/store/use-voice-store.ts`'s `MockWebSocket` test fixture which uses `ws://127.0.0.1:3000/ws`).

**Severity:** Medium · **Platform:** All (port-conflict scenario)

**Fix:** Use `getWsUrl('/ws')` from `backend-url.ts` instead of hardcoding the URL. The helper is already exported but unused:

```ts
const ws = new WebSocket(await getWsUrl('/ws'));
```

---

## Combined summary table

| # | File | Severity | Platform | Status |
|---|------|----------|----------|--------|
| 1 | `tauri.conf.json:41-44` | Critical | macOS + Linux | Open |
| 2 | `tauri.conf.json:36-39` | Critical | macOS | Open |
| 3 | `main.rs:520` | Critical | macOS | **Fixed** |
| 4 | `app_settings.rs:327-329` | Critical | macOS | **Fixed (backend)** — see #30 |
| 5 | `actio-core/Cargo.toml:28` | Critical | macOS + Linux | Unverified |
| 6 | `actio-core/Cargo.toml:64` | Critical | macOS + Linux | Unverified |
| 7 | `PreferencesSection.tsx:82-83` | High | All | Open |
| 8 | `src-tauri/Cargo.toml:19` | High | Linux | Open |
| 9 | `tauri.conf.json:22` | High | Linux | Open |
| 10 | `docs/dev-setup.md` | Medium | All | **Fixed** |
| 11 | `release.yml` Windows-only | Medium | All | See #33 |
| 12 | `.gitattributes` | Medium | macOS + Linux | **Fixed** |
| 13 | `globals.css:60` | Low | macOS + Linux | **Fixed** |
| 14 | `tauri.conf.json:26` | Low | macOS | **Fixed via #19** |
| 15 | `main.rs:1` | Info | — | No action needed |
| 16 | `Info.plist` (missing) | Critical | macOS | **Fixed** |
| 17 | `tauri.conf.json` (no macOS bundle config) | Critical | macOS | **Partial** — scaffold added; cert pending |
| 18 | `entitlements.plist` (missing) | High | macOS | **Fixed** |
| 19 | `tauri.conf.json:24` + `main.rs` setup | High | macOS | **Fixed** |
| 20 | (no permission check anywhere) | High | macOS + Windows | Open |
| 21 | `tauri.conf.json:55-57` | High | macOS + Linux | **Partial** — config; CI work pending |
| 22 | `gen/schemas/` missing macOS | Medium | macOS | Open |
| 23 | `globals.css:95,113,4084` | Medium | Linux | Open |
| 24 | (build doc) | Medium | Linux | Open |
| 25 | `tauri.conf.json:35` | Medium | Linux | Open |
| 26 | `actio-core/Cargo.toml:28,64` | Medium | All | Open |
| 27 | `Card.tsx:293-296` | Low | All | **Fixed** |
| 28 | `main.rs:375-391` | Low | Linux | Open |
| 29 | `app_settings.rs:327` | Low | All | Open |
| 30 | `useGlobalShortcuts.ts:9-14,72` | Critical | macOS | **Fixed** |
| 31 | `useKeyboardShortcuts.ts:16-30` | Critical | macOS | **Fixed** |
| 32 | `tauri.conf.json` (no Windows signing) | Medium | Windows | Open |
| 33 | `release.yml`, no `ci.yml` | High | All | Open |
| 34 | `audio_capture.rs:124` | Critical | macOS + Linux | **Fixed** |
| 35 | `lib.rs:99-104` | Medium | All | Open |
| 36 | `useKeyboardShortcuts.ts` Space key | Low | All | **Fixed** |
| 37 | `lib.rs:336-342` CORS origins | High | macOS + Linux | Open |
| 38 | `audio_capture.rs:84-86` device name NFC | Low | macOS | Open |
| 39 | `useGlobalShortcuts.ts:180` hardcoded WS port | Medium | All | Open |
