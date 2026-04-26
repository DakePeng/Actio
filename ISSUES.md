# Issues & TODOs

Originally a platform-compatibility audit (Windows is the only fully shipped target); now the catch-all for product-quality issues as well. Item numbers are preserved across edits so git history references still resolve — gaps in the numbering are issues that have already been fixed.

---

## Critical — blocks builds / core features on other platforms

### 2. macOS icon (`.icns`) missing (`tauri.conf.json`)

`backend/src-tauri/tauri.conf.json:36-39` only lists `icon.png` and `icon.ico`. Tauri's macOS bundle requires `icon.icns` or it falls back to a generic icon.

**Fix:** Generate `icons/icon.icns` from `icon.png` (e.g. `sips -s format icns icon.png --out icon.icns`). Add `"icons/icon.icns"` to the icons array. Coordinate with #42 — `cargo tauri icon` will produce all sizes in one shot.

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

### 9. Transparent window on Wayland may render opaque or crash

`backend/src-tauri/tauri.conf.json:22` sets `"transparent": true`. Tauri v2 on Wayland uses the `wgpu` backend; compositor-level window transparency depends on the Wayland compositor supporting the `xdg-decoration-unstable` and `ext-session-lock` protocols. On some compositors (especially bare sway/river) the window may appear with a solid black background.

**TODO:** Test on GNOME Wayland, KDE Wayland, and sway. Document workaround (`WEBKIT_DISABLE_COMPOSITING_MODE=1` or `--no-sandbox` equivalent if needed).

---

### 17. macOS code signing & notarization not configured

`tauri.conf.json` scaffold exists from iter 1 but `signingIdentity` / `providerShortName` are still null pending an Apple Developer ID. A `.app`/`.dmg` produced today is Gatekeeper-blocked everywhere except the build machine.

**Severity:** Critical · **Platform:** macOS · **Status:** Partial — config scaffold present; cert pending

**Fix:** Acquire an Apple Developer ID, then fill `signingIdentity` and `providerShortName` in `tauri.conf.json:bundle.macOS`. Wire `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` secrets into the macOS CI job and run notarization after build. See [Tauri's macOS notarization guide](https://v2.tauri.app/distribute/sign/macos/).

---

### 20. No runtime detection of denied microphone permission

Even after #16 (Info.plist with `NSMicrophoneUsageDescription`) was fixed, users can still *deny* the prompt or revoke the permission later in System Settings. Today the app has no detection — the failure mode is identical to a missing usage description (silent empty stream).

**Severity:** High · **Platform:** macOS (also Windows 10/11 with privacy settings)

**Fix:** Add a `check_microphone_permission` Tauri command that uses `AVCaptureDevice.authorizationStatus(for: .audio)` via `objc2`/`block2` crates on macOS. Wire it into the dictation start flow — if denied, surface a toast linking to System Settings → Privacy → Microphone.

---

### 21. Tauri updater config is Windows-only

`backend/src-tauri/tauri.conf.json:55-57` has `plugins.updater.windows.installMode` but nothing for macOS or Linux. The updater will still run on those platforms (it just lacks per-platform install hints), but the bigger issue is the **`latest.json` endpoint format** — it must include keys like `darwin-x86_64`, `darwin-aarch64`, `linux-x86_64` with signed bundle URLs. The current Windows-only release flow won't produce those.

**Severity:** High · **Platform:** macOS + Linux · **Status:** Partial — config + multi-OS release matrix landed in iter 7; CI work to write a multi-platform `latest.json` still pending.

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

### 46. Candidate Speakers panel floods with low-quality "Unknown" provisionals

The People → Candidate Speakers panel ("建议添加的人") shows a long list of `Unknown YYYY-MM-DD HH:MM` rows after even a short session. Most of them are clusters of one or two short segments (background noise, mic blips, momentary cross-talk, podcast cameos) that should never have been promoted to a speaker row in the first place.

Root cause: `backend/actio-core/src/engine/batch_processor.rs:500` (the production path `process_clip_production`) inserts a provisional speaker for **every** AHC cluster, with no minimum-segment-count gate and no minimum-duration gate. The sister function `process_clip_with_clustering` at `batch_processor.rs:222` does honor `cfg.min_segments_per_cluster` — but `min_segments_per_cluster` was never plumbed into the production path or `AudioSettings`, so the only filter that runs in the field is the cosine threshold itself.

What "high quality" should mean here:
- cluster has ≥ N segments (suggested default: 3) **or** total speech duration ≥ T ms (suggested default: 8000 ms), AND
- centroid distance from any existing speaker is comfortably above the confirm threshold (already enforced), AND
- per-tenant cap on auto-created provisionals per clip (e.g. ≤ 3) so a single noisy clip can't spawn a dozen rows.

Fix sketch:
- Add `cluster_min_segments: u32` (default 3) and `cluster_min_duration_ms: u32` (default 8000) to `AudioSettings` (`engine/app_settings.rs`).
- Plumb both into `ClusteringConfig` and apply the same `members.len() < min` (plus a duration sum) guard inside `process_clip_production`'s cluster loop, before the `insert_provisional` branch.
- Segments dropped by the gate keep `speaker_id = NULL` (existing behavior for the with_clustering path) — they still appear in transcripts, just unattributed.
- Backfill / GC: extend `provisional_voiceprint_gc_days` cleanup, or add a one-shot job, to delete provisional rows whose total `audio_segments` count is below the new threshold and whose `provisional_last_matched_at` hasn't been touched in N days.

UI follow-up (optional, after backend filter lands): sort the panel by aggregate evidence (segment count or total seconds spoken) descending so the user sees the strongest candidates first, and hide rows below a small floor entirely behind a "show all" affordance.

**Severity:** High · **Platform:** All

---

### 44. `use_batch_pipeline` makes streaming and batch mutually exclusive — both should run

`backend/actio-core/src/lib.rs:275-302` and `app_settings.rs:208-219`. The `audio.use_batch_pipeline` setting (default `true`) selects exactly one always-on pipeline:

- **`true`** → batch clip writer only. Audio recorded into ~5-min clips on disk, transcribed offline by `BatchProcessor`, results land in `audio_clips` / Archive Clips. Live tab gets **no** transcripts.
- **`false`** → legacy `InferencePipeline` only. Live transcripts stream to the WS aggregator → Live tab. **No** clip recording → Archive Clips empty.

The comment at `lib.rs:276` justifies the exclusion: *"both would try to grab the microphone"*. But users want both — live transcription **and** background clip archival in a single session. The fix is to share a single cpal capture and tee its output:

```
cpal::start_capture() -> mpsc<Vec<f32>>
            │
            ├─► InferencePipeline (streaming ASR + speaker id) → aggregator → WS
            │
            └─► CaptureDaemon → ClipWriter → audio_clips → BatchProcessor → DB
```

The `tee_audio()` helper in `inference_pipeline.rs:489-498` already exists for exactly this kind of fan-out. The work is:

1. Restructure `start_always_on_pipeline` (`lib.rs:608`) to always start a single capture, then tee into both consumers regardless of `use_batch_pipeline`.
2. Repurpose `use_batch_pipeline` as `enable_clip_archive: bool` — the user-facing knob is now "save clips to disk" rather than "swap pipelines".
3. Make sure `install_level_observer` (which feeds the audio_level WS broadcast) only runs once on the streaming branch — the batch branch shouldn't re-tee for that.
4. Migrate existing `settings.json` files: `use_batch_pipeline: true` → `enable_clip_archive: true`, `false → false` (legacy users keep their no-archive behavior).

**Severity:** High · **Platform:** All

**Workaround today:** users pick one or the other in Settings → Audio → "Use batch pipeline" toggle.

---

## Medium — build-time friction / developer experience

### 22. Missing `gen/schemas/macos-schema.json`

`backend/src-tauri/gen/schemas/` contains `desktop-schema.json`, `linux-schema.json`, and `windows-schema.json`, but no `macos-schema.json`. These are emitted by `tauri build` per platform and are needed for capability validation.

**Severity:** Medium · **Platform:** macOS

**Fix:** Run `tauri build --target aarch64-apple-darwin` (or `x86_64-apple-darwin`) on a macOS machine or in macOS CI; commit the resulting `macos-schema.json`.

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

### 42. App icon is a 1×1 placeholder — bundles ship without a real icon

`backend/src-tauri/icons/icon.png` is **1×1 pixels, 70 bytes** (visible from `file icon.png`). `icon.ico` is **92 bytes**. These are placeholder files generated when the project was scaffolded.

Tauri bundles call out to platform-specific icon generators:
- **Windows:** the `.ico` is consumed for the .exe icon, the taskbar entry, and the installer.
- **macOS:** a `.icns` is generated from the largest PNG; without 128× / 256× / 512× / 1024× sources, the resulting `.icns` is 1× scale and looks blank in Finder, the Dock (when activation policy isn't Accessory), and Cmd+Tab.
- **Linux:** `.deb` and `.rpm` packages embed PNG icons at 32× / 64× / 128× / 256×; with a 1× source, every launcher shows a generic placeholder.

The current Windows `.exe` ships with a near-blank icon today; the issue isn't macOS-specific but is masked on Windows because users mostly interact with the standby tray, not the taskbar.

**Severity:** Medium · **Platform:** All

**Fix:** Generate a proper icon set. Tauri ships a CLI helper:

```bash
cd backend/src-tauri
# Source: a square PNG at ≥1024×1024
cargo tauri icon path/to/source-icon.png
```

This regenerates `icon.png` (multi-res), `icon.ico` (multi-res), `icon.icns` (macOS), and per-size PNGs in `icons/`. Also resolves #2 (icns generation). Commit all generated files.

Without a designed icon yet, `cargo tauri icon` against a placeholder graphic still beats the 1×1 — at minimum, ship a recognizable color block until the real icon lands.

---

### 43. Native `window.confirm()` dialogs render inconsistently across WebViews

`frontend/src/components/CandidateSpeakersPanel.tsx:49`, `frontend/src/components/settings/ModelSetup.tsx:180`, and `:197` use `window.confirm(...)` for destructive-action confirmation:

```ts
if (!window.confirm(t('candidates.confirmDismiss'))) return;
```

`window.confirm()` is implemented per-WebView, with three meaningful differences:

| Platform | WebView | Dialog appearance / behavior |
|---|---|---|
| Windows | WebView2 (Chromium) | Native Windows confirm dialog, modal to the WebView frame |
| macOS | WKWebView | Native macOS confirm dialog, modal to the page; matches OS theme |
| Linux | WebKitGTK | Tauri's WebKitGTK build can render through GTK or **silently return false** depending on the embedder's `WebKitWebView` settings — the Tauri default in some 2.x versions disables native confirm to avoid IPC reentrance |

The third case is the real risk: a user on Linux clicking the "Dismiss candidate" button in `CandidateSpeakersPanel` could see *nothing happen* — the confirm returns false (the user never saw it), and the destructive action is skipped. They press the button again, same result. Frustrating bug class.

Even on Windows/macOS where it works, the look is jarringly OS-native against the app's custom Tailwind UI — looks like a security warning rather than a friendly app prompt.

**Severity:** Medium · **Platform:** All (worst on Linux/WebKitGTK)

**Fix:** Either:

1. Use the Tauri dialog plugin — add `tauri-plugin-dialog = "2"` and `@tauri-apps/plugin-dialog`:

```ts
import { ask } from '@tauri-apps/plugin-dialog';
const ok = await ask(t('candidates.confirmDismiss'), { kind: 'warning' });
if (!ok) return;
```

The plugin renders consistent native dialogs on all three platforms via Tauri's IPC and avoids the WebKitGTK quirk.

2. Or build a small in-app `<ConfirmDialog>` modal component using framer-motion (already a dep). This gives complete visual consistency with the rest of the UI and works in browser dev mode (no Tauri runtime needed).

Option 2 is more code but matches the app's existing visual language.

---

## Low — cosmetic / minor

### 29. `Ctrl+\` default shortcut is awkward on non-US keyboard layouts

The default `toggle_board_tray` shortcut is `Ctrl+\` (and on macOS, per the now-fixed #4, is `Cmd+\`). On AZERTY (French), QWERTZ (German), and Japanese JIS layouts, the backslash key requires `Alt Gr` or a multi-key sequence, making the shortcut effectively unreachable for those users. It works (they can rebind in settings) but the default is biased toward US keyboard users.

**Severity:** Low · **Platform:** All (UX concern, not technical)

**Fix:** Pick a default that works on all common layouts — e.g. `Ctrl+Shift+A` or `Ctrl+Space`. This is debatable; a settings-tour first-run experience may be a better answer than swapping defaults globally.

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

## Summary table (open items only)

| # | File | Severity | Platform | Status |
|---|------|----------|----------|--------|
| 2 | `tauri.conf.json:36-39` | Critical | macOS | Open |
| 5 | `actio-core/Cargo.toml:28` | Critical | macOS + Linux | Unverified |
| 6 | `actio-core/Cargo.toml:64` | Critical | macOS + Linux | Unverified |
| 9 | `tauri.conf.json:22` | High | Linux | Open |
| 17 | `tauri.conf.json` (macOS bundle) | Critical | macOS | Partial — scaffold; cert pending |
| 20 | (no permission check anywhere) | High | macOS + Windows | Open |
| 21 | `tauri.conf.json:55-57` | High | macOS + Linux | Partial — config; multi-platform `latest.json` pending |
| 22 | `gen/schemas/` missing macOS | Medium | macOS | Open |
| 26 | `actio-core/Cargo.toml:28,64` | Medium | All | Open |
| 29 | `app_settings.rs:327` | Low | All | Open |
| 32 | `tauri.conf.json` (no Windows signing) | Medium | Windows | Open |
| 38 | `audio_capture.rs:84-86` device name NFC | Low | macOS | Open |
| 42 | `icons/icon.png` 1×1 placeholder | Medium | All | Open |
| 43 | `window.confirm()` in 3 places | Medium | All (worst Linux) | Open |
| 44 | Streaming + batch pipelines mutually exclusive | High | All | Open |
| 46 | `batch_processor.rs:500` no min-cluster gate in production path | High | All | Open |
