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

**Status:** Resolved — backend gate landed 2026-04-26. UI sort/cap and provisional-row GC remain as separate follow-ups (see Out-of-scope below).

The People → Candidate Speakers panel ("建议添加的人") shows a long list of `Unknown YYYY-MM-DD HH:MM` rows after even a short session. Most of them are clusters of one or two short segments (background noise, mic blips, momentary cross-talk, podcast cameos) that should never have been promoted to a speaker row in the first place.

Root cause: `backend/actio-core/src/engine/batch_processor.rs:500` (the production path `process_clip_production`) inserts a provisional speaker for **every** AHC cluster, with no minimum-segment-count gate and no minimum-duration gate. The sister function `process_clip_with_clustering` at `batch_processor.rs:222` does honor `cfg.min_segments_per_cluster` — but `min_segments_per_cluster` was never plumbed into the production path or `AudioSettings`, so the only filter that runs in the field is the cosine threshold itself.

What "high quality" should mean here:
- cluster has ≥ N segments (suggested default: 3) **or** total speech duration ≥ T ms (suggested default: 8000 ms), AND
- centroid distance from any existing speaker is comfortably above the confirm threshold (already enforced), AND
- per-tenant cap on auto-created provisionals per clip (e.g. ≤ 3) so a single noisy clip can't spawn a dozen rows.

Fix landed (this commit):
- Added `cluster_min_segments: u32` (default 3) and `cluster_min_duration_ms: u32` (default 8000) to `AudioSettings` with overlay clamps `[1,50]` and `[0, 600_000]`.
- Extended `ClusteringConfig` with `min_duration_ms` and added a shared `cluster_passes_gate` helper. Both `process_clip_production` and `process_clip_with_clustering` now AND-gate clusters on segment count + summed duration before minting a provisional speaker. Segments in dropped clusters keep `speaker_id = NULL`.
- Three new unit tests pin the behavior: cluster below count is dropped, cluster below duration is dropped, cluster meeting both floors mints exactly one provisional.

Out of scope (follow-up tickets):
- Backfill / GC of existing low-evidence provisional rows in user databases.
- Frontend Settings UI to surface the two knobs (defaults are sensible; advanced users can edit settings.json directly today).
- Reordering / hiding rows in the Candidate Speakers panel by aggregate evidence.

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

**Status:** Resolved — Option 2 implemented 2026-04-26. New `ConfirmDialog` + `useConfirm()` hook (`frontend/src/components/ConfirmDialog.tsx`) replaces all three `window.confirm()` callers. Promise-based, framer-motion animated, keyboard-driven (Esc cancels, Enter confirms), tone variants (`warning` / `destructive`), no new runtime dep. Vitest pins the modal flow.

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

### 47. Dead i18n keys in `en.ts` / `zh-CN.ts` — orphaned strings from removed UI

**Status:** Resolved 2026-04-26 — all 22 keys deleted from both `en.ts` and `zh-CN.ts`. Stale section comments removed (`// Priority values (for interpolation)`, `// State descriptors…`). Parity test green; tsc clean; full test suite 177/177; prod bundle dropped 2.1 kB.


A grep pass over `frontend/src/` for each key declared in `frontend/src/i18n/locales/en.ts` finds **22 keys with zero usages** in code (excluding the locale files themselves and excluding dynamic patterns like `t(\`model.desc.${id}\`)`, `t(\`live.translate.lang.${lang}\`)`, `t(\`settings.preferences.theme.${key}\`)`). They've been carried in both `en.ts` and `zh-CN.ts` since at least the always-on listening refactor — likely orphans from feature renames (`tray.state.*` → `tray.aria.*`, `priority.*` → `board.priority.*` / `card.priority.*`).

Confirmed dead (en + zh-CN parity preserved — both files have the same dead set):

```
feedback.modelSwitched
live.aria.toggleListening
live.translate.disabledTooltip
live.translate.pausedToast
priority.high
priority.low
priority.medium
recording.aria.startTranscribing
recording.aria.stopTranscribing
recording.loadingModel
recording.modelLoadFailed
recording.tapToTranscribe
tray.state.error
tray.state.listening
tray.state.processing
tray.state.standby
tray.state.success
tray.state.transcribing
tray.status.freshCapturesMany
tray.status.freshCapturesOne
tray.status.quiet
```

**Severity:** Low · **Platform:** All · **Type:** refactor / cleanup · **Scope:** small

**Acceptance:**
1. Remove all 22 keys from both `frontend/src/i18n/locales/en.ts` and `frontend/src/i18n/locales/zh-CN.ts` in a single commit.
2. `pnpm test` (parity test stays green — both files lose the same set, parity holds).
3. `pnpm tsc --noEmit` (no `TKey` widening introduces stranded type references).
4. `pnpm build` (catches any prod-only mismatch).

Verification command for "is it really dead?" before removing each key:

```bash
grep -rE "['\"\`]<key>['\"\`]" frontend/src --include='*.ts' --include='*.tsx' | grep -v 'i18n/locales'
```

Empty output → safe to drop.

---

### 48. Stale `TODO(Phase 3-4)` comment in `api/ws.rs:93` misrepresents pipeline state

**Status:** Resolved 2026-04-26 — comment block deleted; replaced with a one-paragraph note clarifying that `/ws` is broadcast-out only (capture comes from `CaptureDaemon` / `LiveStreamingService`).


`backend/actio-core/src/api/ws.rs:93` carries the comment

```rust
// TODO(Phase 3-4): Wire cpal audio capture → VAD → ASR pipeline here.
// For now, accept messages but don't process audio — the inference pipeline
// doesn't exist yet. Transcript events will be pushed once ASR is integrated.
```

…immediately above code that already wires the pipeline through `state.aggregator.subscribe()`, `state.aggregator.subscribe_speaker()`, and `state.audio_levels.subscribe()` (lines 97–99). The pipeline integration the TODO predicts has been done for several iterations — the comment is misleading code archaeology and contradicts the code below it.

The "Phase 3-4" label refers to a long-superseded plan; the current architecture (CaptureDaemon + ClipWriter + BatchProcessor / LiveStreamingService) is documented in `CLAUDE.md` and `engine/AGENTS.md`. A new contributor reading `ws.rs` will trust the comment over the code.

**Severity:** Low · **Platform:** All · **Type:** docs · **Scope:** small (1-line/3-line comment delete)

**Acceptance:**
- Delete the stale TODO block at `ws.rs:93–95`. The audio handling at line 103 already has a clarifying comment ("Audio chunks received but inference pipeline not yet connected") that itself is outdated — replace it with one line noting the WS path is broadcast-out only; capture comes from `CaptureDaemon`.
- `cargo check -p actio-core` clean.

No behaviour change; this is a comment-only fix.

---

### 51. `@tauri-apps/api` mixed static + dynamic imports defeat code-splitting

`pnpm build` emits two warnings — `core.js` and `window.js` are each dynamically imported in some files but statically imported in others, so Rollup can't move them into a separate chunk. Concretely:

```
core.js
  dynamic: App.tsx (×3), BoardWindow.tsx (×2), TraySection.tsx (×1)
  static : StandbyTray.tsx, KeyboardSettings.tsx, useGlobalShortcuts.ts,
           utils/autostart.ts (and re-exported by api/dpi.js, event.js,
           image.js, window.js inside the package)

window.js
  dynamic: BoardWindow.tsx
  static : StandbyTray.tsx
```

Net effect: the entire `@tauri-apps/api` surface lands in the main bundle, which is currently `555.36 kB` (`index-*.js`) — Vite is already flagging it as over its 500 kB warning threshold. The dynamic imports were *intended* to defer Tauri to browser-fallback or feature-gated paths, but the static chain (re-exports inside the package + 4 files in our code) keeps it eagerly loaded.

The four static-importers are all Tauri-only features (autostart, global shortcuts, standby tray invoke calls, keyboard settings keybinder), so they can be re-shaped to dynamic-import the API inside their handlers — same pattern the file's sibling components already use.

**Severity:** Low · **Platform:** All · **Type:** perf · **Scope:** small (4 file edits, all the same shape)

**Acceptance:**
1. Convert the four static imports to dynamic (`const { invoke } = await import('@tauri-apps/api/core')`) inside their handlers. Use a single shared helper if the call sites end up with copy-paste boilerplate.
2. `pnpm build` no longer emits the two `(!)` warnings about `core.js` / `window.js` static-vs-dynamic mixing.
3. Main chunk drops by ~30 kB (rough estimate for the `@tauri-apps/api` surface — actual win confirmed at acceptance time).
4. `pnpm test` (177/177) and `pnpm tsc --noEmit` stay green.

Worth doing as a single tick once the sibling-component pattern is applied uniformly. Verify by running `pnpm build` before/after and capturing the chunk-size delta in the commit message.

---

### 49. CLAUDE.md mis-describes the translation pipeline as session-based

**Status:** Resolved 2026-04-26 — both occurrences (Audio & inference pipeline section) corrected to drop "translation" from the unfinished-migration list. Added a paragraph in the LLM router section clarifying that `POST /llm/translate` shares the router but never enters the audio pipeline.

`CLAUDE.md` lines 63 and 78 both claim:

> Dictation, translation, and live voiceprint enrollment all still call `InferencePipeline::start_session` regardless of the flag — flipping those to `LiveStreamingService` is the last unfinished migration step.

But `backend/actio-core/src/api/translate.rs` is a stateless `POST /llm/translate` handler that calls the LLM router directly — there is **no** session lifecycle for translation, and `start_session` is never invoked from that file (the `InferencePipeline` import on line 98 is in the `#[cfg(test)] mod tests` block only). Translation has nothing to "migrate to LiveStreamingService" because it's not a streaming-capture feature at all.

The accurate statement is: **Dictation and live voiceprint enrollment** still call `InferencePipeline::start_session` (in `api/session.rs:68` and `:680`). Translation is decoupled from the audio pipeline and routes through `LlmRouter::translate_lines`.

A new contributor reading CLAUDE.md will look for a translation start_session call that doesn't exist.

**Severity:** Low · **Platform:** All · **Type:** docs · **Scope:** small (2-line edit in CLAUDE.md)

**Acceptance:**
1. Edit both occurrences (lines 63 and 78) to drop "translation" from the unfinished-migration list.
2. Add one sentence near line 96 (LLM router section) clarifying that `/llm/translate` is per-line stateless and never enters the audio pipeline.
3. `git diff` should be CLAUDE.md-only.

---

### 50. Cluster gate settings (`cluster_min_segments`, `cluster_min_duration_ms`) undocumented

**Status:** Partial 2026-04-26 — docs slice done. Added a Non-obvious-patterns bullet to `CLAUDE.md` describing both fields, the `cluster_passes_gate` shared helper, the defaults, and the rationale (suppress noise/cross-talk from flooding the Candidate Speakers panel). UI knob in Settings → Audio still pending; brainstorming required first per loop rules (UI Type).

ISS-046 (resolved) added two new `AudioSettings` fields with non-trivial behavior:

- `cluster_min_segments: u32` (default **3**)
- `cluster_min_duration_ms: u32` (default **8000**)

They AND-gate provisional speaker creation in both `process_clip_with_clustering` and `process_clip_production` via the shared `cluster_passes_gate` helper. Defaults were chosen to suppress noise/cross-talk/podcast-cameo blips from flooding the People → Candidate Speakers panel.

Neither knob is mentioned in:
- `CLAUDE.md` (Audio & inference pipeline section, or Non-obvious patterns)
- `backend/actio-core/src/engine/AGENTS.md` (if it exists; otherwise the nearest engine-level AGENTS.md)
- frontend Settings UI — there's no slider/numeric input to expose them

The frontend gap is **explicitly out-of-scope** for #46 (called out as "Out of scope: Frontend Settings UI to expose the two knobs"), so file the UI surfacing as part of this issue too. Operators who want to tune the gate either edit `~/.config/Actio/settings.json` manually or hit `PATCH /settings` directly — that's acceptable for power users but means the loop never gets feedback from non-developer users.

**Severity:** Low · **Platform:** All · **Type:** docs + ui · **Scope:** small (docs) + small (UI knob — two number inputs in Settings → Audio)

**Acceptance:**
1. Mention both fields in `CLAUDE.md` Non-obvious patterns (one bullet) so they're grep-able.
2. Add a sub-section in Settings → Audio with two clamped number inputs bound to `audio.cluster_min_segments` (range 1–50) and `audio.cluster_min_duration_ms` (range 0–600000, displayed as seconds for ergonomics). Persist via `PATCH /settings`.
3. New keys land in both `en.ts` and `zh-CN.ts` (parity test).

The docs-only slice is trivially safe to ship first; the UI follow-up needs `superpowers:brainstorming` per loop rules (it's UI Type) before code.

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
| 44 | Streaming + batch pipelines mutually exclusive | High | All | Open |
| 50 | Cluster gate settings — UI knob still pending | Low | All | Partial — docs landed; UI follow-up open |
| 51 | `@tauri-apps/api` mixed imports defeat code-splitting | Low | All | Open |
