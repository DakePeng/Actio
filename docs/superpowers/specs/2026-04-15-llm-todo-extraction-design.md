# LLM-Powered Todo Extraction from Chat Input

**Date:** 2026-04-15
**Status:** Approved

## Overview

Connect the LLM agent framework to the Chat mode in NewReminderBar. When a user types free-form text and submits, the input is sent to a new backend endpoint that runs it through the existing `AgentPipeline` + `TodoExtractionTask`, extracts structured action items, persists them as reminders, and returns them to the frontend — which displays them as cards with an AI-generated badge.

## Requirements

- Chat mode always routes through the LLM (Form mode remains direct/manual)
- Multiple cards can be extracted from a single input
- LLM fills only fields it's confident about; the rest stay default (medium priority, no due time, no labels)
- NewReminderBar closes immediately on submit; shimmer skeleton cards appear on the board
- AI-generated badge stays on each card until the user interacts with it (clicks/expands)

## Architecture

### Backend: `POST /reminders/extract`

New route that accepts free-form text and returns created reminders.

**Request:**
```json
POST /reminders/extract
Content-Type: application/json
x-tenant-id: <tenant-id>

{
  "text": "Buy groceries tomorrow and call the dentist on Friday"
}
```

**Response:** `200 OK` with `BackendReminderDto[]`

**Internal flow:**
1. Validate input: return 400 if `text` is empty
2. Check LLM availability: return 503 if no LLM is loaded/configured
3. Build `TaskInput { text, images: vec![] }` from the request body
4. Call `AgentPipeline::run(TodoExtractionTask, input)` — reuses the existing pipeline
5. Pipeline routes to local LLM (with `llguidance` JSON schema constraint) or remote (with repair fallback)
6. For each extracted `TodoItem`, create a reminder via existing persistence:
   - `TodoItem.description` → `Reminder.title` (truncated to 60 chars) + `Reminder.description` (full text)
   - `TodoItem.priority` → `Reminder.priority` (default: `medium` if absent)
   - `TodoItem.assigned_to` → stored in `Reminder.context` as JSON
   - `TodoItem.speaker_name` → not used (irrelevant for user-typed input)
7. Return all created reminders as `BackendReminderDto[]`

**Edge cases:**
- LLM returns zero todos → return `200` with empty array `[]`
- LLM not loaded/configured → return `503` with error message
- Empty input → return `400`

**No changes to `TodoExtractionTask`, `AgentPipeline`, or the `repair` module.** The existing transcript-oriented prompt and JSON schema work for free-form user input. The `<transcript>` wrapper tag in the user prompt is harmless.

### Frontend: ChatComposer Submit Flow

`ChatComposer.handleSubmit()` changes from calling `addReminder()` to calling a new `extractReminders(text)` store action.

**Flow:**
1. User types text, hits Send or Ctrl+Enter
2. `ChatComposer` calls `store.extractReminders(text)`
3. NewReminderBar closes immediately
4. Store inserts 2 skeleton placeholder reminders with `isExtracting: true` and temporary IDs
5. Board renders shimmer skeleton cards
6. `POST /reminders/extract` fires
7. On success: replace placeholders with real reminders marked `isNew: true` and `isAiGenerated: true`
8. On failure: remove placeholders, show feedback toast "Couldn't extract reminders"

### Frontend: Shimmer Skeleton Cards

The `Card` component renders a skeleton variant when `reminder.isExtracting === true`:

- 2 gray bars (title-width ~60%, description-width ~90%) with a CSS shimmer animation
- No interactive elements (no drag, no expand, no priority badge)
- Same card dimensions as a real collapsed card — no layout shift on swap

**Shimmer CSS:**
```css
.card--skeleton .skeleton-line {
  background: linear-gradient(
    90deg,
    var(--card-bg) 0%,
    var(--shimmer-highlight) 50%,
    var(--card-bg) 100%
  );
  background-size: 200% 100%;
  animation: shimmer 1.5s ease-in-out infinite;
  border-radius: 4px;
}

@keyframes shimmer {
  0% { background-position: 200% 0; }
  100% { background-position: -200% 0; }
}
```

### Frontend: AI-Generated Badge

When `isAiGenerated === true`, the card displays a small subtle badge (sparkle icon or "AI" text pill) in the card header. The badge is cleared when the user clicks or expands the card, via `clearAiGenerated(id)` called from the expand handler.

## Type Changes

### `frontend/src/types/index.ts`

Add two transient flags to `Reminder`:
- `isExtracting?: boolean` — true while LLM is processing, renders skeleton
- `isAiGenerated?: boolean` — true after extraction, cleared on user interaction

### `frontend/src/api/actio-api.ts`

New method:
```typescript
extractReminders(text: string): Promise<Reminder[]>
// Calls POST /reminders/extract, maps response through mapBackendReminder
```

### `frontend/src/store/use-store.ts`

New actions:
- `extractReminders(text: string)` — skeleton → API → swap flow
- `clearAiGenerated(id: string)` — removes the AI badge on interaction

## Files Changed

| File | Change |
|------|--------|
| `backend/actio-core/src/api/reminder.rs` (or new `extract.rs`) | New `POST /reminders/extract` handler |
| `backend/actio-core/src/api/mod.rs` | Register the new route |
| `frontend/src/types/index.ts` | Add `isExtracting`, `isAiGenerated` to `Reminder` |
| `frontend/src/api/actio-api.ts` | Add `extractReminders()` method |
| `frontend/src/store/use-store.ts` | Add `extractReminders()`, `clearAiGenerated()` actions |
| `frontend/src/components/ChatComposer.tsx` | Change `handleSubmit` to call `extractReminders` |
| `frontend/src/components/Card.tsx` | Skeleton variant + AI badge rendering |
| `frontend/src/components/Card.css` (or equivalent) | Shimmer animation styles |

## Files NOT Changed

- `TodoExtractionTask` / `AgentPipeline` / `repair` module — used as-is
- `NewReminderBar` — only `ChatComposer` changes
- Form mode — continues to create reminders directly, no LLM
- `Board.tsx` — no changes needed, it renders whatever's in the reminders array

## End-to-End Flow

```
User types "Buy groceries tomorrow and call dentist Friday"
  → ChatComposer calls store.extractReminders(text)
  → NewReminderBar closes
  → Store inserts 2 skeleton placeholders (isExtracting: true)
  → Board renders shimmer cards
  → POST /reminders/extract fires
  → Backend: AgentPipeline → TodoExtractionTask → 2 TodoItems
  → Backend persists 2 reminders, returns BackendReminderDto[]
  → Store replaces skeletons with real cards (isNew: true, isAiGenerated: true)
  → Cards animate in with AI badge
  → User expands a card → badge clears
```
