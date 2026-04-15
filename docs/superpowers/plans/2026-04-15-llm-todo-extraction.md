# LLM-Powered Todo Extraction Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire Chat mode in NewReminderBar to extract structured action items via the existing AgentPipeline, show shimmer skeleton cards while processing, and mark AI-generated cards with a badge that clears on interaction.

**Architecture:** New `POST /reminders/extract` backend route constructs an `AgentPipeline` from `AppState` fields and runs `TodoExtractionTask`. Frontend `ChatComposer` calls a new `extractReminders` store action that inserts skeleton placeholders, fires the API call, then swaps in real cards.

**Tech Stack:** Rust (Axum, serde, AgentPipeline), TypeScript (React, Zustand, Framer Motion), CSS (shimmer keyframes)

---

## File Structure

| File | Action | Responsibility |
|------|--------|----------------|
| `frontend/src/types/index.ts` | Modify | Add `isExtracting`, `isAiGenerated` flags to `Reminder` |
| `frontend/src/api/actio-api.ts` | Modify | Add `extractReminders()` API method |
| `frontend/src/store/use-store.ts` | Modify | Add `extractReminders()`, `clearAiGenerated()` actions |
| `frontend/src/components/ChatComposer.tsx` | Modify | Change `handleSubmit` to call `extractReminders` |
| `frontend/src/components/Card.tsx` | Modify | Skeleton variant + AI badge |
| `frontend/src/styles/globals.css` | Modify | Shimmer animation + AI badge styles |
| `backend/actio-core/src/api/reminder.rs` | Modify | Add `extract_reminders` handler |
| `backend/actio-core/src/api/mod.rs` | Modify | Register `/reminders/extract` route |

---

### Task 1: Add Transient Flags to Reminder Type

**Files:**
- Modify: `frontend/src/types/index.ts:21-34`

- [ ] **Step 1: Add `isExtracting` and `isAiGenerated` to the `Reminder` interface**

In `frontend/src/types/index.ts`, add two optional boolean fields to the `Reminder` interface after the existing `isNew` field:

```typescript
export interface Reminder {
  id: string;
  title: string;
  description: string;
  priority: Priority;
  dueTime?: string;
  labels: string[];
  transcript?: string;
  context?: string;
  sourceTime?: string;
  isNew?: boolean;
  isExtracting?: boolean;
  isAiGenerated?: boolean;
  createdAt: string;
  archivedAt: string | null;
}
```

- [ ] **Step 2: Verify TypeScript compiles**

Run: `cd D:/Dev/Actio/frontend && pnpm exec tsc --noEmit`
Expected: No errors (the new fields are optional, so no call sites break).

- [ ] **Step 3: Commit**

```bash
git add frontend/src/types/index.ts
git commit -m "feat(types): add isExtracting and isAiGenerated flags to Reminder"
```

---

### Task 2: Add `extractReminders` API Method

**Files:**
- Modify: `frontend/src/api/actio-api.ts`

- [ ] **Step 1: Add the `extractReminders` method to the API client**

In `frontend/src/api/actio-api.ts`, add this method inside `createActioApiClient()` return object, after `deleteLabel`:

```typescript
async extractReminders(text: string) {
  const reminders = await request<BackendReminderDto[]>('/reminders/extract', {
    method: 'POST',
    body: JSON.stringify({ text }),
  });
  return reminders.map(mapBackendReminder);
},
```

- [ ] **Step 2: Verify TypeScript compiles**

Run: `cd D:/Dev/Actio/frontend && pnpm exec tsc --noEmit`
Expected: No errors.

- [ ] **Step 3: Commit**

```bash
git add frontend/src/api/actio-api.ts
git commit -m "feat(api): add extractReminders method for LLM todo extraction"
```

---

### Task 3: Add Store Actions

**Files:**
- Modify: `frontend/src/store/use-store.ts`

- [ ] **Step 1: Add `extractReminders` and `clearAiGenerated` to the `AppState` interface**

Add these two lines to the `AppState` interface (after the `clearNewFlag` declaration):

```typescript
extractReminders: (text: string) => Promise<void>;
clearAiGenerated: (id: string) => void;
```

- [ ] **Step 2: Implement `extractReminders` action**

Add this implementation inside `create<AppState>((set) => ({`, after `clearNewFlag`:

```typescript
extractReminders: async (text) => {
  // Insert skeleton placeholders
  const placeholderIds = [crypto.randomUUID(), crypto.randomUUID()];
  const placeholders: Reminder[] = placeholderIds.map((id) => ({
    id,
    title: '',
    description: '',
    priority: 'medium' as Priority,
    labels: [],
    isExtracting: true,
    createdAt: new Date().toISOString(),
    archivedAt: null,
  }));
  set((state) => ({ reminders: [...placeholders, ...state.reminders] }));

  try {
    const extracted = await api.extractReminders(text);
    set((state) => ({
      reminders: [
        ...extracted.map((r) => ({ ...r, isNew: true, isAiGenerated: true })),
        ...state.reminders.filter((r) => !placeholderIds.includes(r.id)),
      ],
    }));
    if (extracted.length === 0) {
      pushFeedback(set, 'No action items found in your note');
    } else {
      pushFeedback(set, `Extracted ${extracted.length} reminder${extracted.length > 1 ? 's' : ''}`, 'success');
    }
  } catch {
    // Remove placeholders on failure
    set((state) => ({
      reminders: state.reminders.filter((r) => !placeholderIds.includes(r.id)),
    }));
    pushFeedback(set, "Couldn't extract reminders");
  }
},
```

- [ ] **Step 3: Implement `clearAiGenerated` action**

Add after `extractReminders`:

```typescript
clearAiGenerated: (id) =>
  set((state) => ({
    reminders: state.reminders.map((reminder) =>
      reminder.id === id ? { ...reminder, isAiGenerated: false } : reminder,
    ),
  })),
```

- [ ] **Step 4: Verify TypeScript compiles**

Run: `cd D:/Dev/Actio/frontend && pnpm exec tsc --noEmit`
Expected: No errors.

- [ ] **Step 5: Commit**

```bash
git add frontend/src/store/use-store.ts
git commit -m "feat(store): add extractReminders and clearAiGenerated actions"
```

---

### Task 4: Update ChatComposer to Use Extraction

**Files:**
- Modify: `frontend/src/components/ChatComposer.tsx`

- [ ] **Step 1: Replace `addReminder` with `extractReminders` in ChatComposer**

In `frontend/src/components/ChatComposer.tsx`:

1. Change the store selector from `addReminder` to `extractReminders`:

```typescript
const extractReminders = useStore((s) => s.extractReminders);
```

2. Replace the `handleSubmit` function body. The new version sends the text through extraction and closes immediately:

```typescript
const handleSubmit = async () => {
  const content = text.trim();
  if (!content && images.length === 0) return;
  setSubmitting(true);
  if (recording) stopRecording();

  try {
    if (content) {
      void extractReminders(content);
    }
    setText('');
    setImages([]);
    onClose();
  } finally {
    setSubmitting(false);
  }
};
```

Note: We fire `extractReminders` with `void` (fire-and-forget) because the bar closes immediately. The store handles skeletons and error feedback. Image support through extraction is deferred — images are not sent to the extraction endpoint in this iteration.

- [ ] **Step 2: Verify TypeScript compiles**

Run: `cd D:/Dev/Actio/frontend && pnpm exec tsc --noEmit`
Expected: No errors.

- [ ] **Step 3: Commit**

```bash
git add frontend/src/components/ChatComposer.tsx
git commit -m "feat(chat): wire ChatComposer submit to LLM extraction pipeline"
```

---

### Task 5: Add Shimmer Skeleton and AI Badge to Card

**Files:**
- Modify: `frontend/src/components/Card.tsx`
- Modify: `frontend/src/styles/globals.css`

- [ ] **Step 1: Add skeleton early-return to Card component**

In `frontend/src/components/Card.tsx`, add this block right after the component function signature and before the existing store selectors (at the top of the `Card` function body, before `const setFilter = ...`):

```typescript
// Skeleton variant — no interactivity, just shimmer bars
if (reminder.isExtracting) {
  return (
    <motion.div
      layout
      initial={{ opacity: 0, y: 30 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, scale: 0.8, transition: { duration: 0.15 } }}
    >
      <article className="reminder-card card--skeleton">
        <div className="reminder-accent" style={{ background: '#d4d4d8' }} />
        <div className="card-shell">
          <div className="skeleton-line skeleton-line--short" />
          <div className="skeleton-line skeleton-line--long" />
        </div>
      </article>
    </motion.div>
  );
}
```

- [ ] **Step 2: Add AI badge to card header**

In the card header section of `Card.tsx`, find the `div` that contains `{reminder.isNew && <span className="mini-badge">New</span>}` and add the AI badge before the `isNew` badge:

```typescript
<div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
  {reminder.isAiGenerated && <span className="mini-badge mini-badge--ai">AI</span>}
  {reminder.isNew && <span className="mini-badge">New</span>}
</div>
```

- [ ] **Step 3: Clear AI badge on expand**

In `Card.tsx`, add the `clearAiGenerated` store selector alongside the existing selectors:

```typescript
const clearAiGenerated = useStore((s) => s.clearAiGenerated);
```

Then modify the `onClick` handler on the `<article>` element to clear the badge when the user interacts:

```typescript
onClick={(e) => {
  e.stopPropagation();
  if (reminder.isAiGenerated) clearAiGenerated(reminder.id);
  onToggle();
}}
```

- [ ] **Step 4: Add shimmer CSS and AI badge styles**

Add the following to the end of `frontend/src/styles/globals.css`:

```css
/* ── Skeleton shimmer ────────────────────────────────── */
.card--skeleton {
  pointer-events: none;
}

.card--skeleton .card-shell {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 18px 16px;
}

.skeleton-line {
  height: 14px;
  border-radius: 4px;
  background: linear-gradient(
    90deg,
    var(--color-surface-2, #e4e4e7) 0%,
    var(--color-surface-3, #d4d4d8) 50%,
    var(--color-surface-2, #e4e4e7) 100%
  );
  background-size: 200% 100%;
  animation: shimmer 1.5s ease-in-out infinite;
}

.skeleton-line--short {
  width: 60%;
}

.skeleton-line--long {
  width: 90%;
}

@keyframes shimmer {
  0% { background-position: 200% 0; }
  100% { background-position: -200% 0; }
}

/* ── AI badge ────────────────────────────────────────── */
.mini-badge--ai {
  background: linear-gradient(135deg, #818cf8, #a78bfa);
  color: #fff;
  font-size: 0.6rem;
  font-weight: 700;
  letter-spacing: 0.04em;
}
```

- [ ] **Step 5: Verify TypeScript compiles**

Run: `cd D:/Dev/Actio/frontend && pnpm exec tsc --noEmit`
Expected: No errors.

- [ ] **Step 6: Commit**

```bash
git add frontend/src/components/Card.tsx frontend/src/styles/globals.css
git commit -m "feat(card): add shimmer skeleton and AI-generated badge"
```

---

### Task 6: Add Backend `POST /reminders/extract` Handler

**Files:**
- Modify: `backend/actio-core/src/api/reminder.rs`
- Modify: `backend/actio-core/src/api/mod.rs`

- [ ] **Step 1: Add the extract request struct and handler**

In `backend/actio-core/src/api/reminder.rs`, add these imports at the top (merge with existing imports):

```rust
use crate::engine::agent::pipeline::{AgentBackend, AgentPipeline};
use crate::engine::agent::task::TaskInput;
use crate::engine::agent::tasks::todo::TodoExtractionTask;
use crate::engine::llm_router::LlmSelection;
```

Then add the request struct and handler at the bottom of the file (before the closing):

```rust
#[derive(Debug, Deserialize)]
pub struct ExtractRemindersRequest {
    pub text: String,
}

pub async fn extract_reminders(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<ExtractRemindersRequest>,
) -> Result<Json<Vec<Reminder>>, (StatusCode, Json<AppApiError>)> {
    let text = req.text.trim().to_string();
    if text.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(AppApiError("text is required and must not be empty".into())),
        ));
    }

    // Build an AgentBackend from the current LLM settings
    let settings = state.settings_manager.get().await;
    let backend = match &settings.llm.selection {
        LlmSelection::Disabled => {
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(AppApiError("no LLM backend is configured".into())),
            ));
        }
        LlmSelection::Local { id } => AgentBackend::Local {
            slot: Arc::clone(&state.engine_slot),
            model_id: id.clone(),
        },
        LlmSelection::Remote => {
            // Build a remote client from settings
            let (base_url, api_key) = match (
                settings.llm.remote.base_url.as_deref(),
                settings.llm.remote.api_key.as_deref(),
            ) {
                (Some(b), Some(k)) => (b.to_string(), k.to_string()),
                _ => {
                    // Fall back to env-seed client
                    match &state.remote_client_envseed {
                        Some(client) => {
                            return run_extraction(
                                AgentBackend::Remote(Arc::clone(client)),
                                &text,
                                &state,
                                &headers,
                            )
                            .await;
                        }
                        None => {
                            return Err((
                                StatusCode::SERVICE_UNAVAILABLE,
                                Json(AppApiError(
                                    "remote LLM selected but no credentials configured".into(),
                                )),
                            ));
                        }
                    }
                }
            };
            let model = settings
                .llm
                .remote
                .model
                .clone()
                .unwrap_or_else(|| "gpt-4o-mini".into());
            let cfg = crate::config::LlmConfig {
                base_url,
                api_key,
                model,
            };
            AgentBackend::Remote(Arc::new(
                crate::engine::remote_llm_client::RemoteLlmClient::new(cfg),
            ))
        }
    };

    run_extraction(backend, &text, &state, &headers).await
}

async fn run_extraction(
    backend: AgentBackend,
    text: &str,
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Json<Vec<Reminder>>, (StatusCode, Json<AppApiError>)> {
    let pipeline = AgentPipeline::new(backend);
    let input = TaskInput {
        text: text.to_string(),
        images: vec![],
    };

    let output = pipeline
        .run(&TodoExtractionTask, input)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "extract_reminders: agent pipeline failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AppApiError(format!("LLM extraction failed: {e}"))),
            )
        })?;

    let tenant_id = tenant_id_from_headers(headers);
    let mut created_reminders = Vec::new();

    for item in output.todos {
        let title_text = if item.description.len() > 60 {
            format!("{}...", &item.description[..57])
        } else {
            item.description.clone()
        };

        let priority_str = item.priority.as_ref().map(|p| {
            serde_json::to_string(p)
                .unwrap_or_else(|_| "\"medium\"".into())
                .trim_matches('"')
                .to_string()
        });

        let context = item.assigned_to.as_ref().map(|a| {
            serde_json::json!({ "assigned_to": a }).to_string()
        });

        let new_reminder = NewReminder {
            session_id: None,
            tenant_id,
            speaker_id: None,
            assigned_to: item.assigned_to.clone(),
            title: Some(title_text),
            description: item.description,
            priority: priority_str,
            due_time: None,
            transcript_excerpt: None,
            context,
            source_time: None,
        };

        match reminder_repo::create_reminder(&state.pool, &new_reminder, &[]).await {
            Ok(reminder) => created_reminders.push(reminder),
            Err(e) => {
                tracing::warn!(error = %e, "extract_reminders: failed to persist one todo item");
            }
        }
    }

    Ok(Json(created_reminders))
}
```

- [ ] **Step 2: Add the `Arc` import if not already present**

Make sure `use std::sync::Arc;` is in the imports at the top of `reminder.rs`. If not already present, add it.

- [ ] **Step 3: Register the route in `mod.rs`**

In `backend/actio-core/src/api/mod.rs`, add the new route after the existing `/reminders` routes (after `.route("/reminders/:id", delete(reminder::delete_reminder))`):

```rust
.route("/reminders/extract", post(reminder::extract_reminders))
```

**Important:** This must go BEFORE the `/reminders/:id` routes to avoid the `:id` wildcard matching "extract". Move it right after `.route("/reminders", post(reminder::create_reminder))`:

```rust
.route("/reminders", get(reminder::list_reminders))
.route("/reminders", post(reminder::create_reminder))
.route("/reminders/extract", post(reminder::extract_reminders))
.route("/reminders/:id", get(reminder::get_reminder))
.route("/reminders/:id", patch(reminder::patch_reminder))
.route("/reminders/:id", delete(reminder::delete_reminder))
```

- [ ] **Step 4: Verify the backend compiles**

Run: `cd D:/Dev/Actio/backend && cargo check 2>&1 | tail -5`
Expected: `Finished` with no errors.

- [ ] **Step 5: Commit**

```bash
git add backend/actio-core/src/api/reminder.rs backend/actio-core/src/api/mod.rs
git commit -m "feat(api): add POST /reminders/extract endpoint for LLM todo extraction"
```

---

### Task 7: End-to-End Smoke Test

**Files:** None (manual verification)

- [ ] **Step 1: Start the backend**

Run: `cd D:/Dev/Actio/backend && cargo run`
Expected: Backend starts on port 3000.

- [ ] **Step 2: Start the frontend dev server**

Run: `cd D:/Dev/Actio/frontend && pnpm dev`
Expected: Vite dev server starts.

- [ ] **Step 3: Test the extraction endpoint directly**

Run:
```bash
curl -X POST http://127.0.0.1:3000/reminders/extract \
  -H "Content-Type: application/json" \
  -H "x-tenant-id: 00000000-0000-0000-0000-000000000001" \
  -d '{"text": "Buy groceries tomorrow and call the dentist on Friday"}'
```
Expected: JSON array of 2 reminder objects with titles derived from the extracted items.

- [ ] **Step 4: Test via the UI**

1. Open the app in the browser
2. Click "Capture note" to open NewReminderBar
3. Ensure Chat mode is active (default)
4. Type "Buy groceries tomorrow and schedule a meeting with Sarah"
5. Press Ctrl+Enter or click Send

Expected:
- Bar closes immediately
- 2 shimmer skeleton cards appear at the top of the board
- After 1-3 seconds, real cards replace the skeletons
- Cards show a purple "AI" badge
- Clicking a card expands it and the badge disappears

- [ ] **Step 5: Test edge cases**

1. Submit empty text → bar should not submit (button disabled)
2. If LLM is disabled in settings → feedback toast "Couldn't extract reminders"
3. Submit a single action → 1 card appears (only 1 skeleton gets replaced, the extra skeleton is removed)

- [ ] **Step 6: Commit if any fixes were needed**

```bash
git add -A
git commit -m "fix: address issues found during extraction smoke test"
```
