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

### 59. `applyPendingResolutions` has no regression test — "identifying forever" fix is unprotected

**Status:** Resolved 2026-04-26 — exported `applyPendingResolutions` plus three `__*ForTest` helpers (push, reset, count) and added 8 unit tests in `use-voice-store.resolutions.test.ts` covering: empty buffer no-op (referential identity preserved), midpoint-in-window match, no-clobber on already-resolved lines, out-of-window resolution stays buffered, single-pass-each across multiple matching lines, partial drain (only consumed entries are removed), null speaker_id (Unknown case), and the single-resolution-per-line `break` semantics. 196/196 frontend tests pass; 188 → 196 (+8).


`frontend/src/store/use-voice-store.ts:372-398` implements a non-trivial speaker-resolution buffering algorithm: when a `speaker_resolved` event arrives for a transcript line that hasn't finalized yet (or that finalizes later out of order), the resolution is parked in a module-level `pendingResolutions` array and replayed against future-finalizing lines whose midpoint falls within the resolution's `[start_ms, end_ms]` window.

CLAUDE.md (line 105) explicitly calls this out as the **fix for the "identifying forever" bug on short utterances**:

> Contains a module-level `pendingResolutions` buffer that replays `speaker_resolved` events against lines that finalize **after** the event arrives — fixes "identifying forever" on short utterances.

But `frontend/src/store/__tests__/use-voice-store.test.ts` covers `pruneSegments`, `isMeaningfulFinal`, and `looksLikeTargetLang` — **none** of the four describe-blocks touches `applyPendingResolutions`, the `pendingResolutions` array, or the speaker-resolved → transcript merge flow. A grep for `pendingResolutions`, `applyPendingResolutions`, `speaker_resolved`, or `identifying` across the test directory returns zero hits.

That means a refactor that broke this fix would land silently — typecheck and existing tests would all stay green while the "identifying forever" UX regression returns. The function has subtle behaviours that make this real:

- Mid-point-in-window matching (line 379–383): change the mid formula or the inclusive bound and a class of resolutions stop applying.
- Single-resolution-per-line semantics (`break` at line 387): if relaxed, multiple resolutions could compete and the last one wins non-deterministically.
- Drain-after-apply with index compaction (lines 390–396): a refactor that drops the compaction step would leak applied resolutions into future calls.
- The "skip already-resolved lines" check (line 378) — without it, a resolution would clobber an already-attributed line with whatever happens to fit its window.

**Severity:** Low · **Platform:** All · **Type:** test (regression-protection gap on a flagged-critical fix) · **Scope:** small — `applyPendingResolutions` is exported-pure-function-shaped (it operates on its argument + the module-level array; no DI needed beyond resetting the array between cases)

**Acceptance:**
1. New unit tests in `use-voice-store.test.ts` (or a dedicated `use-voice-store.resolutions.test.ts`) covering at minimum:
   - Resolution arrives **before** the matching line finalizes → next finalize-with-applyPendingResolutions stamps the speaker.
   - Resolution arrives **after** the line finalizes → buffer holds it; applies on the next line whose midpoint falls in window.
   - Already-resolved lines are not clobbered.
   - Resolution outside any line's mid-window stays buffered until something matches (or never is).
   - Multiple resolutions in the buffer are each consumed exactly once.
2. Tests reset `pendingResolutions` between cases (the array is module-state). Either re-import the module per test, or expose a `__resetPendingResolutions()` test hook. The existing `use-voice-store.test.ts` setup pattern can be a model.
3. Existing voice-store tests stay green.

The `applyPendingResolutions` function is currently file-private (no `export`). Either export it, or test through `handleTranscriptMessage` / `handleSpeakerResolvedMessage` (the call sites) — the latter is closer to integration but exercises the same logic.

---

### 58. Notifications preference is half-built — toggle persists but nothing fires alerts

`frontend/src/components/settings/PreferencesSection.tsx` exposes a "Notifications — Show alerts for new reminders" toggle bound to `preferences.notifications` (`use-store.ts:71`). The preference round-trips through localStorage and the i18n strings exist in both en/zh-CN. But:

- **Nothing reads `preferences.notifications`.** Grep across `frontend/src` finds no code that branches on the value.
- **No code calls `new Notification(...)` or the Web Notifications API.**
- **`backend/src-tauri/src/main.rs:?` initializes `tauri_plugin_notification`** and the `notification:default` permission is granted in `capabilities/default.json`, but no Rust code ever invokes the plugin (no `Notification::new`, no `notify`, no path in `api/` that emits an OS-level notification).
- **`@tauri-apps/plugin-notification` is in `package.json` dependencies** but has zero static or dynamic imports anywhere in `frontend/src`.

A user toggling "Show alerts for new reminders" gets nothing — the only effect is the bool flipping in localStorage. That's worse than not having the toggle: it makes a promise the app silently breaks.

Two directions to resolve:

**A. Build the feature.** Wire `Notification` (web API for browser dev mode) or `@tauri-apps/plugin-notification` (desktop) to fire on:
- New high-confidence reminder arriving on the Board
- Optionally: dictation-success paste, new candidate-speaker arrival, etc.

This is the productive direction (the toggle was clearly added with a feature in mind) but is medium-large scope: needs brainstorming on which events warrant a notification, throttling, focus-aware suppression (don't notify if the app is foregrounded), permission-prompt UX, and the per-platform plumbing. Plus tests.

**B. Remove the dead surface.** Drop the toggle from PreferencesSection, drop `notifications` from the Preferences type and default, remove the Cargo plugin registration + capability + frontend npm dep. Shrinks binary surface and removes the broken promise. ~6 files, mechanical.

Direction A is the more product-aligned choice if anyone is planning to ship notifications soon; B is the right call if no one is. Either way leaving it in this state is a bug.

**Severity:** Medium · **Platform:** All · **Type:** ui (broken promise) + feature (path A) or refactor (path B) · **Scope:** medium for A, small for B

**Acceptance:**
1. Decide A or B (NEEDS-REVIEW — this is directional).
2. After implementation: toggling the setting either produces a visible behavior change (A) or the toggle no longer exists (B).
3. No path through the codebase reads `preferences.notifications` without acting on it.
4. (If B) `pnpm` and `cargo` build size + permission surface drop. Capture before/after sizes in the commit.

---

### 57. Live transcript auto-scroll yanks the user back down while they're reading

**Status:** Resolved 2026-04-26 — added a `wasAtBottomRef` + `onScroll` handler to `LiveTab.tsx`. The auto-scroll effect now runs only when the user was within `FOLLOW_THRESHOLD_PX` (64 px) of the bottom **before** the new content arrived. Three new vitest cases pin: at-bottom auto-scrolls; reading-mode does not; resuming-after-read re-engages follow. 188/188 frontend tests pass.


`frontend/src/components/LiveTab.tsx:70–74`:

```ts
useEffect(() => {
  if (transcriptRef.current) {
    transcriptRef.current.scrollTop = transcriptRef.current.scrollHeight;
  }
}, [currentSession?.lines, currentSession?.pendingPartial]);
```

This unconditionally jumps to the bottom every time `lines` or `pendingPartial` changes. Two problems:

1. **`pendingPartial` updates many times per second during active speech** (each ASR partial fires a store mutation). Combined with the React reference-equality check on the dep array, this effect runs roughly at the partial cadence. Every time it does, `scrollTop = scrollHeight` is applied.
2. **No "user is reading" guard.** If the user scrolls up mid-meeting to revisit an earlier point, the next partial yanks them back to the bottom. They can't read the past five minutes without pausing the session.

The standard "follow when at bottom, freeze when reading" pattern: capture whether `scrollTop + clientHeight` is within a small threshold of `scrollHeight` (say 64 px) **before** the new content lands; only re-apply auto-scroll if the user was already there.

```ts
const wasAtBottom = useRef(true);
const onScroll = (e: React.UIEvent<HTMLDivElement>) => {
  const el = e.currentTarget;
  wasAtBottom.current = el.scrollHeight - el.scrollTop - el.clientHeight < 64;
};
useEffect(() => {
  if (wasAtBottom.current && transcriptRef.current) {
    transcriptRef.current.scrollTop = transcriptRef.current.scrollHeight;
  }
}, [currentSession?.lines, currentSession?.pendingPartial]);
```

Wire `onScroll` to the `<main>` element. The check runs before the imperative scroll, so manual scroll-up freezes the follow until the user comes back near the bottom.

Bonus: a small "Jump to live" button could appear when `wasAtBottom` is false, mimicking Slack/Discord. Out of scope for the minimum fix.

**Severity:** Medium · **Platform:** All · **Type:** ui (bug-shaped UX gap) · **Scope:** small (one ref, one onScroll handler, one conditional in the existing effect)

**Acceptance:**
1. With the user at the bottom of the transcript, new lines/partials still auto-scroll.
2. With the user scrolled up by more than ~64 px, new lines/partials do **not** scroll the view; the read position is preserved.
3. Once the user manually scrolls back to within 64 px of the bottom, auto-scroll resumes.
4. Existing tests still pass; ideally add a vitest using `Object.defineProperty` to mock `scrollHeight`/`scrollTop`/`clientHeight` and assert the conditional.

---

### 56. Doc-comment drift: `clip_retention_days` is not actually replaced by `audio_retention_days`

**Status:** Resolved 2026-04-26 — both `clip_retention_days` and `audio_retention_days` doc-comments rewritten to call out the coexistence and the Plan Task 17 retirement reference. `cargo check` clean; no behaviour change.


`backend/actio-core/src/engine/app_settings.rs` documents the relationship between two retention settings:

```rust
/// Per-clip WAV files older than this many days are swept by the
/// background cleanup task. Replaces the per-failed-segment retention
/// path that used `clip_retention_days`.
#[serde(default = "default_audio_retention_days")]
pub audio_retention_days: u32,
```

But `lib.rs:317–321` immediately contradicts this:

```rust
//   1. Nested clip-dir cleanup — sweeps <clips_dir>/<session>/<clip>/
//      every hour, removes whole clip directories older than
//      `audio.audio_retention_days` (default 14). Distinct from the
//      legacy flat-dir voiceprint candidate sweep above; both run
//      until Plan Task 17 retires the legacy infra.
```

Both retention paths are alive concurrently:

- `clip_retention_days` (default 3) → `clip_storage::start_cleanup_task` → flat-dir sweep at `<clips_dir>/` for legacy voiceprint candidates (`lib.rs:194-195`).
- `audio_retention_days` (default 14) → `clip_storage::start_clip_dir_cleanup_task` → nested-dir sweep at `<clips_dir>/<session>/<clip>/` for batch-pipeline clips (`lib.rs:339-342`).

The "Replaces" word in the doc comment misleads contributors into thinking `clip_retention_days` is dead and pruning it would be safe — when in fact `lib.rs:194` still reads it on every boot to schedule the legacy sweep.

**Severity:** Low · **Platform:** All · **Type:** docs · **Scope:** small (2-line comment edit in `app_settings.rs`; optionally also a one-line note on `clip_retention_days`'s doc-comment that it's a legacy-data-path knob slated for removal in Plan Task 17)

**Acceptance:**
1. The `audio_retention_days` doc-comment is rewritten to say "Sweeps the nested per-clip directory tree from the batch pipeline. Coexists with the legacy `clip_retention_days` sweep until Plan Task 17 retires the legacy infra" (or similar — the key change is replacing "Replaces" with "Coexists with").
2. Optionally extend `clip_retention_days`'s doc-comment with the same coexistence note + the Plan Task 17 reference, so contributors reading either field find the same story.
3. `git diff` is `app_settings.rs`-only; no behavior change.

---

### 55. Vite bundle warning persists — no `manualChunks` for vendor deps

**Status:** Resolved 2026-04-26 — added `build.rollupOptions.output.manualChunks` to `vite.config.ts` with `vendor-react` and `vendor-motion` entries. Main bundle dropped from **542.11 kB → 399.88 kB (−26 %)**, the chunk-size warning is gone, and the new lazy chunks complement the `core-*.js`/`event-*.js`/`window-*.js` splits from #51. 185/185 tests pass; tsc clean.


`pnpm build` continues to log:

```
(!) Some chunks are larger than 500 kB after minification.
    Consider:
    - Use build.rollupOptions.output.manualChunks to improve chunking
```

After the #51 work that split `@tauri-apps/api/{core,event,window}` into lazy chunks (-16.5 kB), the main chunk still sits at **542.11 kB** because the heavy SPA-time-zero deps land in it: `react` + `react-dom` (≈140 kB), `framer-motion` (≈100 kB), `zustand`, the additional `@tauri-apps/plugin-*` packages used by the keyboard/global-shortcut/autostart paths, plus the app code.

`frontend/vite.config.ts` has no `build.rollupOptions.output.manualChunks` config — Rollup's automatic chunking puts everything reachable from the entry point into the main chunk. A small explicit split would give us:

- `vendor-react` chunk: `react`, `react-dom`, `react/jsx-runtime` (~140 kB)
- `vendor-motion` chunk: `framer-motion` (~100 kB)
- everything else stays in the entry chunk

That alone drops the main below the 500 kB warning threshold and lets the browser cache the vendor chunks across deploys (the entry chunk's hash flips on every app code change; vendor hashes change only when deps bump). Net cold-start TTI is roughly the same (the vendor chunks still have to load), but warm starts and HTTP/2 multiplexing both win.

**Severity:** Low · **Platform:** All · **Type:** perf · **Scope:** small (one config block in `vite.config.ts`)

**Acceptance:**
1. `vite.config.ts` gains `build: { rollupOptions: { output: { manualChunks: { ... } } } }` with at minimum a `react` and `framer-motion` entry.
2. `pnpm build` — main chunk drops below 500 kB and the `(!) Some chunks are larger than 500 kB` warning disappears.
3. Three new `vendor-*.js` chunks emerge in `dist/assets/`; their gzipped sizes are reported in the commit message.
4. `pnpm test` (185/185) and `pnpm tsc --noEmit` stay green — no source code touched.

Verify by capturing before/after `pnpm build` output in the commit message. The change is config-only; the only risk is if `manualChunks` keys collide with already-emitted chunks (the existing `core-*.js`, `event-*.js`, `window-*.js` from #51 must keep their split — `manualChunks` runs after the dynamic-import logic, so this is safe but worth eyeballing).

---

### 54. Needs-Review dismiss has no undo affordance — accidental clicks lose information

**Status:** Resolved 2026-04-26 — extended the existing feedback-toast surface with an optional `action: { labelKey, onAction }` field. Actionable toasts get a 5 s lifetime (vs. 2.2 s for plain ones). `NeedsReviewView` now passes `{ labelKey: 'feedback.undo', onAction: () => restoreReminder(id) }` on Dismiss. New i18n key `feedback.undo` (en + zh-CN), CSS for the action button, and 2 vitest cases pin the flow (Dismiss → Undo restores; Confirm shows no Undo). 185/185 tests pass.

The "brainstorming pause" the issue called out turned out to be unnecessary — the existing toast component had a clean extension point (the `feedback` object on `UIState`), so the change was pattern-match (one new optional field, one new button, one timer-lifetime conditional) rather than novel UX.

`frontend/src/components/NeedsReviewView.tsx:44-47` archives the reminder (`status='archived'`) on Dismiss with no confirmation and no undo. The Needs-Review queue holds medium-confidence auto-extracted items the user is **reviewing for accuracy** — they're already uncertain candidates, so an accidental Dismiss click loses information that's hard to recover. The only path back is opening the Archive view and unarchiving, which most users won't think to do.

This is a worse UX trap than #43's `window.confirm()` problem because:

- Confirm/Dismiss live next to each other on every card (the buttons are 12 px apart in the rendered layout). Misclicks happen.
- The card slides off-screen the moment Dismiss is clicked — even a user who realizes their mistake immediately has no visual anchor to "undo from."
- The existing `setFeedback('feedback.reminderDismissed', 'neutral')` toast (line 46) is the natural surface for an undo, but it currently shows just a label, not an action.

The standard pattern for "destructive but reversible" actions is an undo toast: dismiss the item, show a 5–8 s toast with an "Undo" button that calls `restoreReminder(id)`. Gmail, Linear, GitHub PRs, Slack channel-leave all use this shape.

**Out of scope for this issue (other findings from the same workflow trace, worth tracking separately if they get traction):**
- No keyboard navigation between cards. `card_up`/`card_down`/`card_expand`/`card_archive` shortcuts are defined in `KeyboardSettings.tsx` but aren't wired into `NeedsReviewView`.
- No "show source context" affordance. The backend's `GET /reminders/:id/trace` endpoint (CLAUDE.md line 92) supports this but the card doesn't link to the source clip / window.
- No bulk-action support (Confirm-all / Dismiss-all) — long sessions can produce 20+ pending items.
- No loading state on Confirm/Dismiss buttons; double-clicks during a slow PATCH could fire twice.

**Severity:** Medium · **Platform:** All · **Type:** ui (bug-shaped UX gap) · **Scope:** small (extend `setFeedback` to support an action button, or add a dedicated undo-toast variant)

**Acceptance:**
1. After Dismiss on a Needs-Review card, the existing toast surface ("feedback.reminderDismissed") gains an "Undo" button.
2. Clicking Undo within 5–8 s calls `restoreReminder(id)` (the PATCH that flips `status` back to its prior value — `'open'` for medium-confidence items that were going to land on the Board, or `'pending'` if the user prefers staying in the queue).
3. After the grace period, the toast auto-dismisses and the action becomes durable.
4. New i18n keys land in both `en.ts` and `zh-CN.ts` (`feedback.undo`, `feedback.reminderDismissedWithUndo` if needed).
5. Vitest pins the flow: Dismiss → toast appears → Undo click → reminder reappears in `pendingReminders()`.

Brainstorming pause is appropriate before code on this one — the toast component shape (single-action vs. two-action, lifecycle on tab switch, multiple stacked dismisses) is design-shaped, not pattern-match.

---

### 53. `ConfirmDialog` lacks focus trap + autoFocuses the destructive action

**Status:** Resolved 2026-04-26 — Tab/Shift-Tab now cycles within the modal; destructive tones autoFocus the cancel button (and the global Enter handler routes to `onCancel` when destructive); focus is captured at open via `document.activeElement` and restored on close. 6 new unit tests in `ConfirmDialog.test.tsx` pin all four behaviours; full suite 183/183.


`frontend/src/components/ConfirmDialog.tsx` (added in #43) implements a promise-based modal that's now used for three destructive flows (dismiss candidate speaker, switch embedding model, delete model). Two a11y gaps surfaced on review:

**1. No focus trap.** When the dialog opens, focus moves to the confirm button (via `autoFocus`), but Tab/Shift-Tab can leave the modal and focus elements behind the backdrop — even though the rest of the page is meant to be inert (`aria-modal="true"`). Keyboard-only users can land on a button, link, or input that's visually obscured by the backdrop and click "blind." Standard pattern is to either:

- Set `inert` on the rest of `document.body` while open (or a sibling root); or
- Implement roving Tab — on `Tab` inside the modal, cycle to first focusable; on `Shift-Tab` from first, cycle to last.

The component already manages a global `keydown` listener for `Escape`/`Enter`; adding Tab handling there is mechanical.

**2. `autoFocus` on the confirm button defaults to "destructive" for the destructive tone.** The component sets `autoFocus` on the confirm button regardless of `tone`. Combined with the existing `Enter`-handler that calls `onConfirm`, a user who hits Enter immediately after the dialog opens (e.g., from muscle memory after pressing the Dismiss/Delete row button) confirms the destructive action without ever seeing the prompt. This is the same UX trap `window.confirm()` had — the very thing #43 was supposed to fix.

GitHub's pattern (and most native OS dialogs): autoFocus **cancel** for destructive tones, autoFocus **confirm** only for non-destructive prompts. Concrete change: gate `autoFocus` on `tone !== 'destructive'`, and move `autoFocus` to the cancel button when destructive.

Bonus gap: when the modal closes, focus is not restored to the element that opened it (the row's Dismiss/Delete button). Standard a11y pattern is to capture `document.activeElement` on open and `.focus()` it back on close. Without that, keyboard users get dropped on `<body>` and have to retrace.

**Severity:** Low · **Platform:** All · **Type:** a11y · **Scope:** small (two clear changes plus the focus-restoration polish)

**Acceptance:**
1. Tabbing inside the open modal cycles between the two buttons; Shift-Tab from the first cycles to the last; focus never leaves the dialog.
2. When `tone === 'destructive'`, the cancel button receives initial focus (Enter on a freshly-opened destructive dialog calls `onCancel`, not `onConfirm`).
3. When the dialog closes, focus returns to the element that was focused before it opened (read from `document.activeElement` at open time).
4. Existing `CandidateSpeakersPanel.test.tsx` modal-flow test stays green; ideally extend it to assert the focus-trap + restoration behaviour.

---

### 52. Frontend hardcodes `http://127.0.0.1:3000` in 7 places, bypassing port-fallback

**Status:** Resolved 2026-04-26 — all 7 sites now go through `getApiUrl()` (or `getApiBaseUrl()` for the parallel-fetch refresh in `ModelSetup.tsx`). 177/177 tests pass; `pnpm build` clean of static/dynamic mixing warnings. Bundle size effectively unchanged (`backend-url.ts` was already universally imported elsewhere).


`frontend/src/api/backend-url.ts` exposes `getApiUrl(path)` and `getApiBaseUrl()` which probe ports 3000–3009 (`/health`) and respect the `VITE_ACTIO_API_BASE_URL` env var. Several files still hardcode `http://127.0.0.1:3000` directly, which silently fails when the backend lands on a fallback port (e.g. when 3000 is held by another process — exactly the scenario the comment at `useGlobalShortcuts.ts:245` calls out for the WS path).

Concrete sites (production code, not tests):

```
src/components/settings/AudioSettings.tsx:4    const API_BASE = 'http://127.0.0.1:3000';
src/components/settings/KeyboardSettings.tsx:5 const API_BASE = 'http://127.0.0.1:3000';
src/components/settings/LlmSettings.tsx:4      const API_BASE = 'http://127.0.0.1:3000';
src/components/settings/ModelSetup.tsx:53      const API_BASE = 'http://127.0.0.1:3000';
src/hooks/useGlobalShortcuts.ts:97             fetch('http://127.0.0.1:3000/settings')
src/i18n/index.ts:67                            fetch('http://127.0.0.1:3000/settings')
src/i18n/index.ts:101                           fetch('http://127.0.0.1:3000/settings', { method: 'PATCH', … })
```

The two files that already get this right (`api/actio-api.ts:13`, `components/NewReminderBar.tsx:9`) still hardcode `127.0.0.1:3000` as a fallback for `VITE_ACTIO_API_BASE_URL`, but they **don't** participate in port discovery — the env var path is acceptable for production-build hosts and the fallback only matters when the backend is on the default port (which is the common case). Those two are out of scope; the issue is the seven sites above that ignore both the env var and the discovery probe.

**Severity:** Low · **Platform:** All · **Type:** refactor · **Scope:** small (7 sites, mechanical conversion to `await getApiUrl(...)`)

**Acceptance:**
1. Each of the seven sites switches to `getApiUrl(path)` (or `getApiBaseUrl()` followed by manual concat where the call shape needs it).
2. `pnpm tsc --noEmit` clean; `pnpm test` passes (177/177 currently).
3. Existing tests that mock `fetch` to recognize URLs by suffix (`path.endsWith('/settings')`, `path.includes('/candidate-speakers')`) keep passing — they don't pin the host.
4. No new dependency.

Note: the `i18n/index.ts:101` site is a `PATCH` inside a non-async setter; the conversion would need either an IIFE or a top-level async wrapper. That's the only one with mild structural cost; the others are already inside async functions.

---

### 51. `@tauri-apps/api` mixed static + dynamic imports defeat code-splitting

**Status:** Resolved 2026-04-26 — all four static-importers converted. Both build warnings gone; three new chunks emerged (`core-*.js` 2.44 kB, `event-*.js` 1.36 kB, `window-*.js` 13.91 kB) and the main bundle dropped from **555.36 kB → 538.88 kB** (−16.5 kB total across two ticks). 177/177 frontend tests pass.

**Vitest mock fix:** the second dynamic import of `@tauri-apps/api/event` was bypassing the `vi.mock` and hitting the real package (cause unclear, but reproducible). Workaround: cache each submodule's `import()` Promise at module scope inside the source file (`loadCore`, `loadEvent` helpers). Both useEffects await the same cached Promise, so vitest only resolves the module once and the mock applies consistently. Documented inline in `useGlobalShortcuts.ts`.

**StandbyTray** required pre-loading `getCurrentWindow()` into a `useRef` at mount so `handleDragStart` can still call `startDragging()` synchronously during `mousedown` (Tauri's native OS drag won't fire if the import races against the event).

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

**Status:** Resolved 2026-04-26 — docs slice landed earlier; UI slice landed this tick. Added two range sliders to Settings → Audio (`clusterMinSegments` 1–20, `clusterMinDurationMs` 0–60s with seconds rendering for ergonomics) directly below the existing voice-clustering threshold slider. New keys land in both `en.ts` and `zh-CN.ts` with hint copy explaining the AND-gate semantics. Brainstorming was skipped: the design is fully constrained by the existing 9+ slider siblings in `AudioSettings.tsx`, so it's pattern-match, not novel UI.

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
| 58 | Notifications toggle persists but never fires alerts | Medium | All | Open — directional (NEEDS-REVIEW) |
