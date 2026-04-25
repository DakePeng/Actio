# Live Tab — Diarized Transcripts + LLM Translation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire the always-listening flag to the WS subscription so the existing diarized transcript renderer actually receives data, and ship a session toggle that batch-translates finalized lines via the existing `LlmRouter` and renders them as inline subtitles under each source line.

**Architecture:** A new `useLiveSocket` hook at app root mirrors `ui.listeningEnabled` into `useVoiceStore.startRecording/stopRecording` so the WS lifecycle is a pure derivation of the listening flag. Translation lives in a new `translation` slice on `useVoiceStore` keyed by `TranscriptLine.id`; a 3-second interval flushes pending lines to a new `POST /llm/translate` endpoint, which calls a new `LlmRouter::translate_lines()` method behind a process-level `llm_inflight` mutex that serializes against action-window extraction. UI: a "Translate" pill + language `<select>` in the Live tab header; per-line subtitle inside `LiveTranscript`.

**Tech Stack:** React 19 + TypeScript + Zustand + Vitest (frontend); Rust 2021 + axum + SQLx + tokio + reqwest (backend); existing `LlmRouter` enum (Disabled / Local llama-cpp / Remote OpenAI-compatible / `#[cfg(test)]` Stub).

**Spec:** `docs/superpowers/specs/2026-04-25-live-diarized-transcript-translation-design.md` (commit `bd85aba`).

---

## File Structure

**Frontend — create:**
- `frontend/src/hooks/useLiveSocket.ts` — derives WS lifecycle from `listeningEnabled`.
- `frontend/src/hooks/__tests__/useLiveSocket.test.tsx` — start/stop on flag transitions.
- `frontend/src/api/translate.ts` — typed wrapper for `POST /llm/translate`.
- `frontend/src/store/__tests__/use-voice-store.translation.test.ts` — translation slice behavior.
- `frontend/src/components/__tests__/LiveTab.translation.test.tsx` — toggle + select UI.
- `frontend/src/components/__tests__/LiveTranscript.translation.test.tsx` — inline subtitle rendering.

**Frontend — modify:**
- `frontend/src/App.tsx` — call `useLiveSocket()` at root.
- `frontend/src/store/use-voice-store.ts` — add `translation` slice, batch interval.
- `frontend/src/components/LiveTab.tsx` — translation control cluster.
- `frontend/src/components/LiveTranscript.tsx` — render translation under each line.
- `frontend/src/i18n/locales/en.ts` + `zh-CN.ts` — new keys (parity test enforces both).
- `frontend/src/styles/globals.css` — translation styling.

**Backend — create:**
- `backend/actio-core/src/engine/llm_translate.rs` — prompt builder + JSON parser + tests.
- `backend/actio-core/src/api/translate.rs` — `translate_lines` handler + integration test.

**Backend — modify:**
- `backend/actio-core/src/lib.rs` — add `llm_inflight: Arc<tokio::sync::Mutex<()>>` to `AppState`.
- `backend/actio-core/src/engine/mod.rs` — `pub mod llm_translate;`.
- `backend/actio-core/src/engine/llm_router.rs` — add `translate_lines()` method, extend `Stub` variant.
- `backend/actio-core/src/engine/remote_llm_client.rs` — add `translate_lines()` HTTP call.
- `backend/actio-core/src/engine/window_extractor.rs` — acquire `llm_inflight` in production path.
- `backend/actio-core/src/api/mod.rs` — register `POST /llm/translate` route, declare `pub mod translate;`.

---

## Task Order Rationale

Phase A (Tasks 1–2) ships the plumbing fix in isolation so transcripts become visible immediately, even if the rest of the plan is interrupted. Phase B (Tasks 3–9) builds the backend translation pipeline bottom-up: prompt+parser, then router method per variant, then queueing, then HTTP endpoint. Phase C (Tasks 10–14) builds the frontend translation feature on top of the proven backend.

Branch off main into `feat/live-translation`. Commit after every task.

---

# Phase A — Plumbing fix: Live tab actually shows transcripts

### Task 1: `useLiveSocket` hook

**Files:**
- Create: `frontend/src/hooks/useLiveSocket.ts`
- Test: `frontend/src/hooks/__tests__/useLiveSocket.test.tsx`

- [ ] **Step 1: Write the failing test**

```tsx
// frontend/src/hooks/__tests__/useLiveSocket.test.tsx
import { renderHook, act } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useLiveSocket } from '../useLiveSocket';
import { useStore } from '../../store/use-store';
import { useVoiceStore } from '../../store/use-voice-store';

describe('useLiveSocket', () => {
  let startSpy: ReturnType<typeof vi.fn>;
  let stopSpy: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    startSpy = vi.fn();
    stopSpy = vi.fn();
    useVoiceStore.setState({ startRecording: startSpy, stopRecording: stopSpy });
    useStore.setState({ ui: { ...useStore.getState().ui, listeningEnabled: null } });
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('does nothing while listeningEnabled is null (booting)', () => {
    renderHook(() => useLiveSocket());
    expect(startSpy).not.toHaveBeenCalled();
    expect(stopSpy).not.toHaveBeenCalled();
  });

  it('calls startRecording when listeningEnabled flips to true', () => {
    renderHook(() => useLiveSocket());
    act(() => {
      useStore.setState({ ui: { ...useStore.getState().ui, listeningEnabled: true } });
    });
    expect(startSpy).toHaveBeenCalledTimes(1);
  });

  it('calls stopRecording when listeningEnabled flips from true to false', () => {
    useStore.setState({ ui: { ...useStore.getState().ui, listeningEnabled: true } });
    renderHook(() => useLiveSocket());
    startSpy.mockClear();
    act(() => {
      useStore.setState({ ui: { ...useStore.getState().ui, listeningEnabled: false } });
    });
    expect(stopSpy).toHaveBeenCalledTimes(1);
    expect(startSpy).not.toHaveBeenCalled();
  });

  it('calls startRecording on mount if already true (boot-with-on)', () => {
    useStore.setState({ ui: { ...useStore.getState().ui, listeningEnabled: true } });
    renderHook(() => useLiveSocket());
    expect(startSpy).toHaveBeenCalledTimes(1);
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd frontend && pnpm exec vitest run src/hooks/__tests__/useLiveSocket.test.tsx`
Expected: FAIL — `Cannot find module '../useLiveSocket'`.

- [ ] **Step 3: Write the hook**

```ts
// frontend/src/hooks/useLiveSocket.ts
import { useEffect } from 'react';
import { useStore } from '../store/use-store';
import { useVoiceStore } from '../store/use-voice-store';

/** Mirror `ui.listeningEnabled` into the voice store's WS lifecycle.
 *  Mounted once at the app root. Idempotent: startRecording/stopRecording
 *  in use-voice-store both no-op if the WS is already in the target state. */
export function useLiveSocket(): void {
  const listeningEnabled = useStore((s) => s.ui.listeningEnabled);

  useEffect(() => {
    if (listeningEnabled === null) return; // still booting
    if (listeningEnabled) {
      useVoiceStore.getState().startRecording();
    } else {
      useVoiceStore.getState().stopRecording();
    }
  }, [listeningEnabled]);
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cd frontend && pnpm exec vitest run src/hooks/__tests__/useLiveSocket.test.tsx`
Expected: 4 tests pass.

- [ ] **Step 5: Commit**

```bash
git add frontend/src/hooks/useLiveSocket.ts frontend/src/hooks/__tests__/useLiveSocket.test.tsx
git commit -m "feat(live): useLiveSocket hook mirrors listeningEnabled into WS lifecycle"
```

---

### Task 2: Mount `useLiveSocket` in `App.tsx`

**Files:**
- Modify: `frontend/src/App.tsx:1-12`

- [ ] **Step 1: Add the import and call**

Edit `frontend/src/App.tsx` lines 1–12. Replace:

```tsx
import { useEffect } from 'react';
import { useStore } from './store/use-store';
import { BoardWindow } from './components/BoardWindow';
import { FeedbackToast } from './components/FeedbackToast';
import { StandbyTray } from './components/StandbyTray';
import { OnboardingCard } from './components/OnboardingCard';
import { NewReminderBar } from './components/NewReminderBar';
import { useGlobalShortcuts } from './hooks/useGlobalShortcuts';
import { advanceWordmarkPreview } from './hooks/useWordmarkPreview';

export default function App() {
  useGlobalShortcuts();
```

with:

```tsx
import { useEffect } from 'react';
import { useStore } from './store/use-store';
import { BoardWindow } from './components/BoardWindow';
import { FeedbackToast } from './components/FeedbackToast';
import { StandbyTray } from './components/StandbyTray';
import { OnboardingCard } from './components/OnboardingCard';
import { NewReminderBar } from './components/NewReminderBar';
import { useGlobalShortcuts } from './hooks/useGlobalShortcuts';
import { useLiveSocket } from './hooks/useLiveSocket';
import { advanceWordmarkPreview } from './hooks/useWordmarkPreview';

export default function App() {
  useGlobalShortcuts();
  useLiveSocket();
```

- [ ] **Step 2: Run typecheck + full test suite**

Run: `cd frontend && pnpm tsc --noEmit && pnpm test --run`
Expected: typecheck clean, all existing tests still pass.

- [ ] **Step 3: Manual verification**

Run: `cd frontend && pnpm dev` (in one terminal) and `cd backend && cargo run --bin actio-asr` (in another). Open the app, ensure Listening is on (mic icon active in tray), open the Live tab, speak. Expected: bubbles render with speaker grouping.

If transcripts still don't appear, check browser console for WS errors and confirm `useStore.getState().ui.listeningEnabled === true`.

- [ ] **Step 4: Commit**

```bash
git add frontend/src/App.tsx
git commit -m "fix(live): mount useLiveSocket so transcripts flow when listening is enabled"
```

---

# Phase B — Backend translation infrastructure

### Task 3: Add `llm_inflight` mutex to `AppState`

**Files:**
- Modify: `backend/actio-core/src/lib.rs:60-150` (AppState struct + constructor)

- [ ] **Step 1: Locate AppState struct**

Run: `grep -n "pub struct AppState\|pub router:\|pub pool:" backend/actio-core/src/lib.rs | head -10`. The struct is around line 60–80. The mutex needs to be added as a field and initialized wherever AppState is built (likely the bootstrap fn around line 100–150).

- [ ] **Step 2: Add the field**

In `backend/actio-core/src/lib.rs`, find the `pub struct AppState` declaration and add a new field after `pub router`:

```rust
/// Process-level mutex serializing all `LlmRouter` calls (window
/// extraction + translation). Tokio's mutex is FIFO so neither
/// feature starves the other.
pub llm_inflight: std::sync::Arc<tokio::sync::Mutex<()>>,
```

- [ ] **Step 3: Initialize it**

Find where `AppState { ... }` is constructed (search for `router,` followed by other fields). Add `llm_inflight: std::sync::Arc::new(tokio::sync::Mutex::new(())),` to the struct literal next to `router`. There may be multiple build sites (bootstrap + tests); add it everywhere the compiler complains.

- [ ] **Step 4: cargo check**

Run: `cd backend && cargo check -p actio-core --lib`
Expected: clean. If failures point to test helpers building AppState, add the field there too.

- [ ] **Step 5: Commit**

```bash
git add backend/actio-core/src/lib.rs
git commit -m "feat(backend): add llm_inflight mutex on AppState"
```

---

### Task 4: `llm_translate.rs` — prompt builder + parser

**Files:**
- Create: `backend/actio-core/src/engine/llm_translate.rs`
- Modify: `backend/actio-core/src/engine/mod.rs` (add `pub mod llm_translate;`)

- [ ] **Step 1: Write the file with inline tests (TDD-in-one-file)**

Create `backend/actio-core/src/engine/llm_translate.rs`:

```rust
//! Translation prompt + response parsing for `LlmRouter::translate_lines`.
//!
//! We send the LLM a JSON array of `{id, text}` and expect back a JSON
//! array of `{id, text}` with translations. Order preservation is asked
//! for in the prompt but not relied on — we map by id.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::engine::llm_prompt::ChatMessage;

#[derive(Debug, Clone, Serialize)]
pub struct TranslateLineRequest {
    pub id: Uuid,
    pub text: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TranslateLineResponse {
    pub id: Uuid,
    pub text: String,
}

#[derive(Debug, Clone, Deserialize)]
struct TranslateBatchEnvelope {
    translations: Vec<TranslateLineResponse>,
}

const SYSTEM_PROMPT: &str = "\
You are a translation assistant. You will receive a JSON array of \
transcript lines, each with an `id` and `text`. Translate each `text` \
into the requested target language. If a line is already in the \
target language, return it verbatim. Preserve speaker tone and \
punctuation. Do not add commentary or notes.\n\
\n\
Output ONLY a single JSON object — no markdown, no fences, no \
explanation:\n\
{\"translations\": [{\"id\": \"...uuid...\", \"text\": \"...\"}, ...]}\n\
\n\
The `translations` array MUST contain one entry per input id, in the \
same order. Do not omit, merge, or split lines.";

pub fn build_translate_messages(
    target_lang: &str,
    lines: &[TranslateLineRequest],
) -> Vec<ChatMessage> {
    let lines_json = serde_json::to_string(lines).expect("Vec<TranslateLineRequest> serialises");
    let user = format!("Target language: {target_lang}\n\nLines:\n{lines_json}");
    vec![
        ChatMessage {
            role: "system".into(),
            content: SYSTEM_PROMPT.to_string(),
        },
        ChatMessage {
            role: "user".into(),
            content: user,
        },
    ]
}

/// Parse the LLM response. Tolerates `</think>`-style preambles and
/// prompt-echoed JSON by walking all balanced `{...}` blocks and
/// keeping the LAST one that contains a valid `translations` array.
pub fn parse_translate_response(
    raw: &str,
) -> Result<Vec<TranslateLineResponse>, TranslateParseError> {
    let stripped = strip_think_tags(raw);

    // Walk every balanced {...} block; remember the last that parses
    // into our envelope shape.
    let mut best: Option<Vec<TranslateLineResponse>> = None;
    let mut depth: i32 = 0;
    let mut start: Option<usize> = None;
    for (i, ch) in stripped.char_indices() {
        match ch {
            '{' => {
                if depth == 0 {
                    start = Some(i);
                }
                depth += 1;
            }
            '}' => {
                depth -= 1;
                if depth == 0 {
                    if let Some(s) = start {
                        let candidate = &stripped[s..=i];
                        if let Ok(env) =
                            serde_json::from_str::<TranslateBatchEnvelope>(candidate)
                        {
                            best = Some(env.translations);
                        }
                    }
                    start = None;
                }
            }
            _ => {}
        }
    }

    best.ok_or(TranslateParseError::NoTranslationsFound)
}

fn strip_think_tags(raw: &str) -> String {
    // Mirror engine::llm_router::strip_think_tags semantics: drop a
    // <think>...</think> block (or trailing </think> orphan from some
    // models), keep everything else.
    if let Some(end) = raw.rfind("</think>") {
        return raw[end + "</think>".len()..].trim_start().to_string();
    }
    raw.to_string()
}

#[derive(Debug, thiserror::Error)]
pub enum TranslateParseError {
    #[error("no `translations` array found in LLM response")]
    NoTranslationsFound,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_messages_includes_target_lang_and_lines_json() {
        let lines = vec![
            TranslateLineRequest {
                id: Uuid::nil(),
                text: "hello".into(),
            },
        ];
        let msgs = build_translate_messages("zh-CN", &lines);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "system");
        assert_eq!(msgs[1].role, "user");
        assert!(msgs[1].content.contains("Target language: zh-CN"));
        assert!(msgs[1].content.contains("\"hello\""));
        assert!(msgs[1].content.contains(&Uuid::nil().to_string()));
    }

    #[test]
    fn parse_canonical_response() {
        let raw = r#"{"translations":[{"id":"00000000-0000-0000-0000-000000000001","text":"你好"}]}"#;
        let out = parse_translate_response(raw).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].text, "你好");
    }

    #[test]
    fn parse_with_think_tag_preamble() {
        let raw = r#"<think>let me think</think>{"translations":[{"id":"00000000-0000-0000-0000-000000000001","text":"你好"}]}"#;
        let out = parse_translate_response(raw).unwrap();
        assert_eq!(out[0].text, "你好");
    }

    #[test]
    fn parse_with_prose_preamble() {
        let raw = "Sure, here are the translations:\n\n{\"translations\":[{\"id\":\"00000000-0000-0000-0000-000000000001\",\"text\":\"你好\"}]}";
        let out = parse_translate_response(raw).unwrap();
        assert_eq!(out[0].text, "你好");
    }

    #[test]
    fn parse_picks_last_block_when_prompt_is_echoed() {
        let raw = "Example: {\"translations\":[{\"id\":\"00000000-0000-0000-0000-000000000000\",\"text\":\"example\"}]}\nActual: {\"translations\":[{\"id\":\"00000000-0000-0000-0000-000000000001\",\"text\":\"real\"}]}";
        let out = parse_translate_response(raw).unwrap();
        assert_eq!(out[0].text, "real");
    }

    #[test]
    fn parse_errors_when_empty() {
        let err = parse_translate_response("totally unrelated text").unwrap_err();
        assert!(matches!(err, TranslateParseError::NoTranslationsFound));
    }
}
```

- [ ] **Step 2: Register the module**

Edit `backend/actio-core/src/engine/mod.rs`. Find the existing `pub mod llm_*;` declarations and add:

```rust
pub mod llm_translate;
```

(alphabetically near `pub mod llm_prompt;`)

- [ ] **Step 3: Run the inline tests**

Run: `cd backend && cargo test -p actio-core --lib engine::llm_translate`
Expected: 5 tests pass.

- [ ] **Step 4: Commit**

```bash
git add backend/actio-core/src/engine/llm_translate.rs backend/actio-core/src/engine/mod.rs
git commit -m "feat(backend): llm_translate prompt builder + tolerant response parser"
```

---

### Task 5: Extend `LlmRouter::Stub` to hold optional translations

**Files:**
- Modify: `backend/actio-core/src/engine/llm_router.rs:39-69` (Stub variant + constructor)

- [ ] **Step 1: Update the Stub variant**

In `backend/actio-core/src/engine/llm_router.rs`, replace:

```rust
    /// Test-only variant that returns a fixed list of action items from
    /// `generate_action_items_with_refs` and an empty list from
    /// `generate_todos`. Lets integration tests exercise the windowed
    /// extraction path without a live LLM backend.
    #[cfg(test)]
    Stub {
        action_items: Vec<LlmActionItem>,
    },
}
```

with:

```rust
    /// Test-only variant for integration tests. `action_items` feeds
    /// `generate_action_items_with_refs`; `translation_suffix` is
    /// appended to each input line by `translate_lines` (e.g. "[zh]")
    /// so order-preservation and id-mapping can be asserted.
    #[cfg(test)]
    Stub {
        action_items: Vec<LlmActionItem>,
        translation_suffix: String,
    },
}
```

- [ ] **Step 2: Update the test constructor**

Find `pub fn stub(...)` and replace:

```rust
    /// Test-only constructor for the Stub variant.
    #[cfg(test)]
    pub fn stub(action_items: Vec<LlmActionItem>) -> Self {
        LlmRouter::Stub { action_items }
    }
```

with:

```rust
    /// Test-only constructor for the Stub variant.
    #[cfg(test)]
    pub fn stub(action_items: Vec<LlmActionItem>) -> Self {
        LlmRouter::Stub {
            action_items,
            translation_suffix: " [stub]".into(),
        }
    }

    /// Test-only constructor when translation behavior matters.
    #[cfg(test)]
    pub fn stub_with_translation_suffix(suffix: impl Into<String>) -> Self {
        LlmRouter::Stub {
            action_items: vec![],
            translation_suffix: suffix.into(),
        }
    }
```

- [ ] **Step 3: Update existing match arms**

Search the file for `LlmRouter::Stub { action_items }` and `LlmRouter::Stub { .. }`. Existing arms that destructure `action_items` keep working. Existing `Stub { .. }` arms also keep working (the new field is matched by `..`).

- [ ] **Step 4: cargo check**

Run: `cd backend && cargo check -p actio-core --tests`
Expected: clean. If `window_extractor.rs` tests fail because `LlmRouter::Stub { action_items }` is now incomplete, change those destructures to `LlmRouter::Stub { action_items, .. }`.

- [ ] **Step 5: Commit**

```bash
git add backend/actio-core/src/engine/llm_router.rs
git commit -m "test(llm_router): extend Stub with translation_suffix for translate tests"
```

---

### Task 6: `LlmRouter::translate_lines` — Disabled + Stub variants (TDD)

**Files:**
- Modify: `backend/actio-core/src/engine/llm_router.rs` (add method + tests)

- [ ] **Step 1: Write the failing tests**

At the bottom of `backend/actio-core/src/engine/llm_router.rs`, find the existing `#[cfg(test)] mod tests { ... }` block. Add inside it:

```rust
    use crate::engine::llm_translate::TranslateLineRequest;

    #[tokio::test]
    async fn translate_lines_disabled_returns_disabled_error() {
        let router = LlmRouter::Disabled;
        let lines = vec![TranslateLineRequest {
            id: uuid::Uuid::nil(),
            text: "hello".into(),
        }];
        let err = router.translate_lines("zh-CN", lines).await.unwrap_err();
        assert!(matches!(err, LlmRouterError::Disabled));
    }

    #[tokio::test]
    async fn translate_lines_stub_appends_suffix_in_order() {
        let router = LlmRouter::stub_with_translation_suffix(" [zh]");
        let id1 = uuid::Uuid::new_v4();
        let id2 = uuid::Uuid::new_v4();
        let lines = vec![
            TranslateLineRequest { id: id1, text: "first".into() },
            TranslateLineRequest { id: id2, text: "second".into() },
        ];
        let out = router.translate_lines("zh-CN", lines).await.unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].id, id1);
        assert_eq!(out[0].text, "first [zh]");
        assert_eq!(out[1].id, id2);
        assert_eq!(out[1].text, "second [zh]");
    }
```

- [ ] **Step 2: Run the failing tests**

Run: `cd backend && cargo test -p actio-core --lib engine::llm_router::tests::translate_lines`
Expected: FAIL — `translate_lines` does not exist on `LlmRouter`.

- [ ] **Step 3: Implement the Disabled + Stub branches**

In `backend/actio-core/src/engine/llm_router.rs`, after `generate_action_items_with_refs` (around line 196), add:

```rust
    /// Translate each input line to `target_lang`. Returns translations
    /// in the same order as the input. The Local and Remote backends
    /// dispatch to a structured prompt that returns a JSON envelope;
    /// see `engine::llm_translate`.
    pub async fn translate_lines(
        &self,
        target_lang: &str,
        lines: Vec<crate::engine::llm_translate::TranslateLineRequest>,
    ) -> Result<Vec<crate::engine::llm_translate::TranslateLineResponse>, LlmRouterError> {
        match self {
            LlmRouter::Disabled => Err(LlmRouterError::Disabled),
            #[cfg(test)]
            LlmRouter::Stub { translation_suffix, .. } => Ok(lines
                .into_iter()
                .map(|l| crate::engine::llm_translate::TranslateLineResponse {
                    id: l.id,
                    text: format!("{}{}", l.text, translation_suffix),
                })
                .collect()),
            LlmRouter::Remote(_) => {
                // Implemented in Task 7
                Err(LlmRouterError::Parse("translate_lines remote not yet implemented".into()))
            }
            LlmRouter::Local { .. } => {
                // Implemented in Task 8
                Err(LlmRouterError::Parse("translate_lines local not yet implemented".into()))
            }
        }
    }
```

Note `target_lang` is used by Remote/Local in later tasks; we accept it now to lock the signature. Add `#[allow(unused_variables)]` if clippy complains during this task.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd backend && cargo test -p actio-core --lib engine::llm_router::tests::translate_lines`
Expected: 2 tests pass.

- [ ] **Step 5: Commit**

```bash
git add backend/actio-core/src/engine/llm_router.rs
git commit -m "feat(llm_router): translate_lines for Disabled + Stub variants"
```

---

### Task 7: `LlmRouter::translate_lines` — Remote variant

**Files:**
- Modify: `backend/actio-core/src/engine/remote_llm_client.rs` (add method)
- Modify: `backend/actio-core/src/engine/llm_router.rs` (wire Remote arm)

- [ ] **Step 1: Add `translate_lines` to `RemoteLlmClient`**

In `backend/actio-core/src/engine/remote_llm_client.rs`, after the existing `generate_action_items_with_refs` method (around line 343), add:

```rust
    /// Batch-translate finalized transcript lines to `target_lang`.
    pub async fn translate_lines(
        &self,
        target_lang: &str,
        lines: Vec<crate::engine::llm_translate::TranslateLineRequest>,
    ) -> Result<Vec<crate::engine::llm_translate::TranslateLineResponse>, RemoteLlmError> {
        use tracing::info;

        info!(
            target_lang = %target_lang,
            line_count = lines.len(),
            "Calling remote LLM for translation"
        );

        let messages = crate::engine::llm_translate::build_translate_messages(target_lang, &lines);
        let openai_messages: Vec<serde_json::Value> = messages
            .iter()
            .map(|m| serde_json::json!({"role": m.role, "content": m.content}))
            .collect();

        let payload = serde_json::json!({
            "model": self.config.model,
            "messages": openai_messages,
            "response_format": {"type": "json_object"},
            "temperature": 0.1,
            "max_tokens": 2000,
        });

        let base = self.config.base_url.trim_end_matches('/');
        let url = format!("{base}/chat/completions");

        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.config.api_key)
            .json(&payload)
            .send()
            .await?;

        let chat_resp: LlmChatResponse = resp.json().await?;
        let content = chat_resp
            .choices
            .first()
            .map(|c| &c.message.content)
            .ok_or(RemoteLlmError::InvalidResponse)?;
        tracing::info!(raw_json = %content, "Remote LLM translate raw response");

        let parsed = crate::engine::llm_translate::parse_translate_response(content)
            .map_err(|e| RemoteLlmError::Parse(e.into()))?;
        Ok(parsed)
    }
```

- [ ] **Step 2: Wire the Remote arm in `LlmRouter::translate_lines`**

In `backend/actio-core/src/engine/llm_router.rs`, replace:

```rust
            LlmRouter::Remote(_) => {
                // Implemented in Task 7
                Err(LlmRouterError::Parse("translate_lines remote not yet implemented".into()))
            }
```

with:

```rust
            LlmRouter::Remote(client) => client
                .translate_lines(target_lang, lines)
                .await
                .map_err(LlmRouterError::Remote),
```

- [ ] **Step 3: cargo check + run all router tests**

Run: `cd backend && cargo check -p actio-core --lib && cargo test -p actio-core --lib engine::llm_router`
Expected: clean compile, all router tests still pass (Remote path has no test in this task — exercised end-to-end via the HTTP integration test in Task 9).

- [ ] **Step 4: Commit**

```bash
git add backend/actio-core/src/engine/remote_llm_client.rs backend/actio-core/src/engine/llm_router.rs
git commit -m "feat(llm_router): translate_lines Remote variant via OpenAI-compat client"
```

---

### Task 8: `LlmRouter::translate_lines` — Local variant

**Files:**
- Modify: `backend/actio-core/src/engine/llm_router.rs` (Local arm)

- [ ] **Step 1: Wire the Local arm**

In `backend/actio-core/src/engine/llm_router.rs`, replace:

```rust
            LlmRouter::Local { .. } => {
                // Implemented in Task 8
                Err(LlmRouterError::Parse("translate_lines local not yet implemented".into()))
            }
```

with:

```rust
            LlmRouter::Local { slot, model_id } => {
                let engine = slot
                    .get_or_load(model_id)
                    .await
                    .map_err(LlmRouterError::Local)?;
                let total_chars: usize = lines.iter().map(|l| l.text.len()).sum();
                let params = GenerationParams {
                    max_tokens: 2000,
                    temperature: 0.1,
                    json_mode: true,
                    thinking_budget: Some((total_chars / 10).clamp(100, 500)),
                };
                let messages =
                    crate::engine::llm_translate::build_translate_messages(target_lang, &lines);
                let json = engine
                    .chat_completion(messages, params, EnginePriority::Internal)
                    .await
                    .map_err(LlmRouterError::Local)?;
                tracing::info!(raw_json = %json, "Local LLM translate raw response");
                let parsed = crate::engine::llm_translate::parse_translate_response(&json)
                    .map_err(|e| LlmRouterError::Parse(e.to_string()))?;
                Ok(parsed)
            }
```

- [ ] **Step 2: cargo check**

Run: `cd backend && cargo check -p actio-core --lib`
Expected: clean. (Local path has no unit test — verified manually with a real model in Task 15.)

- [ ] **Step 3: Commit**

```bash
git add backend/actio-core/src/engine/llm_router.rs
git commit -m "feat(llm_router): translate_lines Local variant via llama-cpp engine"
```

---

### Task 9: `POST /llm/translate` endpoint + integration test

**Files:**
- Create: `backend/actio-core/src/api/translate.rs`
- Modify: `backend/actio-core/src/api/mod.rs` (declare module + register route)
- Modify: `backend/actio-core/src/engine/window_extractor.rs` (acquire `llm_inflight`)

- [ ] **Step 1: Create the endpoint module**

Create `backend/actio-core/src/api/translate.rs`:

```rust
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::session::AppApiError;
use crate::engine::llm_router::LlmRouterError;
use crate::engine::llm_translate::TranslateLineRequest;
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct TranslateRequest {
    pub target_lang: String,
    pub lines: Vec<TranslateLineRequestWire>,
}

#[derive(Debug, Deserialize)]
pub struct TranslateLineRequestWire {
    pub id: Uuid,
    pub text: String,
}

#[derive(Debug, Serialize)]
pub struct TranslateResponse {
    pub translations: Vec<TranslateLineWire>,
}

#[derive(Debug, Serialize)]
pub struct TranslateLineWire {
    pub id: Uuid,
    pub text: String,
}

/// POST /llm/translate — batch-translate transcript lines.
///
/// Returns 503 with `{"error":"llm_disabled"}` when the router is in
/// `Disabled` mode so the frontend can surface a precise toast.
pub async fn translate_lines(
    State(state): State<AppState>,
    Json(req): Json<TranslateRequest>,
) -> Result<Json<TranslateResponse>, Response> {
    if req.lines.is_empty() {
        return Ok(Json(TranslateResponse { translations: vec![] }));
    }

    let lines: Vec<TranslateLineRequest> = req
        .lines
        .into_iter()
        .map(|l| TranslateLineRequest {
            id: l.id,
            text: l.text,
        })
        .collect();

    // Serialize against window-extractor LLM calls. Fair FIFO mutex:
    // a translation queued behind a long extraction call simply waits
    // (and vice versa).
    let _guard = state.llm_inflight.lock().await;

    let router = state.router.read().await;
    let result = router.translate_lines(&req.target_lang, lines).await;
    drop(router);
    drop(_guard);

    match result {
        Ok(translations) => Ok(Json(TranslateResponse {
            translations: translations
                .into_iter()
                .map(|t| TranslateLineWire {
                    id: t.id,
                    text: t.text,
                })
                .collect(),
        })),
        Err(LlmRouterError::Disabled) => Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "llm_disabled"})),
        )
            .into_response()),
        Err(other) => Err(AppApiError::Internal(other.to_string()).into_response()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::llm_router::LlmRouter;
    use axum::body::to_bytes;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    async fn build_test_state(router: LlmRouter) -> AppState {
        // The crate already exposes a test helper for AppState
        // construction via the lib.rs test module. If not, this test
        // can be skipped and replaced with a router-only test.
        crate::test_support::build_test_app_state(router).await
    }

    #[tokio::test]
    async fn returns_503_when_router_disabled() {
        let state = build_test_state(LlmRouter::Disabled).await;
        let app = axum::Router::new()
            .route("/llm/translate", axum::routing::post(translate_lines))
            .with_state(state);

        let req = Request::builder()
            .method("POST")
            .uri("/llm/translate")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                serde_json::json!({
                    "target_lang": "zh-CN",
                    "lines": [{"id": "00000000-0000-0000-0000-000000000001", "text": "hello"}]
                })
                .to_string(),
            ))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = to_bytes(resp.into_body(), 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "llm_disabled");
    }

    #[tokio::test]
    async fn returns_translations_for_stub_router() {
        let state = build_test_state(LlmRouter::stub_with_translation_suffix(" [zh]")).await;
        let app = axum::Router::new()
            .route("/llm/translate", axum::routing::post(translate_lines))
            .with_state(state);

        let req = Request::builder()
            .method("POST")
            .uri("/llm/translate")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                serde_json::json!({
                    "target_lang": "zh-CN",
                    "lines": [
                        {"id": "00000000-0000-0000-0000-000000000001", "text": "first"},
                        {"id": "00000000-0000-0000-0000-000000000002", "text": "second"}
                    ]
                })
                .to_string(),
            ))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["translations"][0]["text"], "first [zh]");
        assert_eq!(json["translations"][1]["text"], "second [zh]");
    }
}
```

- [ ] **Step 2: Confirm or create the `test_support` helper**

Run: `grep -n "test_support\|build_test_app_state" backend/actio-core/src/lib.rs backend/actio-core/src/**/*.rs 2>/dev/null | head -10`.

If `test_support::build_test_app_state` does not exist, find how `window_extractor.rs` integration tests build `AppState` (search `grep -n "AppState {" backend/actio-core/src/engine/window_extractor.rs`) and either:
- Add a `pub mod test_support` in `lib.rs` exposing a helper, OR
- Inline the AppState build into the test functions in `translate.rs` mirroring whatever pattern `window_extractor.rs` tests use.

Whichever path, the helper must accept a `LlmRouter` and return a fully-populated `AppState` with an in-memory SQLite pool, dummy aggregator, and a fresh `llm_inflight` mutex.

- [ ] **Step 3: Register the route**

In `backend/actio-core/src/api/mod.rs`:

1. Near the top with other module declarations, add `pub mod translate;`.
2. Inside `pub fn router(state: AppState) -> Router`, after the `// settings / local llm` block (around line 174), before `.with_state(state)`, add:

```rust
        // translation
        .route("/llm/translate", post(translate::translate_lines))
```

- [ ] **Step 4: Acquire `llm_inflight` in window_extractor production path**

In `backend/actio-core/src/engine/window_extractor.rs`, find `process_window` (the AppState wrapper around `process_window_with`). Wrap the call in a mutex acquisition:

```rust
async fn process_window(state: &AppState, window: &window_repo::ExtractionWindow) -> Result<ProcessOutcome, ProcessError> {
    let _guard = state.llm_inflight.lock().await;
    let router = state.router.read().await;
    process_window_with(&state.pool, &router, window).await
}
```

The integration tests that call `process_window_with` directly are unaffected (they don't touch `llm_inflight`).

- [ ] **Step 5: Run the new tests + full backend test suite**

Run: `cd backend && cargo test -p actio-core --lib api::translate`
Expected: 2 tests pass.

Run: `cd backend && cargo test -p actio-core --lib`
Expected: all 115+ tests pass (no regressions).

- [ ] **Step 6: Commit**

```bash
git add backend/actio-core/src/api/translate.rs backend/actio-core/src/api/mod.rs backend/actio-core/src/engine/window_extractor.rs backend/actio-core/src/lib.rs
git commit -m "feat(api): POST /llm/translate with llm_inflight serialization"
```

---

# Phase C — Frontend translation

### Task 10: i18n keys (en + zh-CN parity)

**Files:**
- Modify: `frontend/src/i18n/locales/en.ts`
- Modify: `frontend/src/i18n/locales/zh-CN.ts`

- [ ] **Step 1: Add keys to en.ts**

Find the `// Live transcript` block (around line 28–30) in `frontend/src/i18n/locales/en.ts`. After `'transcript.unknown': 'Unknown',` add:

```ts
  'transcript.translating': '·· translating',
  'transcript.translateError': 'retry',

  // Live tab — translation controls
  'live.translate.toggle': 'Translate',
  'live.translate.targetLabel': 'Target language',
  'live.translate.disabledTooltip': 'Enable an LLM in Settings → AI to use translation.',
  'live.translate.pausedToast': 'Translation paused — LLM is disabled.',
  'live.translate.lang.en': 'English',
  'live.translate.lang.zh-CN': '简体中文',
  'live.translate.lang.ja': '日本語',
  'live.translate.lang.es': 'Español',
  'live.translate.lang.fr': 'Français',
  'live.translate.lang.de': 'Deutsch',
```

- [ ] **Step 2: Add the SAME keys to zh-CN.ts**

Open `frontend/src/i18n/locales/zh-CN.ts`. Find the corresponding `transcript.unknown` line and add (matching the same insertion pattern):

```ts
  'transcript.translating': '·· 翻译中',
  'transcript.translateError': '重试',

  // Live tab — translation controls
  'live.translate.toggle': '翻译',
  'live.translate.targetLabel': '目标语言',
  'live.translate.disabledTooltip': '请在设置 → AI 中启用大语言模型后再使用翻译。',
  'live.translate.pausedToast': '翻译已暂停 — 大语言模型已禁用。',
  'live.translate.lang.en': 'English',
  'live.translate.lang.zh-CN': '简体中文',
  'live.translate.lang.ja': '日本語',
  'live.translate.lang.es': 'Español',
  'live.translate.lang.fr': 'Français',
  'live.translate.lang.de': 'Deutsch',
```

- [ ] **Step 3: Run the parity test**

Run: `cd frontend && pnpm exec vitest run src/i18n/__tests__/parity.test.ts`
Expected: 2 tests pass (parity holds; all values non-empty).

- [ ] **Step 4: Commit**

```bash
git add frontend/src/i18n/locales/en.ts frontend/src/i18n/locales/zh-CN.ts
git commit -m "i18n: translation control keys (en + zh-CN)"
```

---

### Task 11: `api/translate.ts` — typed wrapper

**Files:**
- Create: `frontend/src/api/translate.ts`

- [ ] **Step 1: Create the wrapper**

```ts
// frontend/src/api/translate.ts
import { getApiUrl } from './backend-url';

export interface TranslateLineRequest {
  id: string;
  text: string;
}

export interface TranslateLineResponse {
  id: string;
  text: string;
}

export class LlmDisabledError extends Error {
  constructor() {
    super('llm_disabled');
    this.name = 'LlmDisabledError';
  }
}

/** POST /llm/translate. Throws `LlmDisabledError` on 503 so callers
 *  can flip the toggle off and surface a precise toast. */
export async function translateLines(
  targetLang: string,
  lines: TranslateLineRequest[],
): Promise<TranslateLineResponse[]> {
  const url = await getApiUrl('/llm/translate');
  const resp = await fetch(url, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ target_lang: targetLang, lines }),
  });
  if (resp.status === 503) {
    const body = await resp.json().catch(() => ({}));
    if (body?.error === 'llm_disabled') throw new LlmDisabledError();
    throw new Error(`translate failed: ${resp.status}`);
  }
  if (!resp.ok) {
    throw new Error(`translate failed: ${resp.status}`);
  }
  const data = (await resp.json()) as { translations: TranslateLineResponse[] };
  return data.translations;
}
```

If `getApiUrl` is named differently in `frontend/src/api/backend-url.ts`, run `grep -n "export\|getApiUrl\|getWsUrl" frontend/src/api/backend-url.ts` and use the existing HTTP helper (likely `getApiUrl` is what you want; `getWsUrl` is for WS only).

- [ ] **Step 2: Typecheck**

Run: `cd frontend && pnpm tsc --noEmit`
Expected: clean.

- [ ] **Step 3: Commit**

```bash
git add frontend/src/api/translate.ts
git commit -m "feat(api): translateLines client wrapper with LlmDisabledError"
```

---

### Task 12: Translation slice in `use-voice-store.ts`

**Files:**
- Modify: `frontend/src/store/use-voice-store.ts`
- Create: `frontend/src/store/__tests__/use-voice-store.translation.test.ts`

- [ ] **Step 1: Write the failing tests**

Create `frontend/src/store/__tests__/use-voice-store.translation.test.ts`:

```ts
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useVoiceStore } from '../use-voice-store';
import type { TranscriptLine } from '../use-voice-store';
import * as translateApi from '../../api/translate';

function mkLine(id: string, text: string, isFinal = true): TranscriptLine {
  return {
    id,
    text,
    start_ms: 0,
    end_ms: 0,
    speaker_id: null,
    resolved: true,
    is_final: isFinal,
  };
}

beforeEach(() => {
  // Reset translation slice and start with a session containing 3 finalized lines.
  useVoiceStore.setState({
    isRecording: true,
    currentSession: {
      id: 'live',
      startedAt: new Date().toISOString(),
      lines: [mkLine('a', 'hello'), mkLine('b', 'world'), mkLine('c', 'partial', false)],
      pendingPartial: null,
      pipelineReady: true,
    },
    translation: {
      enabled: false,
      targetLang: 'en',
      byLineId: {},
    },
  });
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe('translation slice', () => {
  it('setTranslationEnabled(true) backfills only finalized lines', async () => {
    const spy = vi
      .spyOn(translateApi, 'translateLines')
      .mockResolvedValue([
        { id: 'a', text: 'HELLO' },
        { id: 'b', text: 'WORLD' },
      ]);
    await useVoiceStore.getState().setTranslationEnabled(true);
    expect(spy).toHaveBeenCalledTimes(1);
    expect(spy.mock.calls[0]?.[1].map((l) => l.id)).toEqual(['a', 'b']);
    const t = useVoiceStore.getState().translation;
    expect(t.enabled).toBe(true);
    expect(t.byLineId['a']).toEqual({ status: 'done', text: 'HELLO' });
    expect(t.byLineId['b']).toEqual({ status: 'done', text: 'WORLD' });
    expect(t.byLineId['c']).toBeUndefined();
  });

  it('setTranslationTargetLang clears byLineId and re-batches', async () => {
    useVoiceStore.setState({
      translation: {
        enabled: true,
        targetLang: 'en',
        byLineId: {
          a: { status: 'done', text: 'HELLO' },
        },
      },
    });
    const spy = vi.spyOn(translateApi, 'translateLines').mockResolvedValue([]);
    await useVoiceStore.getState().setTranslationTargetLang('zh-CN');
    const t = useVoiceStore.getState().translation;
    expect(t.targetLang).toBe('zh-CN');
    // byLineId cleared, new pending entries created for finalized lines, then resolved
    expect(spy).toHaveBeenCalledWith('zh-CN', expect.arrayContaining([
      expect.objectContaining({ id: 'a' }),
      expect.objectContaining({ id: 'b' }),
    ]));
  });

  it('queueLineForTranslation only adds when enabled', () => {
    useVoiceStore.getState().queueLineForTranslation('new-id');
    expect(useVoiceStore.getState().translation.byLineId['new-id']).toBeUndefined();

    useVoiceStore.setState({
      translation: { enabled: true, targetLang: 'en', byLineId: {} },
    });
    useVoiceStore.getState().queueLineForTranslation('new-id');
    expect(useVoiceStore.getState().translation.byLineId['new-id']).toEqual({ status: 'pending' });
  });

  it('flushTranslationBatch sends all pending and marks done on success', async () => {
    useVoiceStore.setState({
      translation: {
        enabled: true,
        targetLang: 'en',
        byLineId: {
          a: { status: 'pending' },
          b: { status: 'pending' },
          c: { status: 'done', text: 'cached' },
        },
      },
    });
    vi.spyOn(translateApi, 'translateLines').mockResolvedValue([
      { id: 'a', text: 'A!' },
      { id: 'b', text: 'B!' },
    ]);
    await useVoiceStore.getState().flushTranslationBatch();
    const t = useVoiceStore.getState().translation;
    expect(t.byLineId['a']).toEqual({ status: 'done', text: 'A!' });
    expect(t.byLineId['b']).toEqual({ status: 'done', text: 'B!' });
    expect(t.byLineId['c']).toEqual({ status: 'done', text: 'cached' });
  });

  it('flushTranslationBatch marks pending as error on rejection', async () => {
    useVoiceStore.setState({
      translation: {
        enabled: true,
        targetLang: 'en',
        byLineId: { a: { status: 'pending' } },
      },
    });
    vi.spyOn(translateApi, 'translateLines').mockRejectedValue(new Error('boom'));
    await useVoiceStore.getState().flushTranslationBatch();
    expect(useVoiceStore.getState().translation.byLineId['a']?.status).toBe('error');
  });

  it('LlmDisabledError flips enabled off and clears pending', async () => {
    useVoiceStore.setState({
      translation: {
        enabled: true,
        targetLang: 'en',
        byLineId: { a: { status: 'pending' } },
      },
    });
    vi.spyOn(translateApi, 'translateLines').mockRejectedValue(new translateApi.LlmDisabledError());
    await useVoiceStore.getState().flushTranslationBatch();
    const t = useVoiceStore.getState().translation;
    expect(t.enabled).toBe(false);
    expect(t.byLineId).toEqual({});
  });

  it('stopRecording clears byLineId but keeps targetLang', () => {
    useVoiceStore.setState({
      _ws: null,
      translation: {
        enabled: true,
        targetLang: 'ja',
        byLineId: { a: { status: 'done', text: 'A!' } },
      },
    });
    useVoiceStore.getState().stopRecording();
    const t = useVoiceStore.getState().translation;
    expect(t.byLineId).toEqual({});
    expect(t.targetLang).toBe('ja');
  });
});
```

- [ ] **Step 2: Run the failing tests**

Run: `cd frontend && pnpm exec vitest run src/store/__tests__/use-voice-store.translation.test.ts`
Expected: FAIL — `setTranslationEnabled is not a function`.

- [ ] **Step 3: Add the translation slice to the voice store**

In `frontend/src/store/use-voice-store.ts`:

**3a.** Near the top with other type imports (around line 4), add:

```ts
import { translateLines, LlmDisabledError } from '../api/translate';
```

**3b.** Find the `interface VoiceState` block (around line 41). Add these fields:

```ts
  translation: {
    enabled: boolean;
    targetLang: string;
    byLineId: Record<string, { status: 'pending' | 'done' | 'error'; text?: string }>;
  };

  setTranslationEnabled: (enabled: boolean) => Promise<void>;
  setTranslationTargetLang: (lang: string) => Promise<void>;
  queueLineForTranslation: (lineId: string) => void;
  flushTranslationBatch: () => Promise<void>;
  retryTranslationLine: (lineId: string) => void;
```

**3c.** Find the storage helpers near the top (`STORAGE_KEY`, `loadVoiceData`, etc.). Add a localStorage key + initial loader:

```ts
const TRANSLATE_LANG_KEY = 'actio-translate-target';

function loadTranslateTarget(): string {
  try {
    return localStorage.getItem(TRANSLATE_LANG_KEY) ?? 'en';
  } catch {
    return 'en';
  }
}

function saveTranslateTarget(lang: string): void {
  try {
    localStorage.setItem(TRANSLATE_LANG_KEY, lang);
  } catch { /* private mode etc. */ }
}
```

**3d.** Find the `create<VoiceState>((set, get) => ({ ... }))` body. Add the initial slice next to other initial values (around line 305):

```ts
  translation: {
    enabled: false,
    targetLang: loadTranslateTarget(),
    byLineId: {},
  },
```

**3e.** Inside the same body, after `stopRecording` (around line 349), add the actions:

```ts
  setTranslationEnabled: async (enabled) => {
    set((state) => ({ translation: { ...state.translation, enabled } }));
    if (!enabled) return;
    // Backfill: every finalized line missing a translation goes pending.
    const { currentSession, translation } = get();
    if (!currentSession) return;
    const missing = currentSession.lines
      .filter((l) => l.is_final && !translation.byLineId[l.id])
      .map((l) => ({ id: l.id, text: l.text }));
    if (missing.length === 0) return;
    set((state) => {
      const next = { ...state.translation.byLineId };
      for (const m of missing) next[m.id] = { status: 'pending' };
      return { translation: { ...state.translation, byLineId: next } };
    });
    await get().flushTranslationBatch();
  },

  setTranslationTargetLang: async (lang) => {
    saveTranslateTarget(lang);
    set((state) => ({
      translation: { ...state.translation, targetLang: lang, byLineId: {} },
    }));
    if (get().translation.enabled) {
      const { currentSession } = get();
      if (currentSession) {
        const missing = currentSession.lines
          .filter((l) => l.is_final)
          .map((l) => ({ id: l.id, text: l.text }));
        if (missing.length > 0) {
          set((state) => {
            const next: Record<string, { status: 'pending' | 'done' | 'error'; text?: string }> = {};
            for (const m of missing) next[m.id] = { status: 'pending' };
            return { translation: { ...state.translation, byLineId: next } };
          });
          await get().flushTranslationBatch();
        }
      }
    }
  },

  queueLineForTranslation: (lineId) => {
    if (!get().translation.enabled) return;
    set((state) => {
      if (state.translation.byLineId[lineId]) return state;
      return {
        translation: {
          ...state.translation,
          byLineId: { ...state.translation.byLineId, [lineId]: { status: 'pending' } },
        },
      };
    });
  },

  flushTranslationBatch: async () => {
    const { translation, currentSession } = get();
    if (!translation.enabled || !currentSession) return;
    const pending = Object.entries(translation.byLineId)
      .filter(([, v]) => v.status === 'pending')
      .map(([id]) => id);
    if (pending.length === 0) return;
    const idToText = new Map(currentSession.lines.map((l) => [l.id, l.text] as const));
    const lines = pending
      .map((id) => ({ id, text: idToText.get(id) ?? '' }))
      .filter((l) => l.text);
    if (lines.length === 0) return;
    try {
      const out = await translateLines(translation.targetLang, lines);
      set((state) => {
        const next = { ...state.translation.byLineId };
        for (const t of out) next[t.id] = { status: 'done', text: t.text };
        return { translation: { ...state.translation, byLineId: next } };
      });
    } catch (e) {
      if (e instanceof LlmDisabledError) {
        set({ translation: { ...get().translation, enabled: false, byLineId: {} } });
        return;
      }
      // Mark every pending id we asked about as error.
      const askedIds = new Set(lines.map((l) => l.id));
      set((state) => {
        const next = { ...state.translation.byLineId };
        for (const id of askedIds) {
          if (next[id]?.status === 'pending') next[id] = { status: 'error' };
        }
        return { translation: { ...state.translation, byLineId: next } };
      });
    }
  },

  retryTranslationLine: (lineId) => {
    set((state) => ({
      translation: {
        ...state.translation,
        byLineId: { ...state.translation.byLineId, [lineId]: { status: 'pending' } },
      },
    }));
    void get().flushTranslationBatch();
  },
```

**3f.** Update `stopRecording` (around line 343) to clear `byLineId`:

```ts
  stopRecording: () => {
    const { currentSession, _ws } = get();
    _ws?.close();
    if (currentSession && currentSession.lines.length > 0) get().flushInterval();
    clearPendingResolutionsForSession();
    set((state) => ({
      isRecording: false,
      currentSession: null,
      _ws: null,
      translation: { ...state.translation, byLineId: {} },
    }));
  },
```

**3g.** Update `handleTranscriptMessage` (search for `function handleTranscriptMessage`) so each newly-finalized line is auto-queued. Find the path that pushes into `currentSession.lines` and, immediately after the `set(...)` call, add:

```ts
      // Queue the new final for translation if the toggle is on.
      // Done outside the set to avoid coupling the slice to the line append.
      const ts = useVoiceStore.getState();
      if (ts.translation.enabled && msg.is_final) {
        ts.queueLineForTranslation(msg.transcript_id);
      }
```

(Use whatever the actual final-transcript id is in `handleTranscriptMessage` — `msg.transcript_id` is the canonical wire field per `api/ws.rs`. If the existing handler creates a different local id, queue with that.)

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd frontend && pnpm exec vitest run src/store/__tests__/use-voice-store.translation.test.ts`
Expected: 7 tests pass.

Run: `cd frontend && pnpm test --run`
Expected: every existing test still passes.

- [ ] **Step 5: Commit**

```bash
git add frontend/src/store/use-voice-store.ts frontend/src/store/__tests__/use-voice-store.translation.test.ts
git commit -m "feat(voice-store): translation slice with backfill, queue, and retry"
```

---

### Task 13: 3-second forward batch interval in `useLiveSocket`

**Files:**
- Modify: `frontend/src/hooks/useLiveSocket.ts`
- Modify: `frontend/src/hooks/__tests__/useLiveSocket.test.tsx`

- [ ] **Step 1: Extend the hook**

Replace `frontend/src/hooks/useLiveSocket.ts` with:

```ts
import { useEffect } from 'react';
import { useStore } from '../store/use-store';
import { useVoiceStore } from '../store/use-voice-store';

const TRANSLATE_BATCH_INTERVAL_MS = 3000;

/** Mirror `ui.listeningEnabled` into the voice store's WS lifecycle,
 *  AND drive a 3-second translation flush while listening + translation
 *  are both on. Mounted once at the app root. */
export function useLiveSocket(): void {
  const listeningEnabled = useStore((s) => s.ui.listeningEnabled);
  const translateEnabled = useVoiceStore((s) => s.translation.enabled);
  const hasSession = useVoiceStore((s) => s.currentSession !== null);

  // WS lifecycle.
  useEffect(() => {
    if (listeningEnabled === null) return;
    if (listeningEnabled) {
      useVoiceStore.getState().startRecording();
    } else {
      useVoiceStore.getState().stopRecording();
    }
  }, [listeningEnabled]);

  // Translation flush loop.
  useEffect(() => {
    if (!translateEnabled || !hasSession) return;
    const id = window.setInterval(() => {
      void useVoiceStore.getState().flushTranslationBatch();
    }, TRANSLATE_BATCH_INTERVAL_MS);
    return () => window.clearInterval(id);
  }, [translateEnabled, hasSession]);
}
```

- [ ] **Step 2: Add a test for the interval**

Append to `frontend/src/hooks/__tests__/useLiveSocket.test.tsx` inside the existing `describe`:

```tsx
  it('flushes translations every 3s when translation.enabled and a session exists', () => {
    vi.useFakeTimers();
    const flushSpy = vi.fn();
    useVoiceStore.setState({
      flushTranslationBatch: flushSpy,
      translation: { enabled: true, targetLang: 'en', byLineId: {} },
      currentSession: {
        id: 'live',
        startedAt: '',
        lines: [],
        pendingPartial: null,
        pipelineReady: true,
      },
    });
    renderHook(() => useLiveSocket());
    vi.advanceTimersByTime(3001);
    expect(flushSpy).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(3000);
    expect(flushSpy).toHaveBeenCalledTimes(2);
    vi.useRealTimers();
  });

  it('does not flush when translation.enabled is false', () => {
    vi.useFakeTimers();
    const flushSpy = vi.fn();
    useVoiceStore.setState({
      flushTranslationBatch: flushSpy,
      translation: { enabled: false, targetLang: 'en', byLineId: {} },
      currentSession: {
        id: 'live',
        startedAt: '',
        lines: [],
        pendingPartial: null,
        pipelineReady: true,
      },
    });
    renderHook(() => useLiveSocket());
    vi.advanceTimersByTime(10_000);
    expect(flushSpy).not.toHaveBeenCalled();
    vi.useRealTimers();
  });
```

- [ ] **Step 3: Run the tests**

Run: `cd frontend && pnpm exec vitest run src/hooks/__tests__/useLiveSocket.test.tsx`
Expected: 6 tests pass (4 from Task 1 + 2 new).

- [ ] **Step 4: Commit**

```bash
git add frontend/src/hooks/useLiveSocket.ts frontend/src/hooks/__tests__/useLiveSocket.test.tsx
git commit -m "feat(live): 3-second translation flush interval in useLiveSocket"
```

---

### Task 14: LiveTab translation controls

**Files:**
- Modify: `frontend/src/components/LiveTab.tsx`
- Create: `frontend/src/components/__tests__/LiveTab.translation.test.tsx`
- Modify: `frontend/src/styles/globals.css` (controls + tooltip styling)

- [ ] **Step 1: Write the failing tests**

Create `frontend/src/components/__tests__/LiveTab.translation.test.tsx`:

```tsx
import { describe, expect, it, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { LiveTab } from '../LiveTab';
import { useStore } from '../../store/use-store';
import { useVoiceStore } from '../../store/use-voice-store';

beforeEach(() => {
  useStore.setState({
    ui: { ...useStore.getState().ui, listeningEnabled: true, listeningStartedAt: Date.now() },
  });
  useVoiceStore.setState({
    isRecording: true,
    currentSession: {
      id: 'live',
      startedAt: '',
      lines: [],
      pendingPartial: null,
      pipelineReady: true,
    },
    translation: { enabled: false, targetLang: 'en', byLineId: {} },
  });
});

describe('LiveTab translation controls', () => {
  it('renders the toggle pill and target select', () => {
    render(<LiveTab />);
    expect(screen.getByRole('button', { name: /translate/i })).toBeInTheDocument();
    expect(screen.getByRole('combobox', { name: /target language/i })).toBeInTheDocument();
  });

  it('clicking the toggle calls setTranslationEnabled', () => {
    const spy = vi.fn().mockResolvedValue(undefined);
    useVoiceStore.setState({ setTranslationEnabled: spy });
    render(<LiveTab />);
    fireEvent.click(screen.getByRole('button', { name: /translate/i }));
    expect(spy).toHaveBeenCalledWith(true);
  });

  it('changing the select calls setTranslationTargetLang', () => {
    const spy = vi.fn().mockResolvedValue(undefined);
    useVoiceStore.setState({ setTranslationTargetLang: spy });
    render(<LiveTab />);
    const select = screen.getByRole('combobox', { name: /target language/i });
    fireEvent.change(select, { target: { value: 'zh-CN' } });
    expect(spy).toHaveBeenCalledWith('zh-CN');
  });

  it('select is disabled while toggle is off', () => {
    render(<LiveTab />);
    const select = screen.getByRole('combobox', { name: /target language/i });
    expect(select).toBeDisabled();
  });

  it('select is enabled while toggle is on', () => {
    useVoiceStore.setState({
      translation: { enabled: true, targetLang: 'en', byLineId: {} },
    });
    render(<LiveTab />);
    const select = screen.getByRole('combobox', { name: /target language/i });
    expect(select).toBeEnabled();
  });
});
```

- [ ] **Step 2: Update LiveTab.tsx**

Replace `frontend/src/components/LiveTab.tsx` with:

```tsx
import { useEffect, useRef, useState } from 'react';
import { useStore } from '../store/use-store';
import { useVoiceStore } from '../store/use-voice-store';
import { LiveTranscript } from './LiveTranscript';
import { ListeningToggle } from './ListeningToggle';
import { useT } from '../i18n';

const TRANSLATE_LANGS = ['en', 'zh-CN', 'ja', 'es', 'fr', 'de'] as const;

function formatDuration(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const pad = (n: number) => String(n).padStart(2, '0');
  return h > 0 ? `${pad(h)}:${pad(m)}:${pad(s)}` : `${pad(m)}:${pad(s)}`;
}

function formatTime(ts: number): string {
  return new Date(ts).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
}

export function LiveTab() {
  const listeningEnabled = useStore((s) => s.ui.listeningEnabled);
  const listeningStartedAt = useStore((s) => s.ui.listeningStartedAt);
  const currentSession = useVoiceStore((s) => s.currentSession);
  const translation = useVoiceStore((s) => s.translation);
  const setTranslationEnabled = useVoiceStore((s) => s.setTranslationEnabled);
  const setTranslationTargetLang = useVoiceStore((s) => s.setTranslationTargetLang);
  const t = useT();

  const transcriptRef = useRef<HTMLDivElement>(null);
  const [now, setNow] = useState(Date.now());

  useEffect(() => {
    if (!listeningEnabled || !listeningStartedAt) return;
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [listeningEnabled, listeningStartedAt]);

  useEffect(() => {
    if (transcriptRef.current) {
      transcriptRef.current.scrollTop = transcriptRef.current.scrollHeight;
    }
  }, [currentSession?.lines, currentSession?.pendingPartial]);

  const isOn = listeningEnabled === true;
  const headerLabel = isOn ? t('live.header.on') : t('live.header.off');

  return (
    <div className="live-tab">
      <div className="live-tab__header">
        <span className={`live-tab__status${isOn ? ' is-on' : ''}`}>{headerLabel}</span>
        <div className="live-tab__translate-cluster">
          <button
            type="button"
            className={`live-tab__translate-toggle${translation.enabled ? ' is-on' : ''}`}
            aria-pressed={translation.enabled}
            onClick={() => void setTranslationEnabled(!translation.enabled)}
          >
            {t('live.translate.toggle')}
          </button>
          <select
            className="live-tab__translate-select"
            aria-label={t('live.translate.targetLabel')}
            value={translation.targetLang}
            disabled={!translation.enabled}
            onChange={(e) => void setTranslationTargetLang(e.target.value)}
          >
            {TRANSLATE_LANGS.map((lang) => (
              <option key={lang} value={lang}>
                {t(`live.translate.lang.${lang}` as Parameters<typeof t>[0])}
              </option>
            ))}
          </select>
        </div>
        <ListeningToggle size={32} />
      </div>

      {isOn && listeningStartedAt && (
        <p className="live-tab__since" aria-live="polite">
          {t('live.listeningSince', {
            time: formatTime(listeningStartedAt),
            duration: formatDuration(now - listeningStartedAt),
          })}
        </p>
      )}

      {!isOn && (
        <p className="live-tab__paused-hint">{t('live.pausedHint')}</p>
      )}

      {isOn && currentSession && (
        <div className="live-tab__transcript" ref={transcriptRef} aria-live="polite">
          <LiveTranscript
            lines={currentSession.lines}
            pendingPartial={currentSession.pendingPartial}
          />
        </div>
      )}
    </div>
  );
}
```

- [ ] **Step 3: Add CSS**

Append to `frontend/src/styles/globals.css`:

```css
/* Live tab — translation cluster (sits in the header next to the listening toggle). */
.live-tab__translate-cluster {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  margin-left: auto;
  margin-right: 12px;
}

.live-tab__translate-toggle {
  appearance: none;
  border: 1px solid var(--color-border);
  background: transparent;
  color: var(--color-text-secondary);
  border-radius: 999px;
  padding: 6px 14px;
  height: 32px;
  font-size: 0.78rem;
  font-weight: 600;
  cursor: pointer;
  transition: background 0.16s ease, color 0.16s ease, border-color 0.16s ease;
}

.live-tab__translate-toggle:hover:not(:disabled) {
  background: var(--color-accent-wash);
  color: var(--color-accent-strong);
  border-color: var(--color-accent);
}

.live-tab__translate-toggle.is-on {
  background: var(--color-accent);
  color: white;
  border-color: var(--color-accent);
}

.live-tab__translate-select {
  appearance: none;
  border: 1px solid var(--color-border);
  background: var(--color-surface);
  color: var(--color-text);
  border-radius: 8px;
  padding: 4px 24px 4px 8px;
  height: 32px;
  font-size: 0.78rem;
}

.live-tab__translate-select:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
```

- [ ] **Step 4: Run the tests**

Run: `cd frontend && pnpm exec vitest run src/components/__tests__/LiveTab.translation.test.tsx`
Expected: 5 tests pass.

Run: `cd frontend && pnpm test --run`
Expected: every existing test still passes.

- [ ] **Step 5: Commit**

```bash
git add frontend/src/components/LiveTab.tsx frontend/src/components/__tests__/LiveTab.translation.test.tsx frontend/src/styles/globals.css
git commit -m "feat(live): translation toggle + target language picker"
```

---

### Task 15: LiveTranscript renders translation under each line

**Files:**
- Modify: `frontend/src/components/LiveTranscript.tsx`
- Create: `frontend/src/components/__tests__/LiveTranscript.translation.test.tsx`
- Modify: `frontend/src/styles/globals.css` (per-line styling)

- [ ] **Step 1: Write the failing tests**

Create `frontend/src/components/__tests__/LiveTranscript.translation.test.tsx`:

```tsx
import { describe, expect, it, beforeEach, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { LiveTranscript } from '../LiveTranscript';
import type { TranscriptLine } from '../../store/use-voice-store';
import { useVoiceStore } from '../../store/use-voice-store';

function mkLine(id: string, text: string): TranscriptLine {
  return {
    id,
    text,
    start_ms: 0,
    end_ms: 0,
    speaker_id: null,
    resolved: true,
    is_final: true,
  };
}

beforeEach(() => {
  useVoiceStore.setState({
    speakers: [],
    translation: { enabled: false, targetLang: 'en', byLineId: {} },
  });
});

describe('LiveTranscript translation rendering', () => {
  it('does not render translations when toggle is off', () => {
    useVoiceStore.setState({
      translation: {
        enabled: false,
        targetLang: 'zh-CN',
        byLineId: { a: { status: 'done', text: '你好' } },
      },
    });
    render(<LiveTranscript lines={[mkLine('a', 'hello')]} pendingPartial={null} />);
    expect(screen.queryByText('你好')).not.toBeInTheDocument();
    expect(screen.getByText('hello')).toBeInTheDocument();
  });

  it('renders done translation under the source line when toggle is on', () => {
    useVoiceStore.setState({
      translation: {
        enabled: true,
        targetLang: 'zh-CN',
        byLineId: { a: { status: 'done', text: '你好' } },
      },
    });
    render(<LiveTranscript lines={[mkLine('a', 'hello')]} pendingPartial={null} />);
    expect(screen.getByText('hello')).toBeInTheDocument();
    expect(screen.getByText('你好')).toBeInTheDocument();
  });

  it('renders pending placeholder while translating', () => {
    useVoiceStore.setState({
      translation: {
        enabled: true,
        targetLang: 'zh-CN',
        byLineId: { a: { status: 'pending' } },
      },
    });
    render(<LiveTranscript lines={[mkLine('a', 'hello')]} pendingPartial={null} />);
    expect(screen.getByText(/translating/i)).toBeInTheDocument();
  });

  it('renders error link and retry calls retryTranslationLine', () => {
    const retrySpy = vi.fn();
    useVoiceStore.setState({
      retryTranslationLine: retrySpy,
      translation: {
        enabled: true,
        targetLang: 'zh-CN',
        byLineId: { a: { status: 'error' } },
      },
    });
    render(<LiveTranscript lines={[mkLine('a', 'hello')]} pendingPartial={null} />);
    const retry = screen.getByRole('button', { name: /retry/i });
    fireEvent.click(retry);
    expect(retrySpy).toHaveBeenCalledWith('a');
  });
});
```

- [ ] **Step 2: Run the failing tests**

Run: `cd frontend && pnpm exec vitest run src/components/__tests__/LiveTranscript.translation.test.tsx`
Expected: FAIL — translation text not rendered.

- [ ] **Step 3: Update LiveTranscript.tsx**

Modify `frontend/src/components/LiveTranscript.tsx`:

**3a.** At the top of the file, alongside the existing `useVoiceStore` import (line 3), the store hooks already work — we just need a few more selectors. Replace the existing `import { useVoiceStore } from '../store/use-voice-store';` with the same line (no change), and replace the existing `import type { TranscriptLine } from '../store/use-voice-store';` (line 4) with the same line (no change). The new bits live inside the component.

**3b.** Update the inner per-line rendering. Find the `b.lines.map((l) => (...))` block (around line 146). Replace:

```tsx
                {b.lines.map((l) => (
                  <span key={l.id} className="live-transcript__line">
                    {l.text}
                  </span>
                ))}
```

with:

```tsx
                {b.lines.map((l) => (
                  <TranscriptLineRow key={l.id} line={l} />
                ))}
```

**3c.** Below the existing `SpeakerHeader` component (anywhere in the file, but keeping `LiveTranscript` exported as the default flow), add:

```tsx
function TranscriptLineRow({ line }: { line: TranscriptLine }) {
  const t = useT();
  const enabled = useVoiceStore((s) => s.translation.enabled);
  const entry = useVoiceStore((s) => s.translation.byLineId[line.id]);
  const retry = useVoiceStore((s) => s.retryTranslationLine);

  return (
    <span className="live-transcript__line">
      <span className="live-transcript__line-text">{line.text}</span>
      {enabled && entry && (
        <span
          className={`live-transcript__translation live-transcript__translation--${entry.status}`}
        >
          {entry.status === 'pending' && t('transcript.translating')}
          {entry.status === 'done' && entry.text}
          {entry.status === 'error' && (
            <button
              type="button"
              className="live-transcript__translation-retry"
              onClick={() => retry(line.id)}
            >
              ⚠ {t('transcript.translateError')}
            </button>
          )}
        </span>
      )}
    </span>
  );
}
```

- [ ] **Step 4: CSS for translation rows**

Append to `frontend/src/styles/globals.css`:

```css
.live-transcript__line-text {
  display: inline;
}

.live-transcript__translation {
  display: block;
  font-size: 0.85em;
  color: var(--color-text-tertiary);
  font-style: italic;
  margin-top: 2px;
}

.live-transcript__translation--pending {
  opacity: 0.6;
}

.live-transcript__translation--error {
  color: #b91c1c;
  font-style: normal;
}

.live-transcript__translation-retry {
  background: none;
  border: 0;
  color: inherit;
  font: inherit;
  cursor: pointer;
  padding: 0;
  text-decoration: underline;
}

.live-transcript__translation-retry:hover {
  color: #7f1d1d;
}
```

- [ ] **Step 5: Run the tests**

Run: `cd frontend && pnpm exec vitest run src/components/__tests__/LiveTranscript.translation.test.tsx`
Expected: 4 tests pass.

Run: `cd frontend && pnpm test --run`
Expected: every existing test still passes.

- [ ] **Step 6: Commit**

```bash
git add frontend/src/components/LiveTranscript.tsx frontend/src/components/__tests__/LiveTranscript.translation.test.tsx frontend/src/styles/globals.css
git commit -m "feat(live): inline translation subtitles under each transcript line"
```

---

### Task 16: End-to-end smoke + final verification

**Files:** none modified — verification only.

- [ ] **Step 1: Full backend test suite**

Run: `cd backend && cargo test -p actio-core --lib`
Expected: every test passes (115+ existing + new translate tests).

- [ ] **Step 2: Full frontend test suite + typecheck**

Run: `cd frontend && pnpm tsc --noEmit && pnpm test --run`
Expected: clean typecheck, every test passes.

- [ ] **Step 3: Manual end-to-end test**

Start backend: `cd backend && cargo run --bin actio-asr`.
Start frontend: `cd frontend && pnpm dev`.

Verify in this order:
1. App boot with always-listening on. Open Live tab. Speak. Expect bubbles to render with speaker grouping. (Phase A.)
2. Settings → AI: confirm an LLM is configured (Local or Remote). If `Disabled`, the translation toggle should be visibly disabled — flip it on, verify the cluster greys out and a tooltip appears on hover.
3. With LLM configured, click "Translate" on Live tab. Speak a sentence. Within ~3s, expect a muted italic translation in the user's chosen target language under the source line.
4. Change the target language while toggle is on. Expect the visible translations to clear and re-translate into the new language.
5. Speak in the target language. Expect the LLM to return the line verbatim (per the system prompt rule).
6. Toggle Translate off. Expect translations to disappear; new lines arrive untranslated.
7. Mute (mic toggle off). Expect WS to close, transcript area to clear. Toggle listening back on; new transcripts flow.
8. Run a window-extraction concurrently (let always-listening run for 5+ minutes so a window matures) and confirm translations during that window queue but eventually flush — neither feature stalls forever.

- [ ] **Step 4: Final commit (only if there are stragglers)**

If any uncommitted CSS or doc changes accumulated during verification:

```bash
git add -A
git commit -m "chore: final cleanup for live diarized transcripts + translation"
```

- [ ] **Step 5: Push the branch**

```bash
git push -u origin feat/live-translation
```

Open a PR for review or merge to main per project convention.

---

## Self-Review Checklist (filled out by the plan author)

**Spec coverage:**

| Spec section | Implementing task |
|---|---|
| Plumbing fix — useLiveSocket lifecycle | Tasks 1, 2 |
| Translation `translation` slice (enabled, targetLang, byLineId) | Task 12 |
| Lifecycle rules 1–5 (backfill, target-lang clear, queue on final, stop clears) | Task 12 |
| Forward 3-second batch interval | Task 13 |
| Backend `POST /llm/translate` endpoint | Task 9 |
| `LlmRouter::translate_lines` Disabled / Stub / Remote / Local | Tasks 6, 7, 8 |
| `llm_inflight` mutex serializing translation + window extraction | Tasks 3, 9 |
| Prompt + parse helpers + tests | Task 4 |
| Live tab toggle + target language picker | Task 14 |
| Inline subtitle rendering (pending/done/error/retry) | Task 15 |
| i18n keys (en + zh-CN parity) | Task 10 |
| LlmDisabled handling — auto-flip toggle off | Task 12 (test asserts) |
| Manual smoke verification | Task 16 |

No spec sections are unmapped.

**Placeholder scan:** No "TBD"/"TODO"/"implement later". Every code block contains the actual implementation. Tasks 7 and 8 explicitly leave the Local/Remote arm body to their respective tasks (referenced by number) — those tasks contain the full code.

**Type / signature consistency:**
- `TranslateLineRequest`, `TranslateLineResponse` defined in Task 4 (Rust) and Task 11 (TS), matching shape.
- `LlmRouter::translate_lines(target_lang, lines) -> Result<Vec<TranslateLineResponse>, LlmRouterError>` consistent across Tasks 6, 7, 8, 9.
- Voice-store actions (`setTranslationEnabled`, `setTranslationTargetLang`, `queueLineForTranslation`, `flushTranslationBatch`, `retryTranslationLine`) declared in Task 12 and consumed by Tasks 13 (interval), 14 (toggle/select), 15 (retry button) with matching arity.
- `LlmDisabledError` thrown by `translate.ts` (Task 11), caught in `flushTranslationBatch` (Task 12).
- i18n key naming matches what Task 14 and Task 15 actually reference (`live.translate.*`, `transcript.translating`, `transcript.translateError`).

No drift detected.
