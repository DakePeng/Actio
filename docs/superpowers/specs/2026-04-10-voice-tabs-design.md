# Voice Tabs Design

**Date:** 2026-04-10  
**Scope:** Frontend only — no backend integration  
**Feature:** Three new tabs added to the Board window: Recording, Clips, People

---

## Overview

Three new tabs extend the existing board tab bar. All voice-related state is managed by a new dedicated Zustand store (`use-voice-store.ts`) with localStorage persistence via Zustand's `persist` middleware. No changes are made to the existing `use-store.ts`.

Tab bar becomes: **Board | Archive | Settings | Recording | Clips | People**

---

## 1. Types

```typescript
// Persisted in localStorage
interface Segment {
  id: string
  sessionId: string
  text: string        // accumulated mock transcript for this interval
  createdAt: string   // ISO timestamp when interval ended
  starred: boolean
}

interface Person {
  id: string
  name: string
  color: string       // hex from preset swatches, used for avatar chip
  createdAt: string
}

// Ephemeral — not persisted
interface RecordingSession {
  id: string
  startedAt: string
  liveTranscript: string  // transcript accumulating in the current interval
}

// Extend existing Tab union
type Tab = 'board' | 'archive' | 'settings' | 'recording' | 'clips' | 'people'
```

---

## 2. Voice Store (`use-voice-store.ts`)

A single Zustand store with `persist` middleware. Only `segments`, `people`, and `clipInterval` are written to localStorage. Recording state (`isRecording`, `currentSession`) resets to defaults on app launch.

**State:**
```typescript
interface VoiceState {
  // Ephemeral
  isRecording: boolean
  currentSession: RecordingSession | null

  // Persisted
  segments: Segment[]
  people: Person[]
  clipInterval: 1 | 2 | 5 | 10 | 30  // minutes, default 5

  // Actions
  startRecording: () => void
  stopRecording: () => void
  appendLiveTranscript: (text: string) => void
  flushInterval: () => void         // called by timer, creates a Segment from current liveTranscript
  starSegment: (id: string) => void
  unstarSegment: (id: string) => void
  deleteSegment: (id: string) => void
  addPerson: (name: string, color: string) => void
  updatePerson: (id: string, updates: { name?: string; color?: string }) => void
  deletePerson: (id: string) => void
  setClipInterval: (minutes: 1 | 2 | 5 | 10 | 30) => void
}
```

**Expiration rule:** After any new segment is added, unstarred segments beyond the 30 most recent are pruned. Starred segments are never pruned.

---

## 3. Recording Tab (`RecordingTab.tsx`)

**Idle state:** Centered mic icon button with a short "Tap to record" label below it. Nothing else.

**Active state:**
- Pulsing red indicator on the button
- Elapsed timer showing time into the current interval (e.g., "2:34 / 5:00")
- A scrollable live transcript area below, showing the current interval's accumulating mock text

**Mock transcript:** A `setInterval` running every ~2 seconds appends a random placeholder sentence to `liveTranscript` in the store.

**Auto-clip timer:** A separate `setInterval` fires every `clipInterval` minutes, calling `flushInterval()`, which:
1. Creates a `Segment` from the current `liveTranscript`
2. Prepends it to `segments[]`
3. Runs expiration
4. Clears `liveTranscript`

**On stop:** Any accumulated `liveTranscript` since the last flush is saved as a final partial segment (even if < full interval). Both intervals are cleared. `currentSession` is set to null.

Both intervals are created on `startRecording` and cleared on `stopRecording` or component unmount.

---

## 4. Clips Tab (`ClipsTab.tsx`)

A scrollable list of `Segment` entries, newest first.

**Filter bar:** Two toggle buttons at the top — **All** | **Starred**. Default: All.

**Clip card:** Shows:
- Timestamp (human-readable, e.g., "Today 3:42 PM")
- Transcript text — truncated at ~3 lines with an expand toggle
- Star button (filled star = starred, outline = unstarred) — clicking toggles
- Delete button (trash icon) — removes segment immediately

No batch operations, no search, no reordering.

---

## 5. People Tab (`PeopleTab.tsx`)

A list of person entries. Each row shows:
- Colored avatar chip (first initial on colored background)
- Name
- Edit button (pencil icon)
- Delete button (trash icon)

An **"Add person"** button at the top opens an inline form:
- Name text input
- Color swatch picker (8 preset colors, single-select)
- Save / Cancel buttons

Editing a person opens the same inline form prefilled with their current values. Only one form is open at a time (adding or editing, not both).

This is a placeholder for future voiceprint enrollment — the UI intentionally stays minimal.

---

## 6. Settings Tab Addition

A new **"Recording"** section is added to the existing `SettingsView`. It contains a single control:

**Auto-clip interval:** A select/dropdown with options: 1 min, 2 min, 5 min, 10 min, 30 min. Default: 5 min. Calls `setClipInterval()` in the voice store on change.

---

## 7. Files Changed / Created

| File | Change |
|------|--------|
| `frontend/src/types/index.ts` | Extend `Tab` union, add `Segment`, `Person`, `RecordingSession` |
| `frontend/src/store/use-voice-store.ts` | New — Zustand store with persist |
| `frontend/src/components/RecordingTab.tsx` | New — recording UI |
| `frontend/src/components/ClipsTab.tsx` | New — clips list UI |
| `frontend/src/components/PeopleTab.tsx` | New — people list UI |
| `frontend/src/components/TabBar.tsx` | Add 3 new tab entries |
| `frontend/src/components/BoardWindow.tsx` | Add 3 conditional tab renders |
| `frontend/src/components/settings/SettingsView.tsx` | Add Recording section with interval select |

---

## 8. Out of Scope

- Real speech-to-text (transcription is mocked)
- Real voiceprint audio fingerprinting
- Backend persistence (all data in localStorage)
- Reordering, searching, or bulk-managing clips or people
- Exporting recordings or segments
