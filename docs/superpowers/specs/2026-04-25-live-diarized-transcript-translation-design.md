# Live Tab — Diarized Transcripts + LLM Translation

**Status:** Draft (2026-04-25)

## Overview

Two coupled changes to the Live tab.

**Plumbing fix.** The diarized-transcript renderer (`LiveTranscript.tsx`) already exists and groups consecutive same-speaker lines into colored bubbles, but it shows nothing today. After the always-on listening migration removed the manual record/start UI, no surface in the app calls `useVoiceStore.startRecording()` — the only path that opens `/ws` and routes `transcript`/`speaker_resolved` events into the live store. The toggle on the tray writes `audio.always_listening` to the backend but never opens the WS, so `currentSession` stays `null` and the Live tab paints empty even though the backend pipeline is happily producing transcripts.

**Translation feature.** Add an LLM-driven translation toggle to the Live tab header. When on, every finalized transcript line gets translated to a user-selected target language and rendered as a muted second line beneath the source — same bubble, same speaker, inline subtitle. Reuses the existing `LlmRouter` (Disabled / Local llama-cpp / Remote OpenAI-compatible) so translation Just Works whenever an LLM is configured for action-item extraction.

## Goals

- Live tab renders diarized transcripts the moment the user enables listening (or at app boot if always-listening was already on).
- WS subscription lifecycle is a pure derivation of `ui.listeningEnabled` — no manual buttons, no separate state.
- New "Translate" toggle on the Live tab header with a target-language picker.
- When the toggle activates, all already-visible session lines are batch-translated; thereafter, finalized lines flush to the LLM in ~3-second batches.
- Translation degrades gracefully: toggle is disabled with a tooltip when no LLM is configured.
- Translation calls share the same single-flight queue as action-window extraction so neither feature starves the other.

## Non-goals (v1)

- Persistent translations in the database. Ephemeral session-only — closing the Live tab or restarting the app drops them.
- Streaming translation tokens. The current `LlmRouter` returns full responses; streaming is a separate plumbing change we don't need yet.
- Per-speaker source-language hints. The LLM auto-detects per line.
- Manual per-line "translate this one" button. Batch + retry-on-failure is the only entry point in v1.
- Translating partial (in-flight) transcript lines. Partials evolve too fast to translate sensibly; only finals go to the LLM.
- Translating the meeting/board UI itself. That's app i18n, already covered by the existing `useT()` system.

## Architecture

```
┌────────────────────────────┐
│  ui.listeningEnabled       │ ──── effect ──► startRecording / stopRecording
└────────────────────────────┘                       │
                                                     ▼
                                       /ws subscription opens
                                                     │
                          ┌──────────────────────────┴──────────────┐
                          ▼                                         ▼
              transcript frames                          speaker_resolved frames
                          │                                         │
                          ▼                                         ▼
            currentSession.lines (with speaker_id)    (resolution stitched in via
                          │                            pendingResolutions buffer —
                          │                            unchanged from today)
                          ▼
              LiveTranscript renders speaker bubbles
                          │
              ┌───────────┴────────────┐
              ▼                        ▼
    Source line (today)     Translation subtitle (new)
                                       ▲
                                       │
                          translation.byLineId[id]
                                       ▲
                                       │
                            batch every 3s     ──── POST /llm/translate ───► LlmRouter
                            on toggle-on (one-shot backfill)
```

### Plumbing fix — WS lifecycle

A new `useLiveSocket()` hook is mounted at app root. It watches `ui.listeningEnabled` from the settings store and:

- on `null → true` (boot with always-listening on): calls `useVoiceStore.startRecording()`.
- on `false → true` (user toggles on): same as above.
- on `true → false` (user mutes): calls `useVoiceStore.stopRecording()`, which closes the WS and clears `currentSession.lines` and `currentSession.pendingPartial`.
- on `null` (still booting): no-op.

Idempotent: `startRecording()` already short-circuits if the WS is open. The hook is the *only* caller of these two voice-store actions in production code (legacy callers can be deleted; tests still call them directly).

### Translation pipeline

**Voice store gains a `translation` slice:**

```ts
translation: {
  enabled: boolean;            // toggle state
  targetLang: string;          // BCP-47: 'en', 'zh-CN', 'ja', 'es', 'fr', 'de'
  byLineId: Record<string, {
    status: 'pending' | 'done' | 'error';
    text?: string;             // present when status==='done'
  }>;
}
```

`byLineId` is keyed by `TranscriptLine.id`, so it survives speaker re-attribution and partial→final upgrades.

**Lifecycle rules:**

1. `setTranslationEnabled(true)`:
   - Read `currentSession.lines.filter(l => l.is_final)` — every finalized line missing from `byLineId`.
   - If non-empty, dispatch one batch call with all of them. Each gets `status: 'pending'` immediately.
   - Start the forward-batch interval (3s).
2. `setTranslationEnabled(false)`:
   - Stop the interval. Existing `byLineId` entries stay (so toggling back on doesn't re-translate). They render in the UI only while `enabled === true`.
3. `setTranslationTargetLang(newLang)`:
   - Clear `byLineId` (translations are language-specific). Persist new lang to `localStorage.actio-translate-target`.
   - If `enabled`, immediately dispatch a backfill batch in the new language.
4. New finalized line arrives via `handleTranscriptMessage` AND `enabled === true`:
   - Add `{status: 'pending'}` to `byLineId`. The next 3s tick flushes it.
5. `stopRecording()` (user mutes):
   - Clear `byLineId`. Translation toggle stays in its current state (so re-enabling listening preserves intent).

**Forward-batch interval** is a single `setInterval` mounted by the same `useLiveSocket` hook (only when `enabled && currentSession != null`). Every 3 seconds, it collects all `byLineId[id].status === 'pending'` ids and sends them in one POST. On response, each id transitions to `done` (with text) or `error`.

**Source-language detection** is delegated to the LLM. The prompt instructs: "Translate each input line to `{target_lang}`. If a line is already in `{target_lang}`, return it verbatim. Reply with a JSON array `[{id, text}, ...]` matching input order."

### Backend

**New endpoint:** `POST /llm/translate`

Request:
```json
{ "target_lang": "zh-CN", "lines": [{"id": "...uuid...", "text": "..."}] }
```

Response:
```json
{ "translations": [{"id": "...uuid...", "text": "..."}] }
```

Status codes:
- 200: success, JSON body as above.
- 503: `LlmRouter::Disabled` — body `{"error": "llm_disabled"}` so the frontend can surface a precise toast.
- 502: router returned malformed JSON or upstream error after retries.

**`LlmRouter` gains:**

```rust
pub async fn translate_lines(
    &self,
    target_lang: &str,
    lines: Vec<(Uuid, String)>,
) -> Result<Vec<(Uuid, String)>, LlmRouterError>;
```

Same three branches as today:
- `Disabled` → returns `LlmRouterError::Disabled`. The endpoint maps this to 503.
- `Local { slot, .. }` → llama-cpp call with the structured prompt.
- `Remote(client)` → existing OpenAI-compatible HTTP client with the structured prompt.
- `#[cfg(test)] Stub` → echo-back stub: each input line gets a deterministic translation marker so order-preservation tests pass.

The prompt is shared across branches (one helper builds it from `target_lang` + `lines`). Response parsing reuses the existing JSON-extraction helper that already tolerates `</think>` tags and prompt-echoed JSON (commit `b318856`).

### Queueing — single-flight across features

Today, `claim_next_pending` in `window_extractor.rs` is the only LLM caller and its atomic `UPDATE … RETURNING` serializes one window-job at a time per process. Translation calls don't compete for that DB-level claim because they're not window jobs.

We add a process-level `tokio::sync::Mutex<()>` (call it `llm_inflight`) on `LlmRouter` itself. Every `generate_action_items_with_refs` and `translate_lines` call grabs the mutex before invoking the underlying client and releases on response/error. The mutex is fair (tokio's default), so a translation queued behind a long action-window call waits, and vice versa, but neither starves.

Action extraction is bursty (every ~5 minutes when a window matures); translation is steady-state every 3s. The expected steady state is translation + occasional pause for action extraction. Worst case during a window-extraction call (~2–5s), one or two translation batches queue up — that's tolerable.

## UI

### Live tab header

Right of the existing speaker-listing area, add a translation control cluster:

- **Pill button** "Translate" — primary action. `aria-pressed` reflects state; class flips to active styling when on. Mic-button-like visual weight.
- **`<select>` to its right**, target language. Options: English (`en`), 简体中文 (`zh-CN`), 日本語 (`ja`), Español (`es`), Français (`fr`), Deutsch (`de`). Disabled while toggle is off.

**Defaults & persistence:**
- Initial target = current UI locale (`en` → `en`, `zh-CN` → `zh-CN`).
- Selected target persists to `localStorage.actio-translate-target`. Reloaded on app boot.
- Toggle state does NOT persist — every session starts off, by intent (avoid silent LLM cost on app open).

**LLM-disabled state:**
- On mount, query the existing settings to determine `LlmRouter` mode. If `Disabled`, the entire cluster is grayed out (`opacity: 0.5`, `cursor: not-allowed`, click is a no-op). Tooltip on hover: "Enable an LLM in Settings → AI to use translation."
- If the LLM becomes disabled mid-session (settings change), the toggle auto-flips off and a non-modal toast announces: "Translation paused — LLM is disabled."

### Bubble rendering changes

Inside `LiveTranscript`, each line in a bubble renders:

```
┌── bubble ──────────────────────────────────────────┐
│  Avatar  SpeakerName                               │
│                                                    │
│  Source line text from ASR.                        │
│    └ pending: "·· translating" (muted, italic)     │
│    └ done:    "Translated text." (smaller, muted)  │
│    └ error:   "⚠ retry" (clickable, red-tinted)    │
│                                                    │
│  Next source line (same speaker, grouped).         │
│    └ done:    "下一句翻译。"                         │
└────────────────────────────────────────────────────┘
```

**Visual specs:**
- Translation text is `font-size: 0.85em` of source, `color: var(--color-text-tertiary)`, `font-style: italic`.
- Pending placeholder uses the same dot-pulse animation as the unresolved-speaker avatar (consistency).
- Error link `⚠ retry` re-batches just that one line on click (keys it back to `pending`, the next 3s tick flushes it alone).
- Bubbles do NOT translate as a single block — per-line translation preserves alignment so the user can match source to target visually.
- Translation lines are rendered ONLY when `translation.enabled === true`. Flipping the toggle off keeps `byLineId` populated but hides the rendered output.

### Speaker-bubble diarization (already implemented)

The existing `LiveTranscript` rendering — colored avatars, "Identifying…" placeholder, "Unknown" with `?` badge, grouping by `speaker_id` — is preserved unchanged. The plumbing fix is what makes it visible.

## Failure & cost handling

- **Per-batch failure:** every line in the failed batch transitions to `error`. The user can click `⚠ retry` on any one to re-batch just that line. Toggle stays on.
- **Retry storms:** clicking retry on N lines in quick succession produces N small batches (one per click) — acceptable; the single-flight mutex serializes them anyway.
- **Router becomes Disabled mid-session:** toggle auto-flips off (see UI section). `byLineId` is cleared so re-enabling later starts fresh (the user may have changed router config in the interim).
- **Network error on Remote client:** propagates as 502 from the endpoint. Each line in the batch goes to `error`.
- **Slow batches:** the 3s interval continues regardless of whether the prior batch returned. If two batches are in flight, both wait on the `llm_inflight` mutex; the older one finishes first because tokio mutexes are FIFO.
- **No client-side rate limiting in v1.** The 3s batch + single-flight queue is enough back-pressure. If we see thrash in real use, add a min-interval-between-flushes config later.

## Testing

### Frontend

- `useLiveSocket.test.tsx`: mount with `listeningEnabled=null` then transition to `true` → assert `startRecording` called once. Transition `true → false` → assert `stopRecording` called.
- `use-voice-store.test.ts`:
  - `setTranslationEnabled(true)` with 3 finalized lines and 1 partial → expect batch dispatched with 3 ids (partial excluded).
  - `setTranslationTargetLang('ja')` while `enabled` → expect `byLineId` cleared and a fresh batch dispatched in `ja`.
  - `handleTranscriptMessage` with `is_final: true` while `enabled` → expect new line in `byLineId` with `status: 'pending'`.
  - `stopRecording()` → expect `byLineId` cleared.
- `LiveTranscript.test.tsx`:
  - Render with `byLineId[id]={status:'done',text:'你好'}` and `enabled=true` → expect translation in DOM.
  - Same with `enabled=false` → expect translation NOT in DOM.
  - `status:'error'` → expect retry link, click triggers re-batch.

### Backend

- `LlmRouter::translate_lines` with `Stub` → returns canned per-id translations; assert order preservation and id mapping.
- `LlmRouter::translate_lines` with `Disabled` → returns `LlmRouterError::Disabled`.
- `POST /llm/translate` integration test against in-memory app state with `Stub` router → assert 200 + correct body shape.
- `POST /llm/translate` with `Disabled` router → assert 503 + `{"error": "llm_disabled"}`.
- Mutex contention test: spawn one slow-stub `generate_action_items_with_refs` and one `translate_lines` concurrently; assert serialization (second starts only after first completes).

## Out of scope (v1)

- Persistent `transcript_translations` table. Translations are session-only.
- Streaming the LLM response token-by-token.
- Per-speaker source-language hints in the prompt.
- Manual "translate this one" UI without enabling the global toggle first.
- Configurable batch interval. Hardcoded to 3s in v1.
- Configurable target-language list beyond the initial six.
- Translating partial lines.
- Translating from a custom source language (we always auto-detect).

## Open questions

None remaining — design choices locked through brainstorming Q1–Q5.
