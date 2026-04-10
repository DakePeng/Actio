# Voice Tabs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add three new tabs to the Board window — Recording (mock live transcript), Clips (auto-segmented snippets with starring), and People (named voiceprint placeholders) — with all state persisted to localStorage.

**Architecture:** A new `use-voice-store.ts` Zustand store owns all voice-domain state (segments, people, clip interval, recording session) and manually persists to localStorage under the key `actio-voice`, mirroring the pattern in `use-store.ts`. The `Tab` union type is extracted to `types/index.ts` so it can be shared across the store, TabBar, and BoardWindow. Three new view components map 1:1 to the new tabs.

**Tech Stack:** React 19, TypeScript 5, Zustand 5, Tailwind CSS 4, Vitest 3, pnpm

---

## File Map

| File | Action |
|------|--------|
| `frontend/src/types/index.ts` | Add `Tab` export, `Segment`, `Person`; update `UIState.activeTab` |
| `frontend/src/store/use-store.ts` | Import `Tab`; update `setActiveTab` signature |
| `frontend/src/store/use-voice-store.ts` | **Create** — Zustand store + `pruneSegments` export |
| `frontend/src/store/__tests__/use-voice-store.test.ts` | **Create** — unit tests for store logic |
| `frontend/src/components/TabBar.tsx` | Import `Tab`; add 3 new tab entries |
| `frontend/src/components/RecordingTab.tsx` | **Create** — recording UI |
| `frontend/src/components/ClipsTab.tsx` | **Create** — clips list UI |
| `frontend/src/components/PeopleTab.tsx` | **Create** — people list UI |
| `frontend/src/components/BoardWindow.tsx` | Import + render 3 new tab components |
| `frontend/src/components/settings/RecordingSection.tsx` | **Create** — clip interval setting |
| `frontend/src/components/settings/SettingsView.tsx` | Import + render `RecordingSection` |

---

## Task 1: Add voice types

**Files:**
- Modify: `frontend/src/types/index.ts`

- [ ] **Step 1: Add `Tab`, `Segment`, `Person` to types and update `UIState`**

  Replace the inline `activeTab` literal in `UIState` and add new types. Open `frontend/src/types/index.ts` and make the following changes:

  At the top of the file, after the existing `Priority` and `ReminderStatus` lines, add:

  ```typescript
  export type Tab = 'board' | 'archive' | 'settings' | 'recording' | 'clips' | 'people';

  export interface Segment {
    id: string;
    sessionId: string;
    text: string;
    createdAt: string; // ISO 8601
    starred: boolean;
  }

  export interface Person {
    id: string;
    name: string;
    color: string; // hex, from preset swatches
    createdAt: string; // ISO 8601
  }
  ```

  Then in `UIState`, change:
  ```typescript
  // Before
  activeTab: 'board' | 'archive' | 'settings';

  // After
  activeTab: Tab;
  ```

- [ ] **Step 2: Verify TypeScript compiles**

  ```bash
  cd frontend && pnpm exec tsc --noEmit
  ```

  Expected: no errors (or only pre-existing errors unrelated to this change).

- [ ] **Step 3: Commit**

  ```bash
  git add frontend/src/types/index.ts
  git commit -m "feat: add Tab, Segment, Person types to types/index.ts"
  ```

---

## Task 2: Update `use-store.ts` to use `Tab`

**Files:**
- Modify: `frontend/src/store/use-store.ts`

- [ ] **Step 1: Import `Tab` and update `setActiveTab`**

  In `frontend/src/store/use-store.ts`, add `Tab` to the import from `../types`:

  ```typescript
  // Before
  import type {
    FilterState,
    Label,
    LabelDraft,
    Preferences,
    Priority,
    Profile,
    Reminder,
    ReminderDraft,
    ReminderPatch,
    UIState,
  } from '../types';

  // After
  import type {
    FilterState,
    Label,
    LabelDraft,
    Preferences,
    Priority,
    Profile,
    Reminder,
    ReminderDraft,
    ReminderPatch,
    Tab,
    UIState,
  } from '../types';
  ```

  Then update the `setActiveTab` signature in the `AppState` interface:

  ```typescript
  // Before
  setActiveTab: (tab: 'board' | 'archive' | 'settings') => void;

  // After
  setActiveTab: (tab: Tab) => void;
  ```

  And update the implementation (the `set` call inside `setActiveTab` is already generic — just the parameter type changes):

  ```typescript
  // Before
  setActiveTab: (tab) =>

  // After — no change to implementation body needed, only the interface above drives TS
  ```

- [ ] **Step 2: Verify TypeScript compiles**

  ```bash
  cd frontend && pnpm exec tsc --noEmit
  ```

  Expected: no errors.

- [ ] **Step 3: Run existing store tests**

  ```bash
  cd frontend && pnpm test -- --reporter=verbose src/store/__tests__/use-store.settings.test.ts
  ```

  Expected: all pass.

- [ ] **Step 4: Commit**

  ```bash
  git add frontend/src/store/use-store.ts
  git commit -m "feat: update setActiveTab to accept full Tab union"
  ```

---

## Task 3: Create `use-voice-store.ts` with tests

**Files:**
- Create: `frontend/src/store/use-voice-store.ts`
- Create: `frontend/src/store/__tests__/use-voice-store.test.ts`

- [ ] **Step 1: Write the failing tests first**

  Create `frontend/src/store/__tests__/use-voice-store.test.ts`:

  ```typescript
  import { beforeEach, describe, expect, it } from 'vitest';
  import { useVoiceStore, pruneSegments } from '../use-voice-store';
  import type { Segment } from '../../types';

  function makeSegment(overrides: Partial<Segment> = {}): Segment {
    return {
      id: crypto.randomUUID(),
      sessionId: 'session-1',
      text: 'Test transcript text.',
      createdAt: new Date().toISOString(),
      starred: false,
      ...overrides,
    };
  }

  describe('pruneSegments', () => {
    it('keeps all starred segments regardless of count', () => {
      const segments = Array.from({ length: 40 }, (_, i) =>
        makeSegment({ id: `s${i}`, starred: true, createdAt: new Date(i * 1000).toISOString() }),
      );
      expect(pruneSegments(segments)).toHaveLength(40);
    });

    it('keeps at most 30 unstarred segments, newest first', () => {
      const segments = Array.from({ length: 40 }, (_, i) =>
        makeSegment({ id: `s${i}`, starred: false, createdAt: new Date(i * 1000).toISOString() }),
      );
      // newest-first order assumed: index 39 = newest
      const result = pruneSegments(segments);
      expect(result).toHaveLength(30);
    });

    it('keeps all starred and top 30 unstarred', () => {
      const starred = Array.from({ length: 5 }, (_, i) =>
        makeSegment({ id: `starred-${i}`, starred: true, createdAt: new Date(i * 1000).toISOString() }),
      );
      const unstarred = Array.from({ length: 35 }, (_, i) =>
        makeSegment({ id: `unstarred-${i}`, starred: false, createdAt: new Date((i + 10) * 1000).toISOString() }),
      );
      // Input: starred + unstarred interleaved newest-first
      const all = [...starred, ...unstarred];
      const result = pruneSegments(all);
      expect(result.filter(s => s.starred)).toHaveLength(5);
      expect(result.filter(s => !s.starred)).toHaveLength(30);
    });

    it('preserves order (newest first) after pruning', () => {
      const segments = Array.from({ length: 5 }, (_, i) =>
        makeSegment({ id: `s${i}`, starred: false, createdAt: new Date((5 - i) * 1000).toISOString() }),
      );
      // already newest-first
      const result = pruneSegments(segments);
      for (let i = 0; i < result.length - 1; i++) {
        expect(result[i].createdAt >= result[i + 1].createdAt).toBe(true);
      }
    });
  });

  describe('useVoiceStore', () => {
    beforeEach(() => {
      localStorage.clear();
      useVoiceStore.setState({
        isRecording: false,
        currentSession: null,
        segments: [],
        people: [],
        clipInterval: 5,
      });
    });

    it('starts with defaults', () => {
      const s = useVoiceStore.getState();
      expect(s.isRecording).toBe(false);
      expect(s.currentSession).toBeNull();
      expect(s.segments).toHaveLength(0);
      expect(s.people).toHaveLength(0);
      expect(s.clipInterval).toBe(5);
    });

    it('startRecording sets isRecording and creates a session', () => {
      useVoiceStore.getState().startRecording();
      const s = useVoiceStore.getState();
      expect(s.isRecording).toBe(true);
      expect(s.currentSession).not.toBeNull();
      expect(s.currentSession!.liveTranscript).toBe('');
    });

    it('appendLiveTranscript appends text to currentSession', () => {
      useVoiceStore.getState().startRecording();
      useVoiceStore.getState().appendLiveTranscript('Hello world.');
      useVoiceStore.getState().appendLiveTranscript('Second sentence.');
      const transcript = useVoiceStore.getState().currentSession!.liveTranscript;
      expect(transcript).toContain('Hello world.');
      expect(transcript).toContain('Second sentence.');
    });

    it('flushInterval creates a segment and clears liveTranscript', () => {
      useVoiceStore.getState().startRecording();
      useVoiceStore.getState().appendLiveTranscript('Some spoken words.');
      useVoiceStore.getState().flushInterval();
      const s = useVoiceStore.getState();
      expect(s.segments).toHaveLength(1);
      expect(s.segments[0].text).toBe('Some spoken words.');
      expect(s.currentSession!.liveTranscript).toBe('');
    });

    it('flushInterval does nothing when liveTranscript is empty', () => {
      useVoiceStore.getState().startRecording();
      useVoiceStore.getState().flushInterval();
      expect(useVoiceStore.getState().segments).toHaveLength(0);
    });

    it('stopRecording with non-empty transcript saves a segment', () => {
      useVoiceStore.getState().startRecording();
      useVoiceStore.getState().appendLiveTranscript('Final words.');
      useVoiceStore.getState().stopRecording();
      const s = useVoiceStore.getState();
      expect(s.isRecording).toBe(false);
      expect(s.currentSession).toBeNull();
      expect(s.segments).toHaveLength(1);
    });

    it('stopRecording with empty transcript saves no segment', () => {
      useVoiceStore.getState().startRecording();
      useVoiceStore.getState().stopRecording();
      expect(useVoiceStore.getState().segments).toHaveLength(0);
    });

    it('starSegment marks a segment as starred', () => {
      useVoiceStore.setState({ segments: [makeSegment({ id: 'seg-1', starred: false })] });
      useVoiceStore.getState().starSegment('seg-1');
      expect(useVoiceStore.getState().segments[0].starred).toBe(true);
    });

    it('unstarSegment marks a segment as unstarred and prunes', () => {
      useVoiceStore.setState({ segments: [makeSegment({ id: 'seg-1', starred: true })] });
      useVoiceStore.getState().unstarSegment('seg-1');
      expect(useVoiceStore.getState().segments[0].starred).toBe(false);
    });

    it('deleteSegment removes the segment', () => {
      useVoiceStore.setState({ segments: [makeSegment({ id: 'seg-1' })] });
      useVoiceStore.getState().deleteSegment('seg-1');
      expect(useVoiceStore.getState().segments).toHaveLength(0);
    });

    it('addPerson creates a person entry', () => {
      useVoiceStore.getState().addPerson('Alice', '#E57373');
      const people = useVoiceStore.getState().people;
      expect(people).toHaveLength(1);
      expect(people[0].name).toBe('Alice');
      expect(people[0].color).toBe('#E57373');
    });

    it('updatePerson changes name and color', () => {
      useVoiceStore.getState().addPerson('Bob', '#64B5F6');
      const id = useVoiceStore.getState().people[0].id;
      useVoiceStore.getState().updatePerson(id, { name: 'Robert', color: '#81C784' });
      const person = useVoiceStore.getState().people[0];
      expect(person.name).toBe('Robert');
      expect(person.color).toBe('#81C784');
    });

    it('deletePerson removes the entry', () => {
      useVoiceStore.getState().addPerson('Carol', '#FFD54F');
      const id = useVoiceStore.getState().people[0].id;
      useVoiceStore.getState().deletePerson(id);
      expect(useVoiceStore.getState().people).toHaveLength(0);
    });

    it('setClipInterval updates interval and persists to localStorage', () => {
      useVoiceStore.getState().setClipInterval(10);
      expect(useVoiceStore.getState().clipInterval).toBe(10);
      const stored = JSON.parse(localStorage.getItem('actio-voice') ?? '{}');
      expect(stored.clipInterval).toBe(10);
    });

    it('persists segments to localStorage on flush', () => {
      useVoiceStore.getState().startRecording();
      useVoiceStore.getState().appendLiveTranscript('Persisted text.');
      useVoiceStore.getState().flushInterval();
      const stored = JSON.parse(localStorage.getItem('actio-voice') ?? '{}');
      expect(stored.segments).toHaveLength(1);
      expect(stored.segments[0].text).toBe('Persisted text.');
    });
  });
  ```

- [ ] **Step 2: Run tests to verify they fail**

  ```bash
  cd frontend && pnpm test -- --reporter=verbose src/store/__tests__/use-voice-store.test.ts
  ```

  Expected: FAIL — `Cannot find module '../use-voice-store'`

- [ ] **Step 3: Create `use-voice-store.ts`**

  Create `frontend/src/store/use-voice-store.ts`:

  ```typescript
  import { create } from 'zustand';
  import type { Segment, Person } from '../types';

  export type ClipInterval = 1 | 2 | 5 | 10 | 30;

  interface RecordingSession {
    id: string;
    startedAt: string;
    liveTranscript: string;
  }

  interface VoiceState {
    isRecording: boolean;
    currentSession: RecordingSession | null;
    segments: Segment[];
    people: Person[];
    clipInterval: ClipInterval;

    startRecording: () => void;
    stopRecording: () => void;
    appendLiveTranscript: (text: string) => void;
    flushInterval: () => void;
    starSegment: (id: string) => void;
    unstarSegment: (id: string) => void;
    deleteSegment: (id: string) => void;
    addPerson: (name: string, color: string) => void;
    updatePerson: (id: string, updates: { name?: string; color?: string }) => void;
    deletePerson: (id: string) => void;
    setClipInterval: (minutes: ClipInterval) => void;
  }

  const MAX_UNSTARRED = 30;
  const STORAGE_KEY = 'actio-voice';

  // Exported for unit testing
  export function pruneSegments(segments: Segment[]): Segment[] {
    // segments are newest-first; keep all starred, keep at most MAX_UNSTARRED unstarred
    let unstarredCount = 0;
    return segments.filter((s) => {
      if (s.starred) return true;
      unstarredCount++;
      return unstarredCount <= MAX_UNSTARRED;
    });
  }

  function loadVoiceData(): { segments: Segment[]; people: Person[]; clipInterval: ClipInterval } {
    try {
      return (
        JSON.parse(localStorage.getItem(STORAGE_KEY) ?? 'null') ?? {
          segments: [],
          people: [],
          clipInterval: 5,
        }
      );
    } catch {
      return { segments: [], people: [], clipInterval: 5 };
    }
  }

  function saveVoiceData(segments: Segment[], people: Person[], clipInterval: ClipInterval) {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ segments, people, clipInterval }));
  }

  const { segments: initialSegments, people: initialPeople, clipInterval: initialClipInterval } =
    loadVoiceData();

  export const useVoiceStore = create<VoiceState>((set, get) => ({
    isRecording: false,
    currentSession: null,
    segments: initialSegments,
    people: initialPeople,
    clipInterval: initialClipInterval,

    startRecording: () => {
      const session: RecordingSession = {
        id: crypto.randomUUID(),
        startedAt: new Date().toISOString(),
        liveTranscript: '',
      };
      set({ isRecording: true, currentSession: session });
    },

    stopRecording: () => {
      const { currentSession } = get();
      if (currentSession?.liveTranscript.trim()) {
        get().flushInterval();
      }
      set({ isRecording: false, currentSession: null });
    },

    appendLiveTranscript: (text) => {
      set((state) => {
        if (!state.currentSession) return state;
        const prev = state.currentSession.liveTranscript;
        return {
          currentSession: {
            ...state.currentSession,
            liveTranscript: prev ? `${prev} ${text}` : text,
          },
        };
      });
    },

    flushInterval: () => {
      const { currentSession, segments, people, clipInterval } = get();
      if (!currentSession || !currentSession.liveTranscript.trim()) return;

      const newSegment: Segment = {
        id: crypto.randomUUID(),
        sessionId: currentSession.id,
        text: currentSession.liveTranscript.trim(),
        createdAt: new Date().toISOString(),
        starred: false,
      };

      const next = pruneSegments([newSegment, ...segments]);
      saveVoiceData(next, people, clipInterval);
      set({
        segments: next,
        currentSession: { ...currentSession, liveTranscript: '' },
      });
    },

    starSegment: (id) => {
      set((state) => {
        const next = state.segments.map((s) => (s.id === id ? { ...s, starred: true } : s));
        saveVoiceData(next, state.people, state.clipInterval);
        return { segments: next };
      });
    },

    unstarSegment: (id) => {
      set((state) => {
        const mapped = state.segments.map((s) => (s.id === id ? { ...s, starred: false } : s));
        const next = pruneSegments(mapped);
        saveVoiceData(next, state.people, state.clipInterval);
        return { segments: next };
      });
    },

    deleteSegment: (id) => {
      set((state) => {
        const next = state.segments.filter((s) => s.id !== id);
        saveVoiceData(next, state.people, state.clipInterval);
        return { segments: next };
      });
    },

    addPerson: (name, color) => {
      set((state) => {
        const person: Person = {
          id: crypto.randomUUID(),
          name,
          color,
          createdAt: new Date().toISOString(),
        };
        const next = [...state.people, person];
        saveVoiceData(state.segments, next, state.clipInterval);
        return { people: next };
      });
    },

    updatePerson: (id, updates) => {
      set((state) => {
        const next = state.people.map((p) => (p.id === id ? { ...p, ...updates } : p));
        saveVoiceData(state.segments, next, state.clipInterval);
        return { people: next };
      });
    },

    deletePerson: (id) => {
      set((state) => {
        const next = state.people.filter((p) => p.id !== id);
        saveVoiceData(state.segments, next, state.clipInterval);
        return { people: next };
      });
    },

    setClipInterval: (minutes) => {
      set((state) => {
        saveVoiceData(state.segments, state.people, minutes);
        return { clipInterval: minutes };
      });
    },
  }));
  ```

- [ ] **Step 4: Run tests to verify they pass**

  ```bash
  cd frontend && pnpm test -- --reporter=verbose src/store/__tests__/use-voice-store.test.ts
  ```

  Expected: all 18 tests pass.

- [ ] **Step 5: Commit**

  ```bash
  git add frontend/src/store/use-voice-store.ts frontend/src/store/__tests__/use-voice-store.test.ts
  git commit -m "feat: add use-voice-store with segments, people, clip interval"
  ```

---

## Task 4: Update `TabBar.tsx` to add the three new tabs

**Files:**
- Modify: `frontend/src/components/TabBar.tsx`

- [ ] **Step 1: Update TabBar**

  Replace the entire content of `frontend/src/components/TabBar.tsx`:

  ```typescript
  import { useStore } from '../store/use-store';
  import { motion } from 'framer-motion';
  import type { Tab } from '../types';

  const TABS: { id: Tab; label: string }[] = [
    { id: 'board', label: 'Board' },
    { id: 'archive', label: 'Archive' },
    { id: 'settings', label: 'Settings' },
    { id: 'recording', label: 'Recording' },
    { id: 'clips', label: 'Clips' },
    { id: 'people', label: 'People' },
  ];

  export function TabBar() {
    const activeTab = useStore((s) => s.ui.activeTab);
    const setActiveTab = useStore((s) => s.setActiveTab);

    return (
      <div className="tab-bar" role="tablist" aria-label="Board navigation">
        {TABS.map(({ id, label }) => {
          const isActive = activeTab === id;
          return (
            <button
              key={id}
              type="button"
              role="tab"
              aria-selected={isActive}
              className={`tab-bar__tab${isActive ? ' is-active' : ''}`}
              onClick={() => setActiveTab(id)}
            >
              {label}
              {isActive && (
                <motion.div
                  layoutId="tabBarIndicator"
                  className="tab-bar__indicator"
                  initial={false}
                  transition={{ type: 'spring', stiffness: 500, damping: 30 }}
                />
              )}
            </button>
          );
        })}
      </div>
    );
  }
  ```

- [ ] **Step 2: Verify TypeScript compiles**

  ```bash
  cd frontend && pnpm exec tsc --noEmit
  ```

  Expected: no errors.

- [ ] **Step 3: Commit**

  ```bash
  git add frontend/src/components/TabBar.tsx
  git commit -m "feat: add Recording, Clips, People tabs to TabBar"
  ```

---

## Task 5: Create `RecordingTab.tsx`

**Files:**
- Create: `frontend/src/components/RecordingTab.tsx`

- [ ] **Step 1: Create the component**

  Create `frontend/src/components/RecordingTab.tsx`:

  ```tsx
  import { useEffect, useRef, useState } from 'react';
  import { useVoiceStore } from '../store/use-voice-store';

  const MOCK_SENTENCES = [
    'The meeting was productive and all agenda items were covered.',
    'Action items were assigned to each team member.',
    'The deadline has been moved to next Friday.',
    'We need to follow up with the client by end of week.',
    'The new feature request will be added to the backlog.',
    'Budget approval is still pending from finance.',
    'The demo went well and the client was satisfied.',
    'We agreed to reconvene next Tuesday at 10 AM.',
    'The design review is scheduled for Thursday afternoon.',
    'Engineering estimates are due by end of sprint.',
  ];

  export function RecordingTab() {
    const isRecording = useVoiceStore((s) => s.isRecording);
    const currentSession = useVoiceStore((s) => s.currentSession);
    const clipInterval = useVoiceStore((s) => s.clipInterval);
    const startRecording = useVoiceStore((s) => s.startRecording);
    const stopRecording = useVoiceStore((s) => s.stopRecording);
    const appendLiveTranscript = useVoiceStore((s) => s.appendLiveTranscript);
    const flushInterval = useVoiceStore((s) => s.flushInterval);

    const [elapsed, setElapsed] = useState(0);
    const transcriptRef = useRef<HTMLDivElement>(null);
    const mockTimerRef = useRef<number | null>(null);
    const clipTimerRef = useRef<number | null>(null);
    const elapsedTimerRef = useRef<number | null>(null);
    const sentenceIndexRef = useRef(0);

    useEffect(() => {
      if (!isRecording) {
        clearAllTimers();
        setElapsed(0);
        return;
      }

      sentenceIndexRef.current = Math.floor(Math.random() * MOCK_SENTENCES.length);

      mockTimerRef.current = window.setInterval(() => {
        const sentence = MOCK_SENTENCES[sentenceIndexRef.current % MOCK_SENTENCES.length];
        sentenceIndexRef.current++;
        appendLiveTranscript(sentence);
      }, 2000);

      clipTimerRef.current = window.setInterval(() => {
        flushInterval();
      }, clipInterval * 60 * 1000);

      elapsedTimerRef.current = window.setInterval(() => {
        setElapsed((prev) => prev + 1);
      }, 1000);

      return clearAllTimers;
    }, [isRecording]); // eslint-disable-line react-hooks/exhaustive-deps

    useEffect(() => {
      if (transcriptRef.current) {
        transcriptRef.current.scrollTop = transcriptRef.current.scrollHeight;
      }
    }, [currentSession?.liveTranscript]);

    function clearAllTimers() {
      if (mockTimerRef.current) window.clearInterval(mockTimerRef.current);
      if (clipTimerRef.current) window.clearInterval(clipTimerRef.current);
      if (elapsedTimerRef.current) window.clearInterval(elapsedTimerRef.current);
      mockTimerRef.current = null;
      clipTimerRef.current = null;
      elapsedTimerRef.current = null;
    }

    const intervalSeconds = clipInterval * 60;
    const secondsIntoInterval = elapsed % intervalSeconds;
    const elapsedMinutes = Math.floor(secondsIntoInterval / 60);
    const elapsedSeconds = secondsIntoInterval % 60;
    const totalMinutes = clipInterval;

    return (
      <div className="recording-tab">
        <div className="recording-tab__controls">
          <button
            type="button"
            className={`recording-btn${isRecording ? ' is-recording' : ''}`}
            onClick={isRecording ? stopRecording : startRecording}
            aria-label={isRecording ? 'Stop recording' : 'Start recording'}
          >
            {isRecording ? '⏹' : '🎙'}
          </button>
          {!isRecording && <p className="recording-tab__hint">Tap to record</p>}
          {isRecording && (
            <p className="recording-tab__timer" aria-live="polite">
              {String(elapsedMinutes).padStart(2, '0')}:{String(elapsedSeconds).padStart(2, '0')}
              {' / '}
              {String(totalMinutes).padStart(2, '0')}:00
            </p>
          )}
        </div>
        {isRecording && currentSession && (
          <div className="recording-tab__transcript" ref={transcriptRef} aria-live="polite">
            {currentSession.liveTranscript || (
              <span className="recording-tab__transcript-placeholder">Listening…</span>
            )}
          </div>
        )}
      </div>
    );
  }
  ```

- [ ] **Step 2: Verify TypeScript compiles**

  ```bash
  cd frontend && pnpm exec tsc --noEmit
  ```

  Expected: no errors.

- [ ] **Step 3: Commit**

  ```bash
  git add frontend/src/components/RecordingTab.tsx
  git commit -m "feat: add RecordingTab with mock live transcript"
  ```

---

## Task 6: Create `ClipsTab.tsx`

**Files:**
- Create: `frontend/src/components/ClipsTab.tsx`

- [ ] **Step 1: Create the component**

  Create `frontend/src/components/ClipsTab.tsx`:

  ```tsx
  import { useState } from 'react';
  import { useVoiceStore } from '../store/use-voice-store';

  type FilterMode = 'all' | 'starred';

  function formatTimestamp(iso: string): string {
    const date = new Date(iso);
    const now = new Date();
    const isToday = date.toDateString() === now.toDateString();
    const time = date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
    if (isToday) return `Today ${time}`;
    return `${date.toLocaleDateString([], { month: 'short', day: 'numeric' })} ${time}`;
  }

  export function ClipsTab() {
    const segments = useVoiceStore((s) => s.segments);
    const starSegment = useVoiceStore((s) => s.starSegment);
    const unstarSegment = useVoiceStore((s) => s.unstarSegment);
    const deleteSegment = useVoiceStore((s) => s.deleteSegment);

    const [filter, setFilter] = useState<FilterMode>('all');
    const [expandedId, setExpandedId] = useState<string | null>(null);

    const visible = filter === 'starred' ? segments.filter((s) => s.starred) : segments;

    return (
      <div className="clips-tab">
        <div className="clips-tab__filters" role="group" aria-label="Filter clips">
          <button
            type="button"
            className={`clips-filter-btn${filter === 'all' ? ' is-active' : ''}`}
            onClick={() => setFilter('all')}
          >
            All
          </button>
          <button
            type="button"
            className={`clips-filter-btn${filter === 'starred' ? ' is-active' : ''}`}
            onClick={() => setFilter('starred')}
          >
            Starred
          </button>
        </div>

        {visible.length === 0 ? (
          <p className="clips-tab__empty">
            {filter === 'starred'
              ? 'No starred clips yet. Star a clip to save it permanently.'
              : 'No clips yet. Start recording to generate clips.'}
          </p>
        ) : (
          <div className="clips-tab__list">
            {visible.map((segment) => {
              const isExpanded = expandedId === segment.id;
              const isLong = segment.text.length > 150;
              return (
                <div key={segment.id} className="clip-card">
                  <div className="clip-card__header">
                    <span className="clip-card__timestamp">{formatTimestamp(segment.createdAt)}</span>
                    <div className="clip-card__actions">
                      <button
                        type="button"
                        className={`clip-star-btn${segment.starred ? ' is-starred' : ''}`}
                        onClick={() =>
                          segment.starred ? unstarSegment(segment.id) : starSegment(segment.id)
                        }
                        aria-label={segment.starred ? 'Unstar clip' : 'Star clip'}
                      >
                        {segment.starred ? '★' : '☆'}
                      </button>
                      <button
                        type="button"
                        className="clip-delete-btn"
                        onClick={() => deleteSegment(segment.id)}
                        aria-label="Delete clip"
                      >
                        🗑
                      </button>
                    </div>
                  </div>
                  <p className={`clip-card__text${isExpanded ? ' is-expanded' : ''}`}>
                    {segment.text}
                  </p>
                  {isLong && (
                    <button
                      type="button"
                      className="clip-expand-btn"
                      onClick={() => setExpandedId(isExpanded ? null : segment.id)}
                    >
                      {isExpanded ? 'Show less' : 'Show more'}
                    </button>
                  )}
                </div>
              );
            })}
          </div>
        )}
      </div>
    );
  }
  ```

- [ ] **Step 2: Verify TypeScript compiles**

  ```bash
  cd frontend && pnpm exec tsc --noEmit
  ```

  Expected: no errors.

- [ ] **Step 3: Commit**

  ```bash
  git add frontend/src/components/ClipsTab.tsx
  git commit -m "feat: add ClipsTab with star/delete and All/Starred filter"
  ```

---

## Task 7: Create `PeopleTab.tsx`

**Files:**
- Create: `frontend/src/components/PeopleTab.tsx`

- [ ] **Step 1: Create the component**

  Create `frontend/src/components/PeopleTab.tsx`:

  ```tsx
  import { useState } from 'react';
  import { useVoiceStore } from '../store/use-voice-store';
  import type { Person } from '../types';

  const PRESET_COLORS = [
    '#E57373',
    '#F06292',
    '#BA68C8',
    '#64B5F6',
    '#4DB6AC',
    '#81C784',
    '#FFD54F',
    '#FF8A65',
  ];

  type FormMode = 'idle' | 'adding' | { editing: string };

  export function PeopleTab() {
    const people = useVoiceStore((s) => s.people);
    const addPerson = useVoiceStore((s) => s.addPerson);
    const updatePerson = useVoiceStore((s) => s.updatePerson);
    const deletePerson = useVoiceStore((s) => s.deletePerson);

    const [formMode, setFormMode] = useState<FormMode>('idle');
    const [name, setName] = useState('');
    const [color, setColor] = useState(PRESET_COLORS[0]);

    function openAdd() {
      setFormMode('adding');
      setName('');
      setColor(PRESET_COLORS[0]);
    }

    function openEdit(person: Person) {
      setFormMode({ editing: person.id });
      setName(person.name);
      setColor(person.color);
    }

    function handleSave() {
      const trimmed = name.trim();
      if (!trimmed) return;
      if (formMode === 'adding') {
        addPerson(trimmed, color);
      } else if (typeof formMode === 'object') {
        updatePerson(formMode.editing, { name: trimmed, color });
      }
      setFormMode('idle');
    }

    function handleCancel() {
      setFormMode('idle');
    }

    const isFormOpen = formMode !== 'idle';

    return (
      <div className="people-tab">
        {!isFormOpen && (
          <button type="button" className="primary-button people-tab__add-btn" onClick={openAdd}>
            Add person
          </button>
        )}

        {isFormOpen && (
          <div className="person-form">
            <input
              type="text"
              className="person-form__name-input"
              placeholder="Name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              autoFocus
              onKeyDown={(e) => {
                if (e.key === 'Enter') handleSave();
                if (e.key === 'Escape') handleCancel();
              }}
            />
            <div className="person-form__swatches" role="group" aria-label="Color">
              {PRESET_COLORS.map((c) => (
                <button
                  key={c}
                  type="button"
                  className={`person-form__swatch${color === c ? ' is-selected' : ''}`}
                  style={{ backgroundColor: c }}
                  onClick={() => setColor(c)}
                  aria-label={`Select color ${c}`}
                  aria-pressed={color === c}
                />
              ))}
            </div>
            <div className="person-form__actions">
              <button
                type="button"
                className="primary-button"
                onClick={handleSave}
                disabled={!name.trim()}
              >
                Save
              </button>
              <button type="button" className="secondary-button" onClick={handleCancel}>
                Cancel
              </button>
            </div>
          </div>
        )}

        <div className="people-tab__list">
          {people.length === 0 && !isFormOpen && (
            <p className="people-tab__empty">No people added yet.</p>
          )}
          {people.map((person) => (
            <div key={person.id} className="person-row">
              <div
                className="person-row__avatar"
                style={{ backgroundColor: person.color }}
                aria-hidden="true"
              >
                {person.name.charAt(0).toUpperCase()}
              </div>
              <span className="person-row__name">{person.name}</span>
              <div className="person-row__actions">
                <button
                  type="button"
                  className="person-edit-btn"
                  onClick={() => openEdit(person)}
                  aria-label={`Edit ${person.name}`}
                >
                  ✏️
                </button>
                <button
                  type="button"
                  className="person-delete-btn"
                  onClick={() => deletePerson(person.id)}
                  aria-label={`Delete ${person.name}`}
                >
                  🗑
                </button>
              </div>
            </div>
          ))}
        </div>
      </div>
    );
  }
  ```

- [ ] **Step 2: Verify TypeScript compiles**

  ```bash
  cd frontend && pnpm exec tsc --noEmit
  ```

  Expected: no errors.

- [ ] **Step 3: Commit**

  ```bash
  git add frontend/src/components/PeopleTab.tsx
  git commit -m "feat: add PeopleTab with add/edit/delete person form"
  ```

---

## Task 8: Wire new tabs into `BoardWindow.tsx`

**Files:**
- Modify: `frontend/src/components/BoardWindow.tsx`

- [ ] **Step 1: Add imports and conditional renders**

  In `frontend/src/components/BoardWindow.tsx`, add three new imports after the existing component imports:

  ```typescript
  // After the existing imports, add:
  import { RecordingTab } from './RecordingTab';
  import { ClipsTab } from './ClipsTab';
  import { PeopleTab } from './PeopleTab';
  ```

  Then in the `desktop-window__body` div, add the three new conditionals after the existing ones:

  ```tsx
  <div className="desktop-window__body">
    {activeTab === 'board' && <Board />}
    {activeTab === 'archive' && <ArchiveView />}
    {activeTab === 'settings' && <SettingsView />}
    {activeTab === 'recording' && <RecordingTab />}
    {activeTab === 'clips' && <ClipsTab />}
    {activeTab === 'people' && <PeopleTab />}
  </div>
  ```

- [ ] **Step 2: Verify TypeScript compiles**

  ```bash
  cd frontend && pnpm exec tsc --noEmit
  ```

  Expected: no errors.

- [ ] **Step 3: Run all tests**

  ```bash
  cd frontend && pnpm test
  ```

  Expected: all tests pass.

- [ ] **Step 4: Commit**

  ```bash
  git add frontend/src/components/BoardWindow.tsx
  git commit -m "feat: render RecordingTab, ClipsTab, PeopleTab in BoardWindow"
  ```

---

## Task 9: Add Recording settings and update `SettingsView`

**Files:**
- Create: `frontend/src/components/settings/RecordingSection.tsx`
- Modify: `frontend/src/components/settings/SettingsView.tsx`

- [ ] **Step 1: Create `RecordingSection.tsx`**

  Create `frontend/src/components/settings/RecordingSection.tsx`:

  ```tsx
  import { useVoiceStore } from '../../store/use-voice-store';
  import type { ClipInterval } from '../../store/use-voice-store';

  const INTERVAL_OPTIONS: { value: ClipInterval; label: string }[] = [
    { value: 1, label: '1 minute' },
    { value: 2, label: '2 minutes' },
    { value: 5, label: '5 minutes' },
    { value: 10, label: '10 minutes' },
    { value: 30, label: '30 minutes' },
  ];

  export function RecordingSection() {
    const clipInterval = useVoiceStore((s) => s.clipInterval);
    const setClipInterval = useVoiceStore((s) => s.setClipInterval);

    return (
      <div className="settings-section">
        <h3 className="settings-section__title">Recording</h3>
        <label className="settings-row">
          <span className="settings-row__label">Auto-clip interval</span>
          <select
            className="settings-row__select"
            value={clipInterval}
            onChange={(e) => setClipInterval(Number(e.target.value) as ClipInterval)}
          >
            {INTERVAL_OPTIONS.map((opt) => (
              <option key={opt.value} value={opt.value}>
                {opt.label}
              </option>
            ))}
          </select>
        </label>
      </div>
    );
  }
  ```

- [ ] **Step 2: Update `SettingsView.tsx` to include `RecordingSection`**

  Replace the content of `frontend/src/components/settings/SettingsView.tsx`:

  ```tsx
  import { ProfileSection } from './ProfileSection';
  import { PreferencesSection } from './PreferencesSection';
  import { LabelManager } from './LabelManager';
  import { RecordingSection } from './RecordingSection';

  export function SettingsView() {
    return (
      <div className="settings-view">
        <ProfileSection />
        <div className="settings-divider" />
        <LabelManager />
        <div className="settings-divider" />
        <PreferencesSection />
        <div className="settings-divider" />
        <RecordingSection />
      </div>
    );
  }
  ```

- [ ] **Step 3: Verify TypeScript compiles**

  ```bash
  cd frontend && pnpm exec tsc --noEmit
  ```

  Expected: no errors.

- [ ] **Step 4: Run all tests**

  ```bash
  cd frontend && pnpm test
  ```

  Expected: all tests pass.

- [ ] **Step 5: Commit**

  ```bash
  git add frontend/src/components/settings/RecordingSection.tsx frontend/src/components/settings/SettingsView.tsx
  git commit -m "feat: add Recording section to Settings with auto-clip interval control"
  ```

---

## Done

All 9 tasks produce a working, tested implementation:

- Three new tabs in the tab bar (Recording, Clips, People)
- Voice store with localStorage persistence, pruning, and full CRUD
- 18 unit tests covering all store logic
- Recording tab with mock live transcript and auto-clip timer
- Clips tab with star/unstar, delete, expand, and All/Starred filter
- People tab with add/edit/delete person and color swatches
- Auto-clip interval configurable from Settings

**Note on styling:** The new components use class names following BEM conventions (`recording-tab`, `clip-card`, `person-row`, etc.). These classes need corresponding CSS rules added to the global stylesheet. The existing `primary-button` and `secondary-button` classes are reused from the existing design system.
