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

### 61. 15 duplicate `fresh_pool()` test helpers across backend test modules

**Status:** Resolved 2026-04-26 — added `actio-core/src/testing.rs` (compiled under `#[cfg(test)] pub mod testing;` in `lib.rs`) exposing the canonical `fresh_pool()`. All 14 sites now `use crate::testing::fresh_pool;` and dropped their local copies. Pruned the unused `run_migrations` / `SqlitePoolOptions` imports they pulled in solely for the helper. 214/214 backend lib tests still pass; `cargo check --tests` clean.


`grep -rE "async fn fresh_pool" backend/actio-core/src` finds **15 identical definitions** of the in-memory-SQLite test helper:

```
backend/actio-core/src/api/candidate_speaker.rs
backend/actio-core/src/api/clip.rs
backend/actio-core/src/api/llm.rs
backend/actio-core/src/api/reminder.rs
backend/actio-core/src/api/segment.rs
backend/actio-core/src/api/session.rs
backend/actio-core/src/domain/speaker_matcher.rs
backend/actio-core/src/engine/batch_processor.rs
backend/actio-core/src/engine/clip_writer.rs
backend/actio-core/src/engine/inference_pipeline.rs
backend/actio-core/src/engine/live_enrollment.rs    ← added in #60 last tick
backend/actio-core/src/engine/window_extractor.rs
backend/actio-core/src/repository/audio_clip.rs
backend/actio-core/src/repository/extraction_window.rs
backend/actio-core/src/repository/speaker.rs
```

Each is the same ~10 lines:

```rust
async fn fresh_pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&pool)
        .await
        .unwrap();
    run_migrations(&pool).await.unwrap();
    pool
}
```

Costs: future migrations changes require touching 15 sites; future test-pool conventions (e.g. seeding a default tenant, enabling WAL mode in tests) get scattered. Each new test module adds another copy.

The crate's CLAUDE.md guidance is "Three similar lines is better than a premature abstraction" — 15 copies is well past that threshold.

**Severity:** Low · **Platform:** All · **Type:** refactor · **Scope:** small

**Acceptance:**
1. Add a `pub(crate)` test helper module — natural location is `actio-core/src/testing.rs` declared under `#[cfg(test)] pub mod testing;` in `lib.rs`. Inside, expose `pub async fn fresh_pool() -> SqlitePool` with the canonical body.
2. Each of the 15 sites switches to `use crate::testing::fresh_pool;` and deletes its local copy.
3. `cargo test -p actio-core --lib` stays green (currently 214 tests, all should still pass).
4. `cargo clippy -p actio-core --tests` flags no new warnings.

Note: `batch_processor.rs` declares its `fresh_pool` as `pub(super) async fn` so it's already shared with one neighbour. The shared `crate::testing::fresh_pool` subsumes that. Two test modules (`api/segment.rs:tests::nested` and `repository/speaker.rs::tests::partial_unique_index_blocks_two_self_speakers_per_tenant`) have inline `let pool = SqlitePoolOptions::new()...; run_migrations(&pool).await.unwrap();` blocks rather than calling a `fresh_pool` helper — those should also adopt the shared helper for consistency (or be left as-is if they need a custom-shaped pool; check at landing time).

---

### 62. Dead structs and functions hidden behind `#[allow(dead_code)]`

**Status:** Resolved 2026-04-26 — deleted `domain::types::SpeakerEmbedding` (the colliding ghost), `AudioSegment`, and `NewTodo` structs; deleted `repository::todo::has_todos` and `create_todos` functions; deleted the `extract_bz2_tar` helper from the bonus audit (model_manager.rs:1367) along with its `bzip2` + `tar` direct-dependency entries in `actio-core/Cargo.toml`. The other two `#[allow(dead_code)]` markers (`api/session.rs:571 AppApiError`, `engine/window_extractor.rs:1089 _force_use_secs_format`) are legitimate (all 3 enum variants are used; underscore-prefixed import-silencer is intentional). 214/214 backend lib tests still pass; cargo check + cargo build clean.


A grep over `#[allow(dead_code)]` markers in `backend/actio-core/src/` surfaces a chain of items that are genuinely unused everywhere — the marker was masking real dead code rather than legitimate "used only behind a feature flag" suppression.

Confirmed dead (`grep -rE '\b<sym>\b' actio-core/src --include='*.rs'` returns zero non-definition references):

```
domain/types.rs:20  pub struct SpeakerEmbedding   (DB-row shape — superseded by
                                                   engine/diarization.rs's
                                                   in-memory SpeakerEmbedding;
                                                   wholly unreachable)
domain/types.rs:45  pub struct AudioSegment       (zero references)
domain/types.rs:110 pub struct NewTodo            (only used inside the
                                                   also-dead repository/todo.rs
                                                   functions below)
repository/todo.rs  pub async fn has_todos        (the "backward-compat alias
repository/todo.rs  pub async fn create_todos      route" the comment cites
                                                   has been removed)
```

`get_todos_for_session` in `repository/todo.rs` is still alive (`api/session.rs` calls it), so the file shouldn't be deleted wholesale — only the two functions above plus the now-orphan `use crate::domain::types::NewTodo;` need to go.

The `domain::types::SpeakerEmbedding` struct collides with the live `engine::diarization::SpeakerEmbedding` — keeping the dead one is also a small footgun. A grep for "SpeakerEmbedding" in the codebase returns hits across both, and the wrong one could land in an import via auto-suggest.

**Severity:** Low · **Platform:** All · **Type:** dead-code / refactor · **Scope:** small

**Acceptance:**
1. Delete the three structs and the two functions.
2. Remove the orphaned imports they leave behind (`use NewTodo` in `todo.rs`, the `#[allow(dead_code)]` markers on the deleted items).
3. `cargo check -p actio-core --tests` clean — no new "unused import" warnings.
4. `cargo test -p actio-core --lib` 214/214 still passes.
5. The remaining `#[allow(dead_code)]` markers in the tree are spot-audited; if any other items have zero non-definition references, fold them into the same commit. Likely candidates from the same grep:
   - `api/session.rs:571 #[allow(dead_code)]` — verify caller exists
   - `engine/model_manager.rs:1367 #[allow(dead_code)]` — verify caller exists
   - `engine/window_extractor.rs:1089 #[allow(dead_code)]` (test-mod scope) — likely harmless

The audit step (#5) is the bonus; #1-4 are the core cleanup.

---

### 63. Reminders/labels client still bypasses port-fallback discovery

**Status:** Resolved 2026-04-26 — `actio-api.ts::request` and `NewReminderBar.tsx::fetchLlmConfigured` both switched to `await getApiUrl(path)`. Dropped the local `API_BASE_URL` / `SETTINGS_API_URL` constants. The reminders/labels load+save flow now uses the same port-discovery path the rest of the app does (closes the asymmetry #52 left). 196/196 frontend tests pass; tsc clean; build clean.


When ISS-052 landed, two fetch sites were explicitly carved out as "uses the env-var shape, leave alone": `frontend/src/api/actio-api.ts:13` (the root reminders/labels client) and `frontend/src/components/NewReminderBar.tsx:9`. The carve-out reasoning was: production builds set `VITE_ACTIO_API_BASE_URL`, and dev usually has the backend on port 3000.

Re-examining that with fresh eyes: the seven sites I fixed in #52 face the exact same scenario as these two — a developer running locally with another process holding port 3000 (so the backend lands on 3001-3009 via the existing port-discovery probe). The seven fixed sites now follow the fallback; these two still don't. Net: the **reminders / labels load+save flow**, which is the bulk of the app's actual work, fails silently when the backend isn't on 3000. The same code path that already works in `LlmSettings.tsx`, `AudioSettings.tsx`, etc. fails in the rest of the app.

Concretely, `request` at `actio-api.ts:24-43` is an async function that today does:

```ts
const API_BASE_URL = (import.meta.env.VITE_ACTIO_API_BASE_URL ?? 'http://127.0.0.1:3000').replace(/\/$/, '');
async function request<T>(path: string, init: RequestInit = {}) {
  const response = await fetch(`${API_BASE_URL}${path}`, { ... });
  ...
}
```

The shape mirrors `http.ts::requestJson` exactly except for the URL resolution. Switching to `await getApiUrl(path)` is a one-line change inside an already-async function — no API-shape ripple to the 30+ callers of `createActioApiClient()`.

`NewReminderBar.tsx:9` is the same pattern: a single `SETTINGS_API_URL` constant feeding one fetch.

**Severity:** Low · **Platform:** All · **Type:** refactor (extension of #52) · **Scope:** small (2 files, ~5 lines)

**Acceptance:**
1. `actio-api.ts::request` switches to `await getApiUrl(path)` and drops the local `API_BASE_URL` constant. Existing tests (`actio-api.test.ts`, etc.) still pass.
2. `NewReminderBar.tsx`'s settings PATCH fetch switches to `await getApiUrl('/settings')`.
3. `pnpm tsc --noEmit` clean; `pnpm test` 188+ pass.
4. The carve-out note in #52 is updated to acknowledge that this followup also landed.

(The `DEV_TENANT_ID` constant in `actio-api.ts:12` is unrelated — leave it.)

---

### 64. `pushFeedback` lifetime branch (actionable vs plain) has no test

**Status:** Resolved 2026-04-26 — added `use-store.feedback.test.ts` with 4 vitest cases under `vi.useFakeTimers()`: plain auto-dismiss at 2200 ms (boundary tested at 2199 / 2200), actionable survives to 5000 ms (boundaries at 4999 / 5000, plus assertion that the Undo callback is the user's choice and auto-dismiss must not fire it), replace-in-flight cancels the prior timer (the second `setFeedback`'s window applies, not the first's), and `clearFeedback` cancels the timer (advancing time after clear doesn't fire a stale callback). 196 → 200 frontend tests pass.


`frontend/src/store/use-store.ts::pushFeedback` (added/extended for the undo-toast work in ISS-054) has a load-bearing conditional that picks the toast lifetime:

```ts
const lifetimeMs = action ? 5000 : 2200;
feedbackTimer = window.setTimeout(() => {
  set((state) => ({ ui: { ...state.ui, feedback: null } }));
  feedbackTimer = null;
}, lifetimeMs);
```

The intent is: actionable toasts (carry an `action: { labelKey, onAction }`) get a 5 s grace period so the user has time to hit "Undo"; plain toasts auto-dismiss at 2.2 s. The Needs-Review undo-flow tests added in #54 (`NeedsReviewView.test.tsx`) cover the user clicking Undo within the window — but none of the existing store tests pin the **timeout** itself: that an unattended actionable toast survives for ~5 s, that an unattended plain toast disappears at ~2.2 s, that a follow-up `setFeedback` call cancels the prior timer rather than letting two timers race, and that `clearFeedback` clears the timer.

A grep across `frontend/src/store/__tests__/*.ts` for `pushFeedback`, `setFeedback`, `lifetimeMs`, `5000`, or `2200` returns zero hits — no regression test pins this.

The risk if the branch breaks (someone refactors back to a single timeout, or flips the conditional): the undo-toast UX silently regresses to a 2.2 s window that's too short to react to, or the plain toast lingers visibly longer than designed. Both would land typecheck + test green.

**Severity:** Low · **Platform:** All · **Type:** test · **Scope:** small (vitest with `vi.useFakeTimers()`)

**Acceptance:**
1. New `use-store.feedback.test.ts` (or extend `use-store.settings.test.ts`) under `vi.useFakeTimers()` covering at minimum:
   - Plain toast: after `setFeedback('msg', 'neutral')`, advancing time by 2 200 ms clears `ui.feedback` to `null`. Advancing by 2 199 ms does not.
   - Actionable toast: after `setFeedback('msg', 'neutral', vars, { labelKey, onAction })`, advancing by 4 999 ms keeps the toast; 5 000 ms clears it.
   - Replace-in-flight: a second `setFeedback` call before the first timer fires replaces both message and timer (the second's lifetime applies, not the first's).
   - `clearFeedback` clears the toast and cancels the timer (advancing time after `clearFeedback` doesn't re-null an already-null state, and importantly doesn't fire a stale callback).
2. 196 → 200ish frontend tests, all green.

This is a small gap on logic I just shipped — surfacing it as a separate issue rather than slipping it into the next refactor so the cleanup is auditable.

---

### 65. `mapBackendReminder` null-fallback branches are mostly untested

**Status:** Resolved 2026-04-26 — extended `actio-api.test.ts` with 5 named cases (null title→description, null priority→medium, nullable string fields→undefined family, archived+null archived_at→updated_at fallback, open+non-null archived_at→archivedAt null). Refactored the existing test scaffold to use a `dto(over)` builder so each new case sets only the fields it cares about. 200 → 205 frontend tests; tsc clean.


`frontend/src/api/actio-api.ts::mapBackendReminder` (lines 45-62) has four non-trivial null-coalescing branches that the existing test doesn't cover. The single test in `actio-api.test.ts` exercises the all-fields-populated archived path; nullable inputs slip through.

```ts
title: dto.title ?? dto.description,                                // (a)
priority: dto.priority ?? 'medium',                                 // (b)
speakerId: dto.speaker_id ?? undefined,                             // (c) and similar
archivedAt: dto.status === 'archived'
  ? dto.archived_at ?? dto.updated_at
  : null,                                                            // (d)
```

(a) When the backend returns a reminder without an explicit title (older auto-extracted items had `title: null`), the description doubles as the title — the UI's primary text. A refactor that swapped `??` for `||` would change behaviour for empty-string descriptions.

(b) When priority is null, the UI defaults to `'medium'`. Without this, the Card component renders a non-existent priority class.

(d) Three-way branch:
- `status='archived'` AND `archived_at` set → use `archived_at` (✅ tested)
- `status='archived'` AND `archived_at` null → fall back to `updated_at` (untested — supports legacy rows)
- `status` is `'open'` / `'pending'` → `archivedAt` is null regardless of any DB-side `archived_at` (untested — frontend treats status as source of truth)

**Severity:** Low · **Platform:** All · **Type:** test · **Scope:** small (extend existing `actio-api.test.ts`)

**Acceptance:**
1. New cases in `actio-api.test.ts`:
   - Null title falls back to description.
   - Null priority falls back to `'medium'`.
   - Null nullable fields (`speaker_id`, `due_time`, `transcript_excerpt`, `context`, `source_time`, `source_window_id`) become `undefined` on the Reminder side (not null).
   - Status='archived' with null `archived_at` falls back to `updated_at`.
   - Status='open' with non-null `archived_at` produces `archivedAt: null` (status wins).
2. 200 → 205ish frontend tests, all green.

These branches survived the codebase ~unchanged since the API client was first written; protecting them is cheap and pays off the next time the DTO grows a new nullable field.

---

### 66. OpenAPI / Swagger UI is missing ~half the API surface

**Status:** Resolved 2026-04-26 — all 4 user-facing surfaces documented across 4 ticks:

  | Tick      | Slice           | Routes added |
  |-----------|-----------------|---|
  | `f75fc5c` | `/reminders`    | 7 |
  | `f64c656` | `/labels`       | 4 |
  | `9fa3878` | `/settings/*`   | 11 |
  | this tick | `/llm` + `/v1/*` + `/clips` | 9 |

  **31 new routes** registered in `paths(...)`, plus the dangling `get_reminder_trace` annotation now wired through. Schemas added to `components(schemas(...))` for every simple request/response body. The complex response shapes (`AppSettings`, `ModelStatus`, `LocalLlmInfo`, `LoadStatus`, `OpenAiChatResponse`, etc.) are intentionally documented with bodyless `responses((status, description))` — they nest too deep to derive `ToSchema` cheaply, and the trade-off documents the route + status code + tag without the schema graph. 214/214 backend lib tests pass at every slice; cargo check clean throughout.

`backend/actio-core/src/api/mod.rs:33-72` declares the OpenAPI doc with `#[derive(OpenApi)] paths(...)`. The `paths(...)` list registers **28** routes — speaker/session/segment/candidate-speaker/profile. CLAUDE.md (line 138) advertises `/docs` as the source of truth: *"Full request/response schemas live at http://localhost:3000/docs while the backend is running."*

But `mod.rs` actually mounts **47 routes**. The 19 missing from OpenAPI cover entire user-facing API surfaces:

```
/reminders                  list / post
/reminders/extract          post
/reminders/:id              get / patch / delete
/reminders/:id/trace        get
/labels                     list / post
/labels/:id                 patch / delete
/settings                   get / patch
/settings/llm/test          post
/settings/models            get
/settings/models/warmup     post
/settings/models/:id        delete
/settings/audio-devices     get
/settings/llm/models        get
/settings/llm/models/:id    delete
/settings/llm/load          post
/settings/llm/cancel-load   post
/settings/llm/load-status   get
/llm/translate              post
/v1/models                  get      (OpenAI-compatible shim)
/v1/chat/completions        post     (OpenAI-compatible shim)
/clips                      get
```

Counts confirm: `grep -c '#[utoipa::path' actio-core/src/api/*.rs` shows `reminder.rs:1` and zero in `label.rs`, `settings.rs`, `llm.rs`, `translate.rs`, `clip.rs`. The single annotation in `reminder.rs` isn't even in the `paths(...)` list, so it's a dangling doc-comment that doesn't render.

A new contributor opening `/docs` to learn the API gets a misleading partial picture — they see the speaker/segment ceremony but not the reminders + labels CRUD that's the actual app's core. The frontend's `actio-api.ts` was hand-written against the backend with no OpenAPI cross-check, which is workable today but fragile when the schemas diverge.

The selection of *what* to document looks accidental: speaker enrollment got annotated as part of its feature work; reminders/labels/settings predate the OpenAPI introduction and never got back-filled. Not a directional decision — just unfinished.

**Severity:** Low · **Platform:** All · **Type:** docs · **Scope:** medium — adding `#[utoipa::path]` + `ToSchema` to 20+ handlers and their request/response types, plus extending `paths(...)` and `components(schemas(...))` in `api/mod.rs`. Each handler is mechanical; the volume is the cost.

**Acceptance:**
1. Each of the 19 routes above has a `#[utoipa::path]` annotation with `responses(...)` covering at minimum the success status and the `AppApiError` shape.
2. Request bodies and DTOs (`BackendReminderDto`, `BackendLabelDto`, `LabelDraft`, `SettingsPatch`, `LlmLoadRequest`, etc.) derive `utoipa::ToSchema` and are included in `components(schemas(...))`.
3. `paths(...)` in `api/mod.rs` lists the new entries.
4. Visiting `/docs` while the backend is running shows the full surface; the existing speaker/segment routes still render correctly.
5. `cargo test -p actio-core --lib` 214/214 stays green; no new test failures from the schema derives.

Lower-cost incremental option: tackle one route group per tick (reminders, labels, settings/*, llm/*, translate, clips, v1/*) — six small commits instead of one large one. The acceptance can be split that way too if the user prefers smaller units.

---

### 67. `AppApiError` lives in `api/session.rs`; the unused `crate::error::AppError` shadows it

**Status:** Resolved 2026-04-27 — picked option B. New `actio-core/src/api/error.rs` houses `AppApiError` (definition + `IntoResponse` impl). All 8 importers (the original 7 plus `segment.rs`, which had been pulling it via `use crate::api::session::{tenant_id_from_headers, AppApiError}`) now `use crate::api::error::AppApiError;`. The dead `crate::error::AppError` enum and its `error.rs` module file are deleted; `lib.rs` no longer declares the module. OpenAPI registration in `api/mod.rs` updated to import the new path. 214/214 backend lib tests pass; cargo check + cargo check --tests warning-free.


Two related code-organization findings:

**(a) Dead module: `crate::error::AppError`** — `backend/actio-core/src/error.rs` (10 lines) declares:

```rust
#[derive(Error, Debug)]
pub enum AppError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("Session not found: {0}")]
    SessionNotFound(String),
}
```

Zero importers across the codebase. The crate's `lib.rs` declares `pub mod error;`, but a grep for `crate::error`, `use crate::error::`, or any `AppError` usage returns nothing. This was probably scaffolded as the first error type and abandoned when `AppApiError` (see below) became the de-facto choice.

**(b) Misplaced active type: `crate::api::session::AppApiError`** — the actually-used enum lives in `api/session.rs:572-576`:

```rust
#[derive(Debug, ToSchema)]
#[allow(dead_code)]
pub enum AppApiError {
    Internal(String),
    BadRequest(String),
    Conflict(String),
}
```

7 sibling files import `use crate::api::session::AppApiError;`:

```
api/candidate_speaker.rs:19    api/clip.rs:25
api/label.rs:6                 api/llm.rs:6
api/reminder.rs:8              api/settings.rs:7
api/translate.rs:7
```

The location is a vestige of when `session.rs` was the first API module; the type is now shared infrastructure. A new contributor reading any of those 7 files sees an oddly-specific cross-module reach.

**Severity:** Low · **Platform:** All · **Type:** refactor · **Scope:** small

**Acceptance:**
1. Decide between two clean homes:
   - **(A)** Move `AppApiError` into the existing `crate::error` module (replacing or coexisting with the dead `AppError`). Imports become `use crate::error::AppApiError;`. Touches the 7 importers + `error.rs` + a couple lines in `api/mod.rs` for the `OpenApi` schema reference.
   - **(B)** Move `AppApiError` into a new `crate::api::error` module. Imports become `use crate::api::error::AppApiError;`. Same touch surface; keeps API-specific types under `api/`.
2. Delete the dead `crate::error::AppError` enum either way (option A folds it; option B drops it).
3. Update the OpenAPI registration in `api/mod.rs` (currently `paths(...)` references handlers that return `AppApiError` and `components(schemas(...))` includes it as a bare ident — adjust path if the type moves).
4. `cargo test -p actio-core --lib` 214/214 stays green; `cargo check` clean of new warnings.

Option B is more idiomatic for a multi-module API crate (the type is API-specific, not a general crate error). Option A is shorter (uses the existing module). Either is fine — the win is removing the surprise of `use crate::api::session::AppApiError;` from sibling files plus retiring the dead `AppError`.

---

### 68. `.env.example` files are stale (backend) or missing (frontend)

**Status:** Resolved 2026-04-27 — both files updated. Verified actual env-var usage by grep before rewriting:

- `LLM_BASE_URL` / `LLM_API_KEY` / `LLM_MODEL` are **live** — `config::LlmConfig::from_env_optional()` reads them to seed the initial Remote-LLM config on first launch. Kept (commented out by default with a note that they're bootstrap-only).
- `RUST_LOG` standard tracing var — kept.
- `DATABASE_URL`, `HTTP_PORT`, `WORKER_HOST`, `WORKER_PORT` had **zero references** in `backend/`. Dropped, and added a footer block explaining what replaces them (SQLite path is computed from `config.data_dir`; backend HTTP uses port-discovery 3000–3009; no Python worker).
- New `frontend/.env.example` documents `VITE_ACTIO_API_BASE_URL` with a "set this only when…" note covering Tauri custom shell, remote dev, and bypass-discovery cases.

The original triage assumption that `LLM_*` were unused was wrong (they're bootstrap-only, but they're alive); the rewritten file keeps them with the right framing instead of dropping them.

Two environment-config docs gaps surfaced during the ISS-067 cleanup:

**(a) Backend `.env.example` is stale and misleading.** `backend/.env.example`:

```
DATABASE_URL=postgres://actio:actio@localhost:5433/actio   ← codebase uses SQLite
HTTP_PORT=3000                                              (kept; this works)
WORKER_HOST=127.0.0.1                                       ← no Python worker
WORKER_PORT=50051                                           ← no Python worker
LLM_BASE_URL=https://api.openai.com/v1                      ← env vars unused;
LLM_API_KEY=sk-...                                            LLM config lives in
LLM_MODEL=gpt-4o-mini                                         settings.json now
RUST_LOG=actio_asr=info                                     (kept; standard)
```

`CLAUDE.md` line 12 explicitly states: *"There is **no** Python worker, no gRPC, no Postgres, no Docker. Older docs mentioning those are obsolete."* The `.env.example` is exactly the obsolete docs CLAUDE.md flags. A new contributor following it would set up invalid Postgres credentials, a phantom worker host, and unused LLM env vars; the resulting confusion ("but the .env.example said…") is exactly the friction CLAUDE.md tries to head off.

**(b) Frontend has no `.env.example`** at all. `VITE_ACTIO_API_BASE_URL` is referenced in 9+ files now (after #52 + #63 plumbed it everywhere) and isn't documented in any README/CLAUDE.md/AGENTS.md. A contributor running `pnpm dev` against a non-default backend host has to grep source for the env-var name.

**Severity:** Low · **Platform:** All · **Type:** docs · **Scope:** small (rewrite the 16-line backend file; add an 8-line frontend one)

**Acceptance:**
1. `backend/.env.example` rewritten to match the actual environment surface:
   - `DATABASE_URL` is unused (we open `<config.data_dir>/actio.db` directly via SQLite). Drop it.
   - `WORKER_HOST` / `WORKER_PORT` — drop.
   - `LLM_*` — drop. Note that LLM config is round-tripped through `settings.json` and hot-reloaded via `PATCH /settings`.
   - Keep `RUST_LOG` (standard tracing var).
   - Add comment block at top noting "Most config is in settings.json (round-tripped via the GUI)" so contributors don't expect every flag to come from env.
2. `frontend/.env.example` created with `VITE_ACTIO_API_BASE_URL=http://127.0.0.1:3000` and a comment explaining when to override (Vite + Tauri build for a host other than the defaults). Match the existing 9 in-code uses.
3. The pair satisfies the test "a new contributor cloning the repo can run dev mode without grepping source for env vars."

Bonus: a one-line note in the root README.md (if one exists; otherwise the closest equivalent) pointing at both files.

---

### 69. README drift — `.env` example, port semantics, embedding dims

**Status:** Resolved 2026-04-27 — README updated:

- (a) Dropped `HTTP_PORT=3000` from the example `.env` block (the var is dead in code). Added a paragraph framing the env vars as bootstrap-only and pointing at `backend/.env.example` for the canonical list.
- (b) Both the "serves on…" line and the "Default HTTP port" callout now mention the 3001–3009 fallback handled by `getApiBaseUrl()`.
- (c) Speaker-embedding bullet now says "3D-Speaker family… vector dim is per-model: CAM++ family + ERes2Net v2 + TitaNet emit 192-dim, ERes2Net Base emits 512-dim", matching CLAUDE.md's Non-obvious-patterns wording.
- (d) **False alarm at fix time** — verified `api/mod.rs:189-197` against the README's bullets: `enroll-live/start` and `enroll-live/cancel` are scoped to `/speakers/:id/`; `enroll-live/status` is global (no speaker id). The README already says exactly that. No change.
- (e) Diarization bullet rewritten to lead with cosine clustering of per-segment embeddings; pyannote files mentioned as still-bundled for the alternate segmentation path.

No code changes; README only.

`README.md` accumulated several misalignments with current code, surfaced while reviewing it after the ISS-068 env-template work landed:

**(a) `HTTP_PORT=3000` in the README's example `.env`** (line 92):

```
Optional `.env` in `backend/`:
HTTP_PORT=3000
```

`HTTP_PORT` doesn't exist in the codebase — `grep -rE "HTTP_PORT" backend/` returns zero. The server listens on a hardcoded `127.0.0.1:3000` with port-fallback to 3001–3009 handled on the *frontend* via `getApiBaseUrl()`. My ISS-068 fix two ticks ago dropped `HTTP_PORT` from `backend/.env.example` precisely because it's dead, but the README still suggests setting it.

**(b) Port-fallback not mentioned** (line 87): *"serves on http://localhost:3000"*. True for the happy path, but the codebase has the 3000-3009 fallback shape that several recent issues (#52, #63) built around — a contributor whose port 3000 is taken would be confused.

**(c) Embedding-dim drift** (line 38): *"Speaker embedding — 3D-Speaker (512-dim)"*. CLAUDE.md "Non-obvious patterns" explicitly says: *"Embedding dimension is per-model, not a single repo-wide constant. Five of six catalog models (CAM++ family + ERes2Net v2 + TitaNet) emit 192-dim vectors; ERes2Net Base emits 512-dim."* The README hardcodes 512 — a contributor reading it would assume embeddings are uniform.

**(d) API surface drift** (line 64): the example bullet `POST /speakers/{id}/enroll-live/start` matches code; the listed `GET /enroll-live/status` doesn't (the actual path includes the speaker ID — verify against `api/mod.rs`).

**(e) "Diarization — pyannote segmentation"** (line 39): the engine still references pyannote in `model_manager` paths, but the production diarization path uses cosine clustering against speaker embeddings (per CLAUDE.md's `batch_processor.rs` description). README's wording suggests pyannote does the segmentation itself, which understates the embedding-clustering reality.

**Severity:** Low · **Platform:** All · **Type:** docs · **Scope:** small (rewrite ~5 lines in README)

**Acceptance:**
1. Drop `HTTP_PORT=3000` from the README example `.env` block. Replace with a one-line note that port-fallback (3000–3009) is handled by the frontend's `getApiBaseUrl()` discovery probe.
2. Update line 87 to mention "with fallback to 3001–3009 if held".
3. Soften line 38 to "Speaker embedding — 3D-Speaker family (192- or 512-dim depending on model — see CLAUDE.md Non-obvious patterns)".
4. Verify line 64's `/enroll-live/status` path against `api/mod.rs` and correct if drifted.
5. Soften line 39 to mention that production diarization is cosine-clustered embeddings, with pyannote files retained for the legacy / future segmentation path.
6. `cargo build` / `pnpm build` are unaffected — README changes only.

This is the natural follow-up to ISS-068: now that the env templates are accurate, the README's parallel claims should match.

---

### 70. AGENTS.md drift — embedding dim, session-end vs rolling windows, broken xrefs

**Status:** Resolved 2026-04-27 (mostly).

Two-step process: the `**/AGENTS.md` line in `.gitignore` matches both files via `git check-ignore`, but `git ls-files` shows `backend/AGENTS.md` IS tracked (predates the gitignore rule and was never `git rm --cached`'d). Only the root `AGENTS.md` is truly untracked. So the fixes split across two files with different commit shapes:

- (a) `backend/AGENTS.md:48` rewritten to mirror CLAUDE.md's Non-obvious-patterns wording. **Committed.**
- (b) `backend/AGENTS.md:7` rewritten — rolling 5-min window extractor, confidence-gating, frontend-side port-fallback. **Committed.**
- (d) `tests/AGENTS.md` xref dropped from `backend/AGENTS.md`'s Subdirectories table. **Committed.**
- (c) Root `AGENTS.md:31` CAM++ clarification — local-only (root file is genuinely not tracked); the working-tree edit benefits this user's AI tooling but doesn't ship.
- (e) Both files stamped `Updated: 2026-04-27` (root locally, backend committed).

**Takeaway:** `git check-ignore` reports a rule match even on already-tracked files; the operational difference is whether `git ls-files --error-unmatch <path>` succeeds. The earlier "won't fix" framing in this issue was wrong about the backend file; corrected here. The audit lesson stands but with a sharper test: combine `check-ignore` with `ls-files` to distinguish "rule applies but file is already tracked" from "file truly outside the repo."

`backend/AGENTS.md` and the root `AGENTS.md` haven't been updated since 2026-04-17 and now contradict the code in ways that would actively mislead a contributor.

**(a) The smoking gun: backend/AGENTS.md:48 has the embedding-dim story exactly backwards.**

```
- Speaker embeddings are 512-dim (3D-Speaker). Any hardcoded `192` in
  the code is stale from the earlier CAM++ design and should be fixed
  as you encounter it.
```

CLAUDE.md "Non-obvious patterns" (line 130) explicitly says: *"Five of six catalog models (CAM++ family + ERes2Net v2 + TitaNet) emit 192-dim vectors; ERes2Net Base emits 512-dim. The DB tracks `embedding_dimension` per row and `speaker_matcher` filters joins on the active dim."*

Confirmed in code: `engine/model_manager.rs` lists CAM++ (`campplus_zh_en`, `campplus_zh`), ERes2Net v2 (`eres2netv2`), TitaNet (`titanet_small_en`) as the live catalog. **192-dim is the majority case; 512 is the outlier (ERes2Net Base only).** A contributor literally following AGENTS.md's instruction to "fix any hardcoded 192" would convert legitimate references and break embedding storage on every CAM++ user.

This is the highest-priority slice of this issue: it's not just stale, it's an *attractive nuisance* — the wording invites a confident wrong fix.

**(b) backend/AGENTS.md:7 — "reminders … at session end".**

> *"optionally generates reminders from transcripts using a local (llama.cpp) or remote (OpenAI-compatible) LLM at session end."*

The current architecture (CLAUDE.md "Always-listening action extractor", `engine/window_extractor.rs`) runs rolling 5-min windows continuously, not at session end. The "session end" model existed in an earlier iteration; the rolling-window extractor superseded it (and lands medium-confidence items in the Needs-Review queue, which is the actual product feature).

**(c) Root AGENTS.md:31 — "Disregard older docs that mention … CAM++ … — all obsolete".**

CAM++ is the *recommended default* speaker embedding model. `engine/app_settings.rs` literally hardcodes `"campplus_zh_en"` as the default for the Chinese+English language pair. The line probably meant "older docs that mentioned CAM++ as a Python-worker dependency are obsolete (CAM++ now runs natively via sherpa-onnx)" — but as written, a contributor would conclude CAM++ is dead and remove it.

**(d) Broken cross-reference: backend/AGENTS.md:27 cites `tests/AGENTS.md`.**

`backend/tests/AGENTS.md` doesn't exist. The Subdirectories table promises documentation that isn't there.

**Severity:** Medium · **Platform:** All · **Type:** docs (the embedding-dim slice is borderline a bug class because it actively misleads) · **Scope:** small (rewrite ~6 lines across 2 files; possibly create or remove the tests/ subsection)

**Acceptance:**
1. Rewrite `backend/AGENTS.md:48` to mirror CLAUDE.md's Non-obvious-patterns wording: 192-dim is the majority across CAM++ + ERes2Net v2 + TitaNet; 512-dim is ERes2Net Base only; the DB tracks `embedding_dimension` per row; never hardcode either number.
2. Update line 7 to mention the rolling 5-min window extractor (cite `window_extractor.rs`); the "session end" path is the legacy `InferencePipeline::start_session` shape that #44 tracks.
3. Either rewrite root `AGENTS.md:31`'s CAM++ phrase to clarify the obsolete bit was the *Python-worker* version, or drop CAM++ from the obsolete list entirely. The other items in the list (python-worker, proto, gRPC, FunASR Python, Postgres) are still correctly flagged.
4. Resolve the `tests/AGENTS.md` xref: either create a 1-paragraph file documenting the integration-test scaffold, or drop the subsection from the Subdirectories table.
5. Stamp both files with `Updated: 2026-04-27` (or whatever the landing date is) so future contributors can see the docs were refreshed against the post-batch-pipeline architecture.

These are the AGENTS.md analogue of ISS-049 (CLAUDE.md) and ISS-069 (README) — same drift class, different files.

---

### 72. `formatTimeShort` has time-vs-calendar bucketing bugs the tests now pin

**Status:** Resolved 2026-04-27 — `formatTimeShort` rewritten to use a calendar-aware day diff (`startOfDay(target) - startOfDay(now)` rounded to days) instead of `floor(diffMin / 1440)`, and the "Due {dayName}" path now uses `dayNames[d.getDay()]` directly. All three bugs fixed:

(a) Next-calendar-day < 24h ahead now correctly says "Tomorrow at h:mm AM/PM" (previously said "h:mm AM/PM today").
(b) Weekday name now matches the actual target day (`dayNames[d.getDay()]`) instead of the time-bucket-derived `(now.getDay() + diffDays) % 7`. Sunday +6 cal days correctly renders "Sunday" (previously rendered "Saturday").
(c) Past dates within the last week now render "Due Friday" / "Due Sunday" / etc. instead of the previous "Due undefined" (the negative-modulo bug is gone — using `target.getDay()` sidesteps it entirely).

Test file updated to assert the corrected behaviour. 215 → 214 tests overall (consolidated two "current-behaviour" tests into single corrected assertions). tsc clean; full suite green.

Two real UX bugs surfaced while writing the ISS-071 tests for `formatTimeShort`:

**(a) "today" label leaks into next-calendar-day times under 24h ahead.** The function uses `diffMin = (target - now) / 60000` and `diffDays = floor(diffMin / 1440)` for time-bucket arithmetic, *not* calendar-day boundaries. So:

```
now    = Mon 14:00
target = Tue 09:00 (next calendar day, but only 19h ahead)
diffMin = 1140  → falls into `if (diffMin < 1440) return ${timeStr} today`
                 → renders as "9:00 AM today"
```

A reminder due Tuesday morning shows up labeled "today" on Monday afternoon. Subtle but visible — every reminder card on the Board, Needs-Review, and the trace inspector formats this way.

**(b) Weekday name is computed from time-bucketed `diffDays`, not calendar position.** Same root cause:

```
now    = Mon 14:00
target = Sun 11:30 (+6 calendar days)
diff   = 5d 21h 30m → diffMin = 8490 → diffDays = floor(8490/1440) = 5
                                        ^ NOT 6, so weekday rolls to Saturday not Sunday
                  → renders as "Saturday at 11:30 AM"
```

The render says "Saturday" for what is actually Sunday, again because the bucket is hour-based.

**(c) "Due {dayName}" emits "Due undefined" for past dates.** The `dayLabel` helper at `time.ts:15-19` does `dayNames[(now.getDay() + diffDays) % 7]`. For past dates, `diffDays` is negative, and JS's `%` operator preserves sign: `-3 % 7 === -3`, which indexes `dayNames` out of bounds → `undefined`. So a reminder due 3 days ago renders the literal string `"Due undefined"`. The fix is `((diffDays % 7) + 7) % 7` (canonical positive-modulo).

The ISS-071 test file pins all three behaviours as "current state" with comments — a future deliberate fix will update both the function and the tests together.

**Severity:** Low · **Platform:** All · **Type:** bug · **Scope:** small (one function, ~10 lines of arithmetic — the trick is doing it without breaking the assertion that `iso(0,...)` falls into the right bucket)

**Acceptance:**
1. `formatTimeShort` renders "today" only when `target.getDate() === now.getDate()` AND the timezone month/year agree, not just `diffMin < 1440`. Move to calendar-aware comparisons (`startOfDay(target).getTime() - startOfDay(now).getTime()` etc.).
2. Weekday name uses `target.getDay()` directly rather than `(now.getDay() + diffDays) % 7`. Eliminates both the time-bucket lag and the negative-modulo bug at once.
3. The "Due {dayName}" branch shows the actual past weekday (target.getDay()) — never "undefined".
4. Update `time.test.ts` so that the formerly-pinned-as-buggy cases now assert correct behaviour: `iso(1, 9, 0)` → "Tomorrow at 9:00 AM", `iso(6, 11, 30)` → "Sunday at 11:30 AM", `iso(-3, 10)` → "Due Friday".
5. Existing "1-23h ahead today" cases remain correct (just `today` label re-anchored to calendar same-day).

This is the natural follow-up to #71: now that the buggy behaviour is pinned, fixing it is a deliberate edit instead of an accidental break.

---

### 71. `formatTimeShort` is unrendered-everywhere and untested

**Status:** Resolved 2026-04-27 — added `frontend/src/utils/__tests__/time.test.ts` with 10 cases under `vi.useFakeTimers()` (anchored at `2026-04-27 14:00 Mon`) covering all 7 rendering branches, the 12-hour edge cases, and minute-padding. Three buggy behaviours discovered during the fix were pinned as current state with inline comments, and filed as ISS-072 for a deliberate cleanup tick. 205 → 215 frontend tests; tsc clean.

`frontend/src/utils/time.ts::formatTimeShort` (32 lines) is the function rendered on every reminder card (Board, Needs-Review, Card.tsx, NeedsReviewView.tsx) for the "due time" display — and it has zero direct test coverage. `frontend/src/utils/__tests__/` only contains `platform.test.ts`.

The function has 6+ branches with real edge cases:

```ts
if (diffMin < 0) {
  if (absDiffMin < 1440) return timeStr;        // (1) past, same day
  return `Due ${dayLabel()}`;                    // (2) past, older
}
if (diffMin < 60) return `In ${diffMin} min`;   // (3) imminent
if (diffMin < 1440) return `${timeStr} today`;  // (4) later today
if (diffDays === 1) return `Tomorrow at …`;     // (5) tomorrow
if (diffDays <= 6) return `${dayLabel()} at …`; // (6) this week
return d.toLocaleDateString(…);                  // (7) further out
```

Plus subtleties:
- **`hours % 12 || 12`** — noon renders as `12 PM` (not `0 PM`), midnight as `12 AM`. A refactor that wrote `hours % 12` plain would render midnight as `0 AM`.
- **`(now.getDay() + diffDays) % 7`** — week-day-name rollover. Sunday → +1 → Monday is straightforward; Friday → +3 → Monday should give "Monday", but a refactor that dropped `% 7` would index out of bounds.
- **`Math.floor(diffMin / 1440)`** — DST nights have 23 or 25 hours; this divides by a fixed 1440 minutes. Acceptable for "due Tuesday" UX but worth knowing.
- **`d.toLocaleDateString('en-US', …)`** — hardcoded `en-US` despite the bilingual UI. The Chinese locale renders dates differently; a `MMM D` of "Apr 27" doesn't translate to 中文. Probably intentional (the rest of the function uses English idioms like "Tomorrow" and "AM/PM" too — full i18n would be a bigger lift), but worth pinning the current behavior.

A regression in any of these branches lands silently because nothing exercises them.

**Severity:** Low · **Platform:** All · **Type:** test · **Scope:** small (extend or create `utils/__tests__/time.test.ts`)

**Acceptance:**
1. New `frontend/src/utils/__tests__/time.test.ts` under `vi.useFakeTimers()` (set a fixed `Date.now()` so `new Date()` inside the function is deterministic) covering at minimum:
   - Past same-day → time string only
   - Past older → `Due {dayName}` for 2-6 days back, `Due {dayName}` rolls over correctly
   - Within an hour → `In N min` for N=1, 30, 59
   - Later today → `h:mm AM/PM today`
   - Tomorrow → `Tomorrow at h:mm AM/PM`
   - This week (2-6 days out) → `{Monday|Tuesday|…} at h:mm AM/PM`
   - Further out → `MMM D` formatted via `toLocaleDateString`
   - 12-hour edge cases: noon → `12:00 PM`, midnight → `12:00 AM`, 1pm → `1:00 PM`, 11am → `11:00 AM`
2. 205 → ~215 frontend tests, all green.

While extending coverage in this file, consider also pinning `computeLabelCounts` (utils/labels.ts:16) — same accumulator pattern, 6 lines of logic, easy bonus. `getLabelById` and `sortByPriority` are too trivial to need direct tests.

---

### 73. Dead exports in `utils/labels.ts` (`computeLabelCounts`, `BUILTIN_LABELS`)

**Status:** Resolved 2026-04-27 — deleted `BUILTIN_LABELS` (8 hardcoded labels, predates backend seeding) and `computeLabelCounts` (zero callers; only referenced by an old plan doc) from `frontend/src/utils/labels.ts`. `getLabelById` (the actually-used export) stays. 214/214 frontend tests still pass; tsc clean. Bundle delta is ~negligible (Vite/Rollup were already tree-shaking the unused exports — the cleanup is source-level clarity rather than a runtime win, which is the right outcome and confirms the dead-code finding).

`frontend/src/utils/labels.ts` exports three things; only `getLabelById` is actually used. The other two are residue from an earlier design:

```ts
export const BUILTIN_LABELS: Label[] = [   // 8 lines — hardcoded
  { id: 'work',     name: 'Work',     ... },
  { id: 'urgent',   name: 'Urgent',   ... },
  ...                                       // Personal / Health / Finance / Meeting
];

export function computeLabelCounts(...)    // 8 lines — accumulator
```

`BUILTIN_LABELS` predates the backend label seeding work (CLAUDE.md mentions "the six seeded default label names" — those live in `i18n/label-names.ts` and `repository::label::seed_defaults`). The frontend reads labels from the backend via `api/actio-api.ts::listLabels()`, never falling back to hardcoded ones. The export is unreachable.

`computeLabelCounts` is referenced by exactly one file in the entire repo: `frontend/docs/superpowers/plans/2026-04-07-frontend-polish.md`, which is a plan doc that says *"Keep `getLabelById` and `computeLabelCounts` unchanged"*. The plan never landed the consumer; the function was added but no UI ever called it. Same shape as the `repository::todo::has_todos` case in ISS-062 — a function preserved for a "the alias route uses it" comment that itself is stale.

Plus during ISS-071 I noted `computeLabelCounts` as a "bonus while there" target. Re-checking now shows there's nothing to bonus into — it's not called.

**Severity:** Low · **Platform:** All · **Type:** dead-code · **Scope:** small (delete ~14 lines + the `Label` type import if it becomes unused)

**Acceptance:**
1. Delete the `BUILTIN_LABELS` constant and the `computeLabelCounts` function from `frontend/src/utils/labels.ts`. Keep `getLabelById`.
2. Drop the `import type { Label }` if `getLabelById`'s signature no longer needs it (it does — leave the import).
3. `pnpm tsc --noEmit` clean — no other file imports the deleted symbols, so this should just compile.
4. `pnpm test` 214/214 unchanged.
5. `pnpm build` — main bundle should drop by ~0.3 kB (the static `BUILTIN_LABELS` array was being included in the entry chunk).

The plan doc reference (`2026-04-07-frontend-polish.md`) is in `frontend/docs/` and refers to past intent — leave it as-is rather than rewriting it; the doc is a historical artifact.

---

### 74. More dead/over-exported exports across `frontend/src/api/` and `utils/`

**Status:** Resolved 2026-04-27 — deleted `isAutostartEnabled` from `utils/autostart.ts` and `enrollSpeaker` from `api/speakers.ts`; dropped `export` from `ProfileResponse`/`UpdateProfileRequest` in `api/profile.ts` and `TranslateLineRequest`/`TranslateLineResponse` in `api/translate.ts`. Also pruned the now-orphan `EnrollResponse` and `DEV_TENANT_ID` imports in `api/speakers.ts`. Verification: `pnpm tsc --noEmit` clean, `pnpm test` 214/214, `pnpm build` succeeded with chunk sizes flat.

After ISS-073 cleaned up `utils/labels.ts`, a deeper sweep over `src/api/` + `src/utils/` (cross-file word-grep) turned up another batch:

**(a) Truly dead — zero callers:**

```
src/utils/autostart.ts::isAutostartEnabled   only the export declaration matches
src/api/speakers.ts::enrollSpeaker           "                                  "
```

`isAutostartEnabled` is interesting: the "Launch at login" preference is **write-only**. `setAutostart(true/false)` is called when the user flips the toggle, but the OS state is never read back. If the user enables launch-at-login and then revokes the autostart permission externally (Windows Task Manager, macOS System Settings), the UI keeps showing "on" — there's no consistency check at boot. Removing the unused export is fine; whether anyone wants to wire the read path is a separate UX call (filed inline below as out-of-scope).

`enrollSpeaker` predates the live-enrollment flow. The current voice-enroll UX uses `startLiveEnrollment` / `consume_segment` (5 passages × ~5s each); the one-shot `enrollSpeaker(audio)` is a shape from the earliest enrollment design that's no longer wired to any UI.

**(b) Over-exported (`export` leakage — used only inside the defining file):**

```
src/api/profile.ts::ProfileResponse          → used by fetchProfile / updateProfile internally
src/api/profile.ts::UpdateProfileRequest     → used by updateProfile internally
src/api/translate.ts::TranslateLineRequest   → used by translateLines internally
src/api/translate.ts::TranslateLineResponse  → used by translateLines internally
```

External consumers import the *function* (`fetchProfile`, `translateLines`) and never the wire shapes. The `export` keyword here advertises an internal contract; downgrading to file-private (no `export`) tightens the surface and prevents drift if the wire shape ever needs to change.

**Severity:** Low · **Platform:** All · **Type:** dead-code / refactor · **Scope:** small (delete 2 functions, drop 4 `export` keywords)

**Acceptance:**
1. Delete `isAutostartEnabled` from `utils/autostart.ts` and `enrollSpeaker` from `api/speakers.ts`.
2. Drop `export` from `ProfileResponse`, `UpdateProfileRequest`, `TranslateLineRequest`, `TranslateLineResponse` (keep the type definitions; they're referenced internally).
3. `pnpm tsc --noEmit` clean.
4. `pnpm test` 214/214 unchanged.
5. `pnpm build` — bundle should stay flat; tree-shaking already DCE'd the unused functions.

**Out of scope (noted for visibility):** the write-only nature of `setAutostart` is itself a small UX gap — if anyone wants the toggle to reflect actual OS state on launch, that's a separate ticket (call `tauri-plugin-autostart`'s `is_enabled` at boot and reconcile against `preferences.launchAtLogin` in localStorage, with a one-time toast if they diverge). Out of scope for #74; flagged here so the inevitable "wait, isn't isAutostartEnabled supposed to be called?" question has an answer when this lands.

---

### 75. `LlmSettings` swallows `patchLlmSettings` errors on two paths, divorcing UI state from server state

**Status:** Resolved 2026-04-27 — "Local" radio onChange now routes through `handleSelectionChange`; `cancelAndUnselect` now wraps the `patchLlmSettings` call in try/catch and surfaces failures via `setError`. Added `frontend/src/components/settings/__tests__/LlmSettings.errors.test.tsx` to pin the new behaviour: stubs the `/settings` PATCH to 500, asserts the "Failed to save" banner renders. Verification: `pnpm tsc --noEmit` clean, `pnpm test` 214 → 215, `pnpm build` succeeded with bundle sizes flat.

**Found:** 2026-04-27

`frontend/src/components/settings/LlmSettings.tsx` has three sibling paths that persist a `LlmSelection` change. Two of them route through `handleSelectionChange` (lines 157–177), which captures errors into `setError(...)` so a failed save surfaces as red text inline. The other two skip that wrapper and use `.catch(() => {})` — meaning a failed save silently leaves the radio looking selected while the backend still has the prior value.

**The inconsistency, side-by-side:**

```ts
// "Disabled" radio (line 232) — error-aware
onChange={() => handleSelectionChange({ kind: 'disabled' })}

// "Local" radio (line 241) — error-silent
onChange={() => {
  setSelection({ kind: 'local', id: '' });
  patchLlmSettings({ selection: { kind: 'local', id: '' } }).catch(() => {});
}}

// "Remote" radio (line 253) — error-aware
onChange={() => handleSelectionChange({ kind: 'remote' })}
```

The same anti-pattern shows up in `cancelAndUnselect` (line 294–300):

```ts
const cancelAndUnselect = async () => {
  await cancelLlmLoad();
  setLoadStatus({ state: 'idle' });
  const cleared: LlmSelection = { kind: 'local', id: '' };
  setSelection(cleared);
  patchLlmSettings({ selection: cleared }).catch(() => {});  // ← swallowed
};
```

**Why the duplication exists:** the Local-radio onChange duplicates the first half of `handleSelectionChange` because it knows `id: ''` doesn't need the load-status flow (`if (sel.kind === 'local' && sel.id)` is false). But that's exactly what `handleSelectionChange` already handles — it falls through to `setLoadStatus({ state: 'idle' })` for the no-id case. So the duplication is unnecessary.

**Failure modes:**
1. User clicks "Local" radio while offline / backend down. UI shows Local selected; backend still has Disabled/Remote. After page refresh, the Disabled/Remote radio reappears with no explanation.
2. During `cancelAndUnselect`, the cancel succeeds against the load endpoint but the persistence-clear fails. Same divorce: UI shows cleared, backend still has the old `{ kind: 'local', id: '<x>' }`.

**Severity:** Low · **Platform:** All · **Type:** bug (silent failure) / refactor · **Scope:** small (~6 LoC change in one file)

**Proposed direction:**
1. Replace the Local-radio onChange (lines 241–244) with `() => handleSelectionChange({ kind: 'local', id: '' })` — `handleSelectionChange` already handles the empty-id branch correctly.
2. Convert `cancelAndUnselect`'s trailing `.catch(() => {})` into `try { await patchLlmSettings(...) } catch (e) { setError(e instanceof Error ? e.message : t('settings.llm.saveFailed')) }`. The user already saw `await cancelLlmLoad()` succeed; if the persistence clear fails, they need to know.

**Acceptance:**
1. Both error-silent paths (`onChange` Local radio, `cancelAndUnselect`) now route through `handleSelectionChange` or surface via `setError`.
2. Add a vitest covering one of the two paths: stub `patchLlmSettings` to reject; assert the component renders the error message instead of leaving the radio in a phantom-selected state.
3. `pnpm tsc --noEmit` clean; full `pnpm test` green.

**Out of scope:** other `.catch(() => {})` in this file (e.g. `fetchLoadStatus().then(setLoadStatus).catch(() => {})` at line 141 — that's a status fetch with no UI commitment, swallow is defensible). Only state-persisting calls that diverge from sibling paths' behaviour are in scope here.

---

### 77. Archive deletes are destructive with no confirmation, single OR bulk

**Status:** Resolved 2026-04-27 — wired `useConfirm()` + `<ConfirmDialog>` (destructive tone) into all four delete entry points in `ArchiveView.tsx`. Added a single `requestDelete(count)` helper that picks the singular vs `{count}`-substituted message, with shared cancel/confirm labels. Three new i18n keys (`archive.confirmDeleteOne`, `archive.confirmDeleteBulk`, `archive.cancel`) landed in both `en.ts` and `zh-CN.ts` — parity test stayed green. New test `ArchiveView.confirm.test.tsx` pins both the per-row Cancel path (zero `deleteReminder` calls) and the bulk Confirm path (exactly N calls with the right ids). Verification: `pnpm tsc --noEmit` clean, `pnpm test` 215 → 217, `pnpm build` succeeded with bundle sizes flat.

**Found:** 2026-04-27

`frontend/src/components/ArchiveView.tsx` exposes four delete entry points with **zero confirmation prompts and no undo**:

| Surface | Line | Action |
|--------|------|--------|
| Per-row task delete | 297-306 | `void deleteReminder(reminder.id)` |
| Bulk task delete | 250-252 | `bulkDeleteTasks` → forEach `deleteReminder` |
| Per-clip delete | 438-450 | `deleteSegment(segment.id)` |
| Bulk clip delete | 387-389 | `bulkDeleteClips` → forEach `deleteSegment` |

`deleteReminder` (`store/use-store.ts:276`) issues `api.deleteReminder(id)` (server `DELETE /reminders/:id`), strips the row from in-memory state, and pushes a `'feedback.deletedPermanently'` toast. The toast is **informational, not actionable** — there's no undo button, no countdown, nothing reversible. `deleteSegment` (`store/use-voice-store.ts:924`) is local-only but identically irreversible.

**Why this matters:**
1. **Inconsistency with the rest of the app.** ISS-43 (resolved 2026-04-26) established `ConfirmDialog` + `useConfirm()` (`components/ConfirmDialog.tsx`) and wired it into the two prior destructive surfaces (`CandidateSpeakersPanel:9,151,210`, `settings/ModelSetup:3,92,545`). Archive sits alongside those as the third major destructive entry point but skipped the migration.
2. **Bulk delete is the worst case.** Selecting 50 items + clicking the bulk Delete button = 50 instant DELETE calls. Mis-tap on an iPad or accidental double-click after a select-all means a measurable chunk of someone's archive is gone with one toast they can't act on.
3. **Archive ≠ Trash.** From the user's mental model the items are *already* archived (i.e. removed from the active board). The Delete button here means "permanently destroy" — a step further. Users who don't read the button label twice can interpret it as "remove from this view," not "remove from the database."

**Severity:** Medium · **Platform:** All · **Type:** ui (broken affordance) · **Scope:** small (~20 LoC: import `useConfirm`, wrap each of the 4 delete handlers, render `<ConfirmDialog {...dialogProps} />` once at root, add 4 i18n string pairs)

**Proposed direction:** mirror the `CandidateSpeakersPanel` migration. Single deletes → confirm with destructive tone, single-item count. Bulk deletes → confirm with item count substituted into the message:

```ts
const { confirm, dialogProps } = useConfirm();
// per-row
const handleDeleteReminder = async (id: string) => {
  if (!await confirm({
    message: t('archive.confirmDeleteOne'),
    confirmLabel: t('archive.action.delete'),
    cancelLabel: t('common.cancel'),
    tone: 'destructive',
  })) return;
  void deleteReminder(id);
};
// bulk
const handleBulkDeleteTasks = async () => {
  if (!await confirm({
    message: t('archive.confirmDeleteBulk', { count: selectedTaskIds.size }),
    confirmLabel: t('archive.action.delete'),
    cancelLabel: t('common.cancel'),
    tone: 'destructive',
  })) return;
  selectedTaskIds.forEach((id) => void deleteReminder(id));
  setSelectedTaskIds(new Set());
};
```

**i18n:** new keys `archive.confirmDeleteOne`, `archive.confirmDeleteBulk` (with `{count}` placeholder) land in BOTH `en.ts` and `zh-CN.ts` (parity test enforces it).

**Acceptance:**
1. All four delete entry points route through `useConfirm()` with `tone: 'destructive'`.
2. New i18n keys present in en + zh-CN.
3. Vitest covers at least one path: e.g. simulate bulk-delete-2 → assert no `deleteReminder` calls until confirm fires; cancel = zero calls; confirm = exactly N calls.
4. `pnpm tsc --noEmit` clean; `pnpm test` green; `pnpm build` clean.

**Out of scope (noted for visibility):** ArchiveView's tablist/tabpanel ARIA pattern is half-implemented — `role="tablist"` + `role="tab"` on the section buttons (lines 177-202), but the corresponding `<motion.div>` panels (lines 207, 319) lack `role="tabpanel"` + `aria-labelledby`. Same shape on the clip-filter group (line 332-343): `role="group"` is set, but the buttons lack `aria-pressed` to indicate which is active. Separate ticket; #77 is the destructive-action hole, not the a11y sweep.

---

### 76. Backend has 37 clippy warnings in `actio-core` lib (denied by the auto-improve script)

**Status:** In-progress 2026-04-27 — batch 1 of 8 landed (`ptr_arg` sweep in `engine/model_manager.rs`, 7 sites converted from `&PathBuf` → `&Path`). Lib warnings 37 → 30; all 214 backend tests stay green. Remaining batches: `useless_conversion` in `api/ws.rs`, test-only `field_reassign_with_default` in `app_settings.rs`, `needless_update` in `diarization.rs`, `unnecessary_map_or`, `derivable_impls`, `unnecessary_cast`, singletons.

**Found:** 2026-04-27

`cd backend && cargo clippy --all-targets -- -D warnings` (the QUALITY-lane lint that the auto-improve loop runs) currently fails with **37 warnings in `actio-core` lib + 7 unique additional in `actio-core` lib tests + 2 in `actio-desktop`**. Cargo.toml has no `[lints]` section so plain `cargo clippy` only emits warnings (37 → exit 0); the `-D warnings` flag in the loop script is what flips them to errors.

That means the loop's `backend clippy` lane is effectively un-runnable as a discovery tool: it always fails on legacy noise, drowning any genuinely new warning a future change might introduce. Future regressions land invisibly.

**Categorisation (lib + lib test, dedup'd):**

| Category                            | Count | Files                                                                 |
|-------------------------------------|-------|-----------------------------------------------------------------------|
| `ptr_arg` (`&PathBuf` → `&Path`)    | 6     | `engine/model_manager.rs:651,658,664,1022,1040,1052` (and one more)   |
| `useless_conversion`                | 5     | `api/ws.rs:134,161,180,194` + `engine/inference_pipeline.rs:822` (test) |
| `field_reassign_with_default`       | 4     | `engine/app_settings.rs:843,856,866,921` (all in tests)               |
| `unnecessary_map_or`                | 4     | `engine/cluster.rs:54`, `engine/continuity.rs:171`, `engine/voiceprint_clustering.rs:95`, `repository/speaker.rs:296` |
| `needless_update`                   | 3     | `engine/diarization.rs:47,62,66`                                      |
| `too_many_arguments`                | 3     | `engine/inference_pipeline.rs:66`, `repository/segment.rs:203`, `src-tauri/src/main.rs:55` |
| `type_complexity`                   | 3     | `api/reminder.rs:196`, `engine/window_extractor.rs:403,713`           |
| `derivable_impls`                   | 2     | `engine/app_settings.rs:382` (`AppSettings`), `engine/llm_router.rs:21` (`LlmSelection`) |
| `collapsible_if`                    | 2     | `api/reminder.rs:308`, `engine/app_settings.rs:102`                   |
| `needless_borrow`                   | 2     | `api/reminder.rs:398`, `src-tauri/src/main.rs:296`                    |
| `unnecessary_cast`                  | 2     | `engine/batch_processor.rs:418,419`                                   |
| Singletons                          | 7     | `manual_map`, `manual_strip`, `should_implement_trait` (`SpeakerKind::from_str`), `manual_is_multiple_of`, `explicit_auto_deref`, `doc_lazy_continuation`, `map_flatten`, `manual_range_contains` (test), `needless_range_loop` (test) |

**Severity:** Medium · **Platform:** All · **Type:** refactor / cleanup · **Scope:** medium-large overall (touches ~15 files); breaks down into independent SMALL sub-batches that can land separately.

**Proposed direction — split into independent commits, each verifiable in one tick:**

1. **`ptr_arg` sweep in `model_manager.rs`** (single-file, 6 sites, mechanical `&PathBuf` → `&Path` + `use std::path::Path`).
2. **`useless_conversion` in `api/ws.rs`** (4 sites, drop `.into()` on already-String/Vec values).
3. **`field_reassign_with_default` test cleanup in `app_settings.rs`** (test code only, lowest risk).
4. **`needless_update` in `diarization.rs`** (3 sites, drop `..Default::default()` from already-fully-spec'd structs).
5. **`unnecessary_map_or` → `is_none_or`/`is_some_and`** (4 files, idiomatic Rust 1.82+ migration).
6. **`derivable_impls`** (`AppSettings` → `#[derive(Default)]`, `LlmSelection` → `#[derive(Default)] + #[default]`).
7. **`unnecessary_cast` in `batch_processor.rs`** (2 sites: `seg.start_ms as i64` is already i64).
8. The remaining singletons (8) — bundle the trivial ones, leave `too_many_arguments` (3) and `type_complexity` (3) for a separate refactor pass since they suggest larger structural change.

After all batches land: add a `[workspace.lints.clippy]` block to root `Cargo.toml` denying the categories we cleaned, so regressions trip CI immediately.

**Out of scope for #76:** introducing new lint denials (`pedantic`, `nursery`) or enabling a CI workflow — the goal is to drain the existing warning queue and stand up a deny-on-warnings posture for the *currently-clean* set so the loop's QUALITY lane becomes useful again.

**Acceptance:** After the multi-commit rollup, `cd backend && cargo clippy --all-targets -- -D warnings` passes with exit 0. `cargo test -p actio-core --lib` stays green throughout. No public-API rename (this is intentional — `&PathBuf → &Path` for *internal* helpers; if any of the 6 are `pub`, they get a separate ticket).

---

### 60. `live_enrollment::consume_segment` race-fix and gate logic have no tests

**Status:** Resolved 2026-04-26 — added a `#[cfg(test)] mod tests` to `live_enrollment.rs` with 10 tokio tests covering: no-session bail, non-Active-status bail, three rejection gates (too_short / too_long / low_quality with version bump + last_rejected_reason), accept path (counter + version + last_captured_duration_ms + saved_embedding_ids + cleared rejection), target-reached flip-to-Complete + staging clear, `cleanup_partial_embeddings` selective delete (preserves prior successful enrollment), `cleanup_partial_embeddings` no-op after Complete, and `publish_level` version-stability. Backend lib suite: **204 → 214** tests, all green.


`backend/actio-core/src/engine/live_enrollment.rs` (261 lines, **0 tests**) implements the live voiceprint enrollment flow that CLAUDE.md (line 100) describes:

> Gate checks happen **inside** the Mutex critical section to avoid snapshot-recheck races. Cancelling cleans **only** the rows saved during the current session via `cleanup_partial_embeddings` — prior successful enrollments for the same speaker survive. A watchdog tokio task owns natural-completion teardown (pipeline stop + `session::end_session`) so a Complete status doesn't leak an unbounded DB session.

`consume_segment` (lines 174–261) is the routing entry point with non-trivial ordered logic:

1. Lock the `LiveEnrollment` Arc<Mutex<Option<EnrollmentState>>>.
2. Bail if no session or `status != Active`.
3. Duration gate (`MIN 3 s`, `MAX 30 s`) → set `last_rejected_reason`, bump version, bail.
4. Quality gate (`MIN_QUALITY 0.6`) → reject same way.
5. On accept: bump `captured`, stamp `last_captured_duration_ms`, clear `last_rejected_reason`, bump version, transition to `Complete` if `captured >= target`.
6. **Drop the lock**, persist the embedding via `speaker_matcher::save_embedding` (DB write).
7. **Re-acquire** the lock to push the saved embedding ID onto `saved_embedding_ids` — but only if status is still `Active` (Complete already cleared the staging list, so a post-completion cancel can't wipe legitimate voiceprints).

The race-window between steps 5 and 6/7 is exactly what the CLAUDE.md callout warns about. The check at step 7 (`status == Active`) is load-bearing — if a refactor drops it or moves the staging-list mutation back inside the first lock, completing-then-cancelling could clobber the voiceprint that just got saved.

Other untested behaviours in the same module:

- `start` rejects when a session is already active.
- `cancel` returns the snapshot at cancel time and clears the slot.
- `cleanup_partial_embeddings` deletes only the IDs in `saved_embedding_ids`, leaving prior successful rows for the same speaker intact.
- `is_complete` reflects only `Status::Complete` (not Active or Cancelled).
- `publish_level` updates `rms_level` without bumping `version` (so quiet sessions don't spin the counter).

**Severity:** Low · **Platform:** All · **Type:** test · **Scope:** medium — async tokio test setup with an in-memory SQLite pool plus the `domain::speaker_matcher::save_embedding` dependency. The existing `repository::speaker` tests show the in-memory-pool scaffold to mirror.

**Acceptance:**
1. New `actio-core/src/engine/live_enrollment.rs` `mod tests` covers at minimum:
   - Status gate (Inactive returns Ok(None) without writes).
   - Each duration gate (too short, too long) sets the right `last_rejected_reason` and bumps version.
   - Quality gate.
   - Accept path: counter bumps, version bumps twice on the target-reaching call (once for capture, once for status flip), `last_captured_duration_ms` stamped, `last_rejected_reason` cleared.
   - Race fix: simulating "cancel between save_embedding and second lock" — the staging list mutation no-ops for Cancelled state. (Either inject a delay via a test seam, or test the post-step-7 behaviour: after `cancel` clears the slot, a subsequent `consume_segment` returning past step 5 won't be possible because step 2 bails. The narrower test is to call `consume_segment` then directly mutate the slot to Cancelled and assert no leak.)
   - `cleanup_partial_embeddings` only touches rows in `saved_embedding_ids`.
   - `publish_level` is version-stable.
2. `cargo test -p actio-core --lib live_enrollment` passes; full lib suite stays green.

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
| 74 | More dead exports — `isAutostartEnabled`, `enrollSpeaker`, types | Low | All | Open |
