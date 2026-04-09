# Settings & Archive Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add Archive and Settings views to the Actio board window via a persistent top tab bar, repurpose "mark done" as archiving, and expose label management, profile, and app preferences in Settings.

**Architecture:** `BoardWindow` gains a `TabBar` beneath the existing toolbar that switches among three views (`Board`, `ArchiveView`, `SettingsView`). The Zustand store is extended with `archiveReminder`/`restoreReminder`/`deleteReminder` actions, an `activeTab` field, and `profile`/`preferences` slices persisted to `localStorage`. Label management is extracted from `Board` into a `LabelManager` component used by `SettingsView`.

**Tech Stack:** React 19, Zustand 5, Framer Motion 12, Vitest 3 (jsdom), Tailwind CSS v4, TypeScript 5.

---

## File Map

| Action | Path | Responsibility |
|---|---|---|
| Create | `frontend/src/setupTests.ts` | Vitest setup (required by vite.config.ts) |
| Modify | `frontend/src/types/index.ts` | Add `Profile`, `Preferences`, `activeTab` to `UIState` |
| Modify | `frontend/src/store/use-store.ts` | Archive actions, tab state, profile/preferences slices |
| Create | `frontend/src/store/__tests__/use-store.settings.test.ts` | Tests for new store state |
| Create | `frontend/src/components/TabBar.tsx` | Three-tab nav for board window |
| Create | `frontend/src/components/ArchiveView.tsx` | Archived reminders list with restore/delete |
| Create | `frontend/src/components/settings/LabelManager.tsx` | Label CRUD UI (extracted from Board) |
| Create | `frontend/src/components/settings/ProfileSection.tsx` | Name/initials edit |
| Create | `frontend/src/components/settings/PreferencesSection.tsx` | Theme/notifications/launch toggles |
| Create | `frontend/src/components/settings/SettingsView.tsx` | Settings container |
| Modify | `frontend/src/components/Board.tsx` | Remove label edit UI; filter archived reminders |
| Modify | `frontend/src/components/Card.tsx` | `archiveReminder` on swipe; `updateReminderInline` rename |
| Modify | `frontend/src/components/StandbyTray.tsx` | Filter archived reminders; `archiveReminder` on swipe |
| Modify | `frontend/src/components/BoardWindow.tsx` | Add TabBar, conditional view rendering |
| Modify | `frontend/src/App.tsx` | Apply `data-theme` from preferences |
| Modify | `frontend/src/styles/globals.css` | Tab bar, archive row, settings, dark mode CSS |

---

## Task 1: Create setupTests.ts

**Files:**
- Create: `frontend/src/setupTests.ts`

Vitest's config (`vite.config.ts`) references `./src/setupTests.ts` as `setupFiles`. The file is missing, which causes an error before any tests run.

- [ ] **Step 1: Create setupTests.ts**

```ts
// frontend/src/setupTests.ts
import '@testing-library/jest-dom';
```

- [ ] **Step 2: Run tests to confirm they reach the actual test failures**

```bash
cd frontend && pnpm test 2>&1 | head -40
```

Expected: tests run (and fail with errors like "archiveReminder is not a function"), not "cannot find setupTests.ts".

- [ ] **Step 3: Commit**

```bash
cd frontend && git add src/setupTests.ts && git commit -m "test: add vitest setupTests entry point"
```

---

## Task 2: Store — archive/restore/delete, updateReminderInline, updateLabelInline

The file `frontend/src/store/__tests__/use-store.swipe.test.ts` already contains tests for these actions. They are currently failing because the store still has `markDone` and `updateReminder` instead of the new names.

**Files:**
- Modify: `frontend/src/types/index.ts`
- Modify: `frontend/src/store/use-store.ts`

- [ ] **Step 1: Run existing tests to confirm they fail**

```bash
cd frontend && pnpm test 2>&1 | grep -E "FAIL|PASS|archiveReminder|restoreReminder"
```

Expected: failures on `archiveReminder is not a function`, `restoreReminder is not a function`, `updateReminderInline is not a function`, `updateLabelInline is not a function`.

- [ ] **Step 2: Update `types/index.ts` — no structural changes needed**

The `Reminder` type already has `archivedAt: string | null`. No type changes required in this task.

- [ ] **Step 3: Update `use-store.ts` — replace `markDone`/`updateReminder`, add new actions**

Replace the entire file `frontend/src/store/use-store.ts` with:

```ts
import { create } from 'zustand';
import type { Reminder, FilterState, UIState, Label, Priority, Profile, Preferences } from '../types';
import { BUILTIN_LABELS } from '../utils/labels';

interface AppState {
  reminders: Reminder[];
  labels: Label[];
  filter: FilterState;
  ui: UIState;
  profile: Profile;
  preferences: Preferences;

  setReminders: (reminders: Reminder[]) => void;
  addReminder: (reminder: Omit<Reminder, 'id' | 'isNew'>) => void;
  updateReminderInline: (id: string, patch: Partial<Pick<Reminder, 'title' | 'description' | 'dueTime'>>) => void;
  addLabel: (label: Omit<Label, 'id'>) => void;
  deleteLabel: (id: string) => void;
  updateLabelInline: (id: string, patch: Partial<Pick<Label, 'name' | 'color' | 'bgColor'>>) => void;
  archiveReminder: (id: string) => void;
  restoreReminder: (id: string) => void;
  deleteReminder: (id: string) => void;
  setPriority: (id: string, priority: Priority) => void;
  setLabels: (id: string, labels: string[]) => void;
  setFilter: (filter: Partial<FilterState>) => void;
  clearFilter: () => void;
  setBoardWindow: (show: boolean) => void;
  setTrayExpanded: (expanded: boolean) => void;
  setActiveTab: (tab: 'board' | 'archive' | 'settings') => void;
  setExpandedCard: (id: string | null) => void;
  highlightCard: (id: string | null) => void;
  setNewReminderBar: (show: boolean) => void;
  setHasSeenOnboarding: (seen: boolean) => void;
  setFeedback: (message: string, tone?: 'neutral' | 'success') => void;
  clearFeedback: () => void;
  clearNewFlag: (id: string) => void;
  setProfile: (patch: Partial<Profile>) => void;
  setPreferences: (patch: Partial<Preferences>) => void;
  reset: () => void;
}

const initialFilter: FilterState = { priority: null, label: null, search: '' };

const defaultProfile: Profile = { name: '', initials: 'JD' };
const defaultPreferences: Preferences = { theme: 'system', launchAtLogin: false, notifications: true };

function loadProfile(): Profile {
  try {
    return JSON.parse(localStorage.getItem('actio-profile') ?? 'null') ?? defaultProfile;
  } catch {
    return defaultProfile;
  }
}

function loadPreferences(): Preferences {
  try {
    return JSON.parse(localStorage.getItem('actio-preferences') ?? 'null') ?? defaultPreferences;
  } catch {
    return defaultPreferences;
  }
}

const initialUI: UIState = {
  showBoardWindow: false,
  trayExpanded: false,
  expandedCardId: null,
  highlightedCardId: null,
  showNewReminderBar: false,
  hasSeenOnboarding: localStorage.getItem('actio-onboarded') === 'true',
  activeTab: 'board',
  feedback: null,
};

function filterReminders(reminders: Reminder[], filter: FilterState) {
  return reminders.filter((r) => {
    if (r.archivedAt !== null) return false;
    if (filter.priority && r.priority !== filter.priority) return false;
    if (filter.label && !r.labels.includes(filter.label)) return false;
    if (filter.search) {
      const q = filter.search.toLowerCase();
      if (!r.title.toLowerCase().includes(q) && !r.description.toLowerCase().includes(q))
        return false;
    }
    return true;
  });
}

let feedbackTimer: number | null = null;
let highlightTimer: number | null = null;

function pushFeedback(
  set: (partial: Partial<AppState> | ((state: AppState) => Partial<AppState>)) => void,
  message: string,
  tone: 'neutral' | 'success' = 'neutral',
) {
  if (feedbackTimer) window.clearTimeout(feedbackTimer);
  set((state) => ({ ui: { ...state.ui, feedback: { message, tone } } }));
  feedbackTimer = window.setTimeout(() => {
    set((state) => ({ ui: { ...state.ui, feedback: null } }));
    feedbackTimer = null;
  }, 2200);
}

export const useStore = create<AppState>((set) => ({
  reminders: [],
  labels: [...BUILTIN_LABELS],
  filter: initialFilter,
  ui: initialUI,
  profile: loadProfile(),
  preferences: loadPreferences(),

  setReminders: (reminders) => set({ reminders }),

  addReminder: (reminder) => {
    set((state) => ({
      reminders: [...state.reminders, { ...reminder, id: crypto.randomUUID(), isNew: true }],
    }));
    pushFeedback(set, 'Reminder added to the board', 'success');
  },

  updateReminderInline: (id, patch) => {
    set((state) => ({
      reminders: state.reminders.map((r) => (r.id === id ? { ...r, ...patch } : r)),
    }));
  },

  addLabel: (label) => {
    set((state) => ({ labels: [...state.labels, { ...label, id: crypto.randomUUID() }] }));
    pushFeedback(set, 'Label created', 'success');
  },

  deleteLabel: (id) => {
    set((state) => ({
      labels: state.labels.filter((l) => l.id !== id),
      reminders: state.reminders.map((r) => ({ ...r, labels: r.labels.filter((lId) => lId !== id) })),
      filter: state.filter.label === id ? { ...state.filter, label: null } : state.filter,
    }));
    pushFeedback(set, 'Label deleted', 'neutral');
  },

  updateLabelInline: (id, patch) => {
    set((state) => ({
      labels: state.labels.map((l) => (l.id === id ? { ...l, ...patch } : l)),
    }));
  },

  archiveReminder: (id) => {
    set((state) => ({
      reminders: state.reminders.map((r) =>
        r.id === id ? { ...r, archivedAt: new Date().toISOString() } : r,
      ),
    }));
    pushFeedback(set, 'Reminder archived', 'neutral');
  },

  restoreReminder: (id) => {
    set((state) => ({
      reminders: state.reminders.map((r) => (r.id === id ? { ...r, archivedAt: null } : r)),
    }));
    pushFeedback(set, 'Restored to board', 'success');
  },

  deleteReminder: (id) => {
    set((state) => ({ reminders: state.reminders.filter((r) => r.id !== id) }));
    pushFeedback(set, 'Deleted permanently', 'neutral');
  },

  setPriority: (id, priority) => {
    set((state) => ({
      reminders: state.reminders.map((r) => (r.id === id ? { ...r, priority } : r)),
    }));
    pushFeedback(set, `Priority set to ${priority}`, 'success');
  },

  setLabels: (id, labels) => {
    set((state) => ({
      reminders: state.reminders.map((r) => (r.id === id ? { ...r, labels } : r)),
    }));
    pushFeedback(set, 'Labels updated', 'success');
  },

  setFilter: (filter) => set((state) => ({ filter: { ...state.filter, ...filter } })),

  clearFilter: () => {
    set({ filter: initialFilter });
    pushFeedback(set, 'Filters cleared', 'neutral');
  },

  setBoardWindow: (show) =>
    set((state) => ({
      ui: {
        ...state.ui,
        showBoardWindow: show,
        trayExpanded: show ? false : state.ui.trayExpanded,
        showNewReminderBar: show ? state.ui.showNewReminderBar : false,
      },
    })),

  setTrayExpanded: (expanded) => set((state) => ({ ui: { ...state.ui, trayExpanded: expanded } })),

  setActiveTab: (tab) =>
    set((state) => ({
      ui: {
        ...state.ui,
        activeTab: tab,
        expandedCardId: null,
        showNewReminderBar: false,
      },
    })),

  setExpandedCard: (id) => set((state) => ({ ui: { ...state.ui, expandedCardId: id } })),

  highlightCard: (id) => {
    if (highlightTimer) { window.clearTimeout(highlightTimer); highlightTimer = null; }
    set((state) => ({ ui: { ...state.ui, highlightedCardId: id } }));
    if (id) {
      highlightTimer = window.setTimeout(() => {
        set((state) => ({ ui: { ...state.ui, highlightedCardId: null } }));
        highlightTimer = null;
      }, 1600);
    }
  },

  setNewReminderBar: (show) => set((state) => ({ ui: { ...state.ui, showNewReminderBar: show } })),

  setHasSeenOnboarding: (seen) => {
    localStorage.setItem('actio-onboarded', 'true');
    set((state) => ({ ui: { ...state.ui, hasSeenOnboarding: seen } }));
  },

  setFeedback: (message, tone = 'neutral') => { pushFeedback(set, message, tone); },

  clearFeedback: () => {
    if (feedbackTimer) { window.clearTimeout(feedbackTimer); feedbackTimer = null; }
    set((state) => ({ ui: { ...state.ui, feedback: null } }));
  },

  clearNewFlag: (id) =>
    set((state) => ({
      reminders: state.reminders.map((r) => (r.id === id ? { ...r, isNew: false } : r)),
    })),

  setProfile: (patch) => {
    set((state) => {
      const next = { ...state.profile, ...patch };
      localStorage.setItem('actio-profile', JSON.stringify(next));
      return { profile: next };
    });
  },

  setPreferences: (patch) => {
    set((state) => {
      const next = { ...state.preferences, ...patch };
      localStorage.setItem('actio-preferences', JSON.stringify(next));
      return { preferences: next };
    });
  },

  reset: () => set({ reminders: [], labels: [...BUILTIN_LABELS], filter: initialFilter, ui: initialUI }),
}));

export function useFilteredReminders() {
  const reminders = useStore((s) => s.reminders);
  const filter = useStore((s) => s.filter);
  return filterReminders(reminders, filter);
}
```

- [ ] **Step 4: Update `types/index.ts` to add `Profile`, `Preferences`, and `activeTab` to `UIState`**

```ts
export type Priority = 'high' | 'medium' | 'low';

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
  createdAt: string;
  archivedAt: string | null;
}

export interface Label {
  id: string;
  name: string;
  color: string;
  bgColor: string;
}

export interface FilterState {
  priority: Priority | null;
  label: string | null;
  search: string;
}

export interface Profile {
  name: string;
  initials: string;
}

export interface Preferences {
  theme: 'light' | 'system' | 'dark';
  launchAtLogin: boolean;
  notifications: boolean;
}

export interface UIState {
  showBoardWindow: boolean;
  trayExpanded: boolean;
  expandedCardId: string | null;
  highlightedCardId: string | null;
  showNewReminderBar: boolean;
  hasSeenOnboarding: boolean;
  activeTab: 'board' | 'archive' | 'settings';
  feedback: {
    message: string;
    tone: 'neutral' | 'success';
  } | null;
}
```

- [ ] **Step 5: Run the existing swipe tests and confirm they pass**

```bash
cd frontend && pnpm test 2>&1 | grep -E "FAIL|PASS|✓|×"
```

Expected: all 5 tests in `use-store.swipe.test.ts` pass.

- [ ] **Step 6: Commit**

```bash
cd frontend && git add src/types/index.ts src/store/use-store.ts && git commit -m "feat: replace markDone with archiveReminder, add restoreReminder/deleteReminder, rename updateReminder to updateReminderInline, add updateLabelInline"
```

---

## Task 3: Store settings tests

**Files:**
- Create: `frontend/src/store/__tests__/use-store.settings.test.ts`

- [ ] **Step 1: Write the failing tests**

```ts
// frontend/src/store/__tests__/use-store.settings.test.ts
import { beforeEach, describe, expect, it } from 'vitest';
import { useStore } from '../use-store';

describe('useStore settings actions', () => {
  beforeEach(() => {
    useStore.getState().reset();
    localStorage.clear();
  });

  it('defaults activeTab to board', () => {
    expect(useStore.getState().ui.activeTab).toBe('board');
  });

  it('sets activeTab and resets expandedCardId', () => {
    useStore.setState((s) => ({ ui: { ...s.ui, expandedCardId: 'r1' } }));
    useStore.getState().setActiveTab('archive');
    expect(useStore.getState().ui.activeTab).toBe('archive');
    expect(useStore.getState().ui.expandedCardId).toBeNull();
  });

  it('setActiveTab closes new reminder bar', () => {
    useStore.setState((s) => ({ ui: { ...s.ui, showNewReminderBar: true } }));
    useStore.getState().setActiveTab('settings');
    expect(useStore.getState().ui.showNewReminderBar).toBe(false);
  });

  it('setProfile merges patch and persists to localStorage', () => {
    useStore.getState().setProfile({ name: 'Jane Doe', initials: 'JD' });
    expect(useStore.getState().profile.name).toBe('Jane Doe');
    expect(JSON.parse(localStorage.getItem('actio-profile') ?? '{}')).toMatchObject({ name: 'Jane Doe' });
  });

  it('setPreferences merges patch and persists to localStorage', () => {
    useStore.getState().setPreferences({ theme: 'dark' });
    expect(useStore.getState().preferences.theme).toBe('dark');
    expect(JSON.parse(localStorage.getItem('actio-preferences') ?? '{}')).toMatchObject({ theme: 'dark' });
  });

  it('setPreferences does not overwrite unrelated fields', () => {
    useStore.getState().setPreferences({ notifications: false });
    useStore.getState().setPreferences({ theme: 'light' });
    expect(useStore.getState().preferences.notifications).toBe(false);
    expect(useStore.getState().preferences.theme).toBe('light');
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd frontend && pnpm test 2>&1 | grep -E "settings|FAIL|PASS"
```

Expected: 6 failing tests in `use-store.settings.test.ts`.

- [ ] **Step 3: Run tests after Task 2 store changes are in place**

The store already implements all these actions (done in Task 2). Run to confirm they pass:

```bash
cd frontend && pnpm test 2>&1 | grep -E "FAIL|PASS|✓|×"
```

Expected: all tests in both test files pass.

- [ ] **Step 4: Commit**

```bash
cd frontend && git add src/store/__tests__/use-store.settings.test.ts && git commit -m "test: add store tests for activeTab, profile, and preferences"
```

---

## Task 4: CSS — tab bar, archive rows, settings sections, dark mode

**Files:**
- Modify: `frontend/src/styles/globals.css` (append at end)

- [ ] **Step 1: Append new CSS rules to `globals.css`**

Add the following to the end of `frontend/src/styles/globals.css`:

```css
/* ─── Tab bar ─────────────────────────────────────────────────────── */

.tab-bar {
  display: flex;
  border-bottom: 1px solid var(--color-border);
  padding: 0 20px;
  background: var(--color-surface);
  flex-shrink: 0;
}

.tab-bar__tab {
  padding: 10px 16px;
  font-size: 0.875rem;
  font-weight: 500;
  color: var(--color-text-secondary);
  background: none;
  border: none;
  border-bottom: 2px solid transparent;
  cursor: pointer;
  transition: color 0.15s, border-color 0.15s;
  margin-bottom: -1px;
  font-family: var(--font-sans);
}

.tab-bar__tab:hover {
  color: var(--color-text);
}

.tab-bar__tab.is-active {
  color: var(--color-accent);
  border-bottom-color: var(--color-accent);
}

/* ─── Archive view ─────────────────────────────────────────────────── */

.archive-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 20px;
  overflow-y: auto;
  flex: 1;
}

.archive-empty {
  display: flex;
  align-items: center;
  justify-content: center;
  flex: 1;
  color: var(--color-text-tertiary);
  font-size: 0.9rem;
}

.archive-row {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 14px;
  background: var(--color-surface);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-sm);
}

.archive-row__title {
  flex: 1;
  font-size: 0.9rem;
  font-weight: 500;
  color: var(--color-text);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.archive-row__date {
  font-size: 0.78rem;
  color: var(--color-text-tertiary);
  white-space: nowrap;
}

.archive-row__actions {
  display: flex;
  gap: 6px;
  flex-shrink: 0;
}

.archive-row__delete {
  color: #dc2626;
}

.archive-row__delete:hover {
  background: #fef2f2;
  color: #b91c1c;
}

/* ─── Settings view ────────────────────────────────────────────────── */

.settings-view {
  padding: 24px 28px;
  display: flex;
  flex-direction: column;
  gap: 0;
  overflow-y: auto;
  flex: 1;
}

.settings-section {
  display: flex;
  flex-direction: column;
  gap: 14px;
  padding: 20px 0;
}

.settings-section__title {
  font-size: 0.7rem;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.08em;
  color: var(--color-text-tertiary);
}

.settings-divider {
  height: 1px;
  background: var(--color-border);
  flex-shrink: 0;
}

.settings-field {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.settings-field__label {
  font-size: 0.82rem;
  font-weight: 500;
  color: var(--color-text-secondary);
}

.settings-input {
  padding: 8px 12px;
  border: 1px solid var(--color-border);
  border-radius: 8px;
  font-size: 0.9rem;
  font-family: var(--font-sans);
  color: var(--color-text);
  background: var(--color-surface);
  outline: none;
  max-width: 280px;
  transition: border-color 0.15s;
}

.settings-input:focus {
  border-color: var(--color-accent);
}

.settings-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  max-width: 400px;
}

.settings-row__label {
  font-size: 0.9rem;
  color: var(--color-text);
}

.settings-row__sublabel {
  font-size: 0.78rem;
  color: var(--color-text-tertiary);
  margin-top: 2px;
}

/* Toggle switch */
.toggle {
  position: relative;
  width: 40px;
  height: 22px;
  flex-shrink: 0;
}

.toggle input {
  opacity: 0;
  width: 0;
  height: 0;
  position: absolute;
}

.toggle__track {
  position: absolute;
  inset: 0;
  background: var(--color-border-strong);
  border-radius: 999px;
  cursor: pointer;
  transition: background 0.2s;
}

.toggle input:checked + .toggle__track {
  background: var(--color-accent);
}

.toggle__thumb {
  position: absolute;
  top: 3px;
  left: 3px;
  width: 16px;
  height: 16px;
  background: #fff;
  border-radius: 50%;
  transition: transform 0.2s;
  pointer-events: none;
}

.toggle input:checked ~ .toggle__thumb {
  transform: translateX(18px);
}

/* Theme selector */
.theme-selector {
  display: flex;
  gap: 8px;
}

.theme-btn {
  padding: 6px 14px;
  border: 1px solid var(--color-border);
  border-radius: 8px;
  font-size: 0.82rem;
  font-family: var(--font-sans);
  background: var(--color-surface);
  color: var(--color-text-secondary);
  cursor: pointer;
  transition: border-color 0.15s, color 0.15s, background 0.15s;
}

.theme-btn.is-active {
  border-color: var(--color-accent);
  color: var(--color-accent);
  background: var(--color-accent-wash);
}

/* ─── Dark mode ─────────────────────────────────────────────────────── */

@media (prefers-color-scheme: dark) {
  :root:not([data-theme="light"]) {
    --color-bg: #0f172a;
    --color-surface: #1e293b;
    --color-surface-strong: #273449;
    --color-border: #334155;
    --color-border-strong: #475569;
    --color-text: #f1f5f9;
    --color-text-secondary: #94a3b8;
    --color-text-tertiary: #64748b;
    --color-accent-wash: #1e1b4b;
  }
}

[data-theme="dark"] {
  --color-bg: #0f172a;
  --color-surface: #1e293b;
  --color-surface-strong: #273449;
  --color-border: #334155;
  --color-border-strong: #475569;
  --color-text: #f1f5f9;
  --color-text-secondary: #94a3b8;
  --color-text-tertiary: #64748b;
  --color-accent-wash: #1e1b4b;
}
```

- [ ] **Step 2: Commit**

```bash
cd frontend && git add src/styles/globals.css && git commit -m "style: add tab bar, archive row, settings, and dark mode CSS"
```

---

## Task 5: TabBar component

**Files:**
- Create: `frontend/src/components/TabBar.tsx`

- [ ] **Step 1: Create the component**

```tsx
// frontend/src/components/TabBar.tsx
import { useStore } from '../store/use-store';

type Tab = 'board' | 'archive' | 'settings';

const TABS: { id: Tab; label: string }[] = [
  { id: 'board', label: 'Board' },
  { id: 'archive', label: 'Archive' },
  { id: 'settings', label: 'Settings' },
];

export function TabBar() {
  const activeTab = useStore((s) => s.ui.activeTab);
  const setActiveTab = useStore((s) => s.setActiveTab);

  return (
    <div className="tab-bar" role="tablist" aria-label="Board navigation">
      {TABS.map(({ id, label }) => (
        <button
          key={id}
          type="button"
          role="tab"
          aria-selected={activeTab === id}
          className={`tab-bar__tab${activeTab === id ? ' is-active' : ''}`}
          onClick={() => setActiveTab(id)}
        >
          {label}
        </button>
      ))}
    </div>
  );
}
```

- [ ] **Step 2: Commit**

```bash
cd frontend && git add src/components/TabBar.tsx && git commit -m "feat: add TabBar component for board/archive/settings navigation"
```

---

## Task 6: ArchiveView component

**Files:**
- Create: `frontend/src/components/ArchiveView.tsx`

- [ ] **Step 1: Create the component**

```tsx
// frontend/src/components/ArchiveView.tsx
import { useStore } from '../store/use-store';

function formatDate(iso: string) {
  return new Date(iso).toLocaleDateString('en-US', {
    month: 'short',
    day: 'numeric',
    year: 'numeric',
  });
}

const PRIORITY_COLORS = {
  high: { bg: '#fef2f2', text: '#b91c1c', label: 'High' },
  medium: { bg: '#fff7df', text: '#a16207', label: 'Medium' },
  low: { bg: '#edf9f1', text: '#166534', label: 'Low' },
};

export function ArchiveView() {
  const reminders = useStore((s) => s.reminders);
  const restoreReminder = useStore((s) => s.restoreReminder);
  const deleteReminder = useStore((s) => s.deleteReminder);

  const archived = [...reminders]
    .filter((r) => r.archivedAt !== null)
    .sort((a, b) => new Date(b.archivedAt!).getTime() - new Date(a.archivedAt!).getTime());

  if (archived.length === 0) {
    return <div className="archive-empty"><p>Nothing archived yet.</p></div>;
  }

  return (
    <div className="archive-list">
      {archived.map((r) => {
        const colors = PRIORITY_COLORS[r.priority ?? 'medium'];
        return (
          <div key={r.id} className="archive-row">
            <span
              className="card-badge"
              style={{ background: colors.bg, color: colors.text, flexShrink: 0 }}
            >
              {colors.label}
            </span>
            <span className="archive-row__title">{r.title}</span>
            <span className="archive-row__date">{formatDate(r.archivedAt!)}</span>
            <div className="archive-row__actions">
              <button
                type="button"
                className="ghost-button"
                onClick={() => restoreReminder(r.id)}
              >
                Restore
              </button>
              <button
                type="button"
                className="ghost-button archive-row__delete"
                onClick={() => deleteReminder(r.id)}
              >
                Delete
              </button>
            </div>
          </div>
        );
      })}
    </div>
  );
}
```

- [ ] **Step 2: Commit**

```bash
cd frontend && git add src/components/ArchiveView.tsx && git commit -m "feat: add ArchiveView with restore and delete actions"
```

---

## Task 7: LabelManager component

Extract the label management UI from `Board.tsx` into a standalone component.

**Files:**
- Create: `frontend/src/components/settings/LabelManager.tsx`

- [ ] **Step 1: Create `frontend/src/components/settings/LabelManager.tsx`**

This is the label editing section currently inside `Board.tsx` (the color wheel, add form, and delete chips), wrapped in a settings-section shell:

```tsx
// frontend/src/components/settings/LabelManager.tsx
import { useState, useRef, useEffect } from 'react';
import { AnimatePresence, motion } from 'framer-motion';
import { useStore } from '../../store/use-store';

const PALETTE = [
  { c: '#6366F1', b: '#EEF2FF' },
  { c: '#DC2626', b: '#FEF2F2' },
  { c: '#D97706', b: '#FFFBEB' },
  { c: '#16A34A', b: '#F0FDF4' },
  { c: '#0284C7', b: '#F0F9FF' },
  { c: '#8B5CF6', b: '#EDE9FE' },
  { c: '#EC4899', b: '#FCE7F3' },
  { c: '#F43F5E', b: '#FFE4E6' },
  { c: '#EAB308', b: '#FEF9C3' },
  { c: '#84CC16', b: '#ECFCCB' },
  { c: '#14B8A6', b: '#CCFBF1' },
  { c: '#64748B', b: '#F1F5F9' },
];

export function LabelManager() {
  const labels = useStore((s) => s.labels);
  const addLabel = useStore((s) => s.addLabel);
  const deleteLabel = useStore((s) => s.deleteLabel);

  const [newLabelText, setNewLabelText] = useState('');
  const [newLabelColor, setNewLabelColor] = useState<{ c: string; b: string } | null>(null);
  const [colorError, setColorError] = useState(false);
  const [showColorWheel, setShowColorWheel] = useState(false);
  const colorWheelRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!showColorWheel) return;
    const handler = (e: MouseEvent) => {
      if (colorWheelRef.current && !colorWheelRef.current.contains(e.target as Node)) {
        setShowColorWheel(false);
      }
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [showColorWheel]);

  const usedColors = new Set(labels.map((l) => l.color));
  const availableColors = PALETTE.filter((p) => !usedColors.has(p.c));

  const handleAddLabel = (e: React.FormEvent) => {
    e.preventDefault();
    if (!newLabelText.trim()) return;
    if (!newLabelColor) { setColorError(true); return; }
    addLabel({ name: newLabelText.trim(), color: newLabelColor.c, bgColor: newLabelColor.b });
    setNewLabelText('');
    setNewLabelColor(null);
    setColorError(false);
    setShowColorWheel(false);
  };

  return (
    <section className="settings-section">
      <div className="settings-section__title">Labels</div>

      <div style={{ display: 'flex', flexWrap: 'wrap', gap: '8px', alignItems: 'center' }}>
        {labels.map((label) => (
          <div
            key={label.id}
            className="filter-chip"
            style={{
              display: 'inline-flex',
              alignItems: 'center',
              gap: '4px',
              color: label.color,
              background: label.bgColor,
              borderColor: `${label.color}33`,
              cursor: 'default',
              userSelect: 'none',
            }}
          >
            <span style={{ display: 'inline-block', width: '7px', height: '7px', borderRadius: '50%', background: label.color, flexShrink: 0 }} />
            {label.name}
            <button
              type="button"
              aria-label={`Delete ${label.name}`}
              onClick={() => deleteLabel(label.id)}
              style={{ background: 'none', border: 'none', cursor: 'pointer', padding: '0 2px', marginLeft: '2px', lineHeight: 1, fontSize: '14px', color: 'inherit', opacity: 0.7, display: 'inline-flex', alignItems: 'center' }}
            >
              ×
            </button>
          </div>
        ))}
      </div>

      <form onSubmit={handleAddLabel} style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
        <div ref={colorWheelRef} style={{ position: 'relative', marginTop: '5px' }}>
          <button
            type="button"
            onClick={() => { setShowColorWheel((v) => !v); setColorError(false); }}
            aria-label="Choose color"
            style={{
              width: '28px', height: '28px', borderRadius: '50%',
              background: newLabelColor ? newLabelColor.c : '#fff',
              border: colorError ? '2px solid #dc2626' : newLabelColor ? '2px solid rgba(0,0,0,0.12)' : '2px dashed rgba(0,0,0,0.25)',
              cursor: 'pointer', padding: 0, flexShrink: 0,
              boxShadow: showColorWheel ? '0 0 0 3px rgba(0,0,0,0.12)' : 'none',
              transition: 'box-shadow 0.15s, border-color 0.15s',
            }}
          />
          <AnimatePresence>
            {showColorWheel && (() => {
              const RADIUS = 42;
              const DOT = 22;
              const n = availableColors.length;
              const offset = RADIUS + DOT / 2 + 4;
              return (
                <motion.div
                  key="colorwheel"
                  initial={{ scale: 0, opacity: 0 }}
                  animate={{ scale: 1, opacity: 1 }}
                  exit={{ scale: 0, opacity: 0 }}
                  transition={{ type: 'spring', stiffness: 380, damping: 28, mass: 0.8 }}
                  style={{ position: 'absolute', left: `${14 - offset}px`, top: `${14 - offset}px`, width: `${offset * 2}px`, height: `${offset * 2}px`, zIndex: 200, pointerEvents: 'none', transformOrigin: `${offset}px ${offset}px` }}
                >
                  <div style={{ position: 'absolute', inset: 0, borderRadius: '50%', background: 'var(--color-surface, #fff)', boxShadow: '0 8px 32px rgba(0,0,0,0.16)', border: '1px solid rgba(0,0,0,0.07)', pointerEvents: 'auto' }} />
                  {availableColors.map((p, i) => {
                    const angle = (2 * Math.PI * i) / n - Math.PI / 2;
                    const cx = offset + RADIUS * Math.cos(angle);
                    const cy = offset + RADIUS * Math.sin(angle);
                    const isChosen = newLabelColor?.c === p.c;
                    return (
                      <button
                        key={p.c}
                        type="button"
                        aria-label={`Pick color ${p.c}`}
                        onClick={() => { setNewLabelColor(p); setShowColorWheel(false); setColorError(false); }}
                        style={{
                          position: 'absolute', left: `${cx - DOT / 2}px`, top: `${cy - DOT / 2}px`,
                          width: `${DOT}px`, height: `${DOT}px`, borderRadius: '50%', background: p.c,
                          border: isChosen ? '3px solid var(--color-text-primary)' : '2px solid rgba(255,255,255,0.7)',
                          cursor: 'pointer', padding: 0, pointerEvents: 'auto',
                          boxShadow: isChosen ? '0 0 0 1px rgba(0,0,0,0.25)' : '0 1px 4px rgba(0,0,0,0.18)',
                          transition: 'transform 0.1s, box-shadow 0.1s',
                          transform: isChosen ? 'scale(1.25)' : 'scale(1)',
                        }}
                      />
                    );
                  })}
                </motion.div>
              );
            })()}
          </AnimatePresence>
        </div>

        <input
          type="text"
          value={newLabelText}
          onChange={(e) => setNewLabelText(e.target.value)}
          placeholder="Label name…"
          className="filter-chip"
          style={{ maxWidth: '160px', padding: '0 12px', outline: 'none', cursor: 'text' }}
        />
        <button
          type="submit"
          disabled={!newLabelText.trim()}
          className="filter-chip"
          style={{
            background: newLabelColor ? newLabelColor.b : 'var(--color-surface)',
            color: newLabelColor ? newLabelColor.c : 'var(--color-text-secondary)',
            borderColor: newLabelColor ? `${newLabelColor.c}33` : undefined,
            opacity: newLabelText.trim() ? 1 : 0.4,
            cursor: newLabelText.trim() ? 'pointer' : 'default',
          }}
        >
          Add label
        </button>
        {colorError && (
          <span style={{ fontSize: '0.78rem', color: '#dc2626', whiteSpace: 'nowrap' }}>Pick a color first</span>
        )}
      </form>
    </section>
  );
}
```

- [ ] **Step 2: Commit**

```bash
cd frontend && git add src/components/settings/LabelManager.tsx && git commit -m "feat: extract LabelManager component for settings"
```

---

## Task 8: SettingsView (ProfileSection + PreferencesSection + container)

**Files:**
- Create: `frontend/src/components/settings/ProfileSection.tsx`
- Create: `frontend/src/components/settings/PreferencesSection.tsx`
- Create: `frontend/src/components/settings/SettingsView.tsx`

- [ ] **Step 1: Create `ProfileSection.tsx`**

```tsx
// frontend/src/components/settings/ProfileSection.tsx
import { useStore } from '../../store/use-store';

export function ProfileSection() {
  const profile = useStore((s) => s.profile);
  const setProfile = useStore((s) => s.setProfile);

  return (
    <section className="settings-section">
      <div className="settings-section__title">Profile</div>
      <div className="settings-field">
        <label className="settings-field__label" htmlFor="profile-name">Name</label>
        <input
          id="profile-name"
          type="text"
          className="settings-input"
          value={profile.name}
          onChange={(e) => setProfile({ name: e.target.value })}
          placeholder="Your name"
        />
      </div>
      <div className="settings-field">
        <label className="settings-field__label" htmlFor="profile-initials">Initials</label>
        <input
          id="profile-initials"
          type="text"
          className="settings-input"
          value={profile.initials}
          onChange={(e) => setProfile({ initials: e.target.value.slice(0, 2).toUpperCase() })}
          placeholder="JD"
          maxLength={2}
          style={{ maxWidth: '80px' }}
        />
      </div>
    </section>
  );
}
```

- [ ] **Step 2: Create `PreferencesSection.tsx`**

```tsx
// frontend/src/components/settings/PreferencesSection.tsx
import { useStore } from '../../store/use-store';
import type { Preferences } from '../../types';

export function PreferencesSection() {
  const preferences = useStore((s) => s.preferences);
  const setPreferences = useStore((s) => s.setPreferences);

  const themes: { id: Preferences['theme']; label: string }[] = [
    { id: 'light', label: 'Light' },
    { id: 'system', label: 'System' },
    { id: 'dark', label: 'Dark' },
  ];

  return (
    <section className="settings-section">
      <div className="settings-section__title">Preferences</div>

      <div className="settings-field">
        <div className="settings-field__label">Theme</div>
        <div className="theme-selector">
          {themes.map(({ id, label }) => (
            <button
              key={id}
              type="button"
              className={`theme-btn${preferences.theme === id ? ' is-active' : ''}`}
              onClick={() => setPreferences({ theme: id })}
            >
              {label}
            </button>
          ))}
        </div>
      </div>

      <div className="settings-row">
        <div>
          <div className="settings-row__label">Notifications</div>
          <div className="settings-row__sublabel">Show alerts for new reminders</div>
        </div>
        <label className="toggle">
          <input
            type="checkbox"
            checked={preferences.notifications}
            onChange={(e) => setPreferences({ notifications: e.target.checked })}
          />
          <div className="toggle__track" />
          <div className="toggle__thumb" />
        </label>
      </div>

      <div className="settings-row">
        <div>
          <div className="settings-row__label">Launch at login</div>
          <div className="settings-row__sublabel">Start Actio automatically when you log in</div>
        </div>
        <label className="toggle">
          <input
            type="checkbox"
            checked={preferences.launchAtLogin}
            onChange={(e) => setPreferences({ launchAtLogin: e.target.checked })}
          />
          <div className="toggle__track" />
          <div className="toggle__thumb" />
        </label>
      </div>
    </section>
  );
}
```

- [ ] **Step 3: Create `SettingsView.tsx`**

```tsx
// frontend/src/components/settings/SettingsView.tsx
import { ProfileSection } from './ProfileSection';
import { PreferencesSection } from './PreferencesSection';
import { LabelManager } from './LabelManager';

export function SettingsView() {
  return (
    <div className="settings-view">
      <ProfileSection />
      <div className="settings-divider" />
      <LabelManager />
      <div className="settings-divider" />
      <PreferencesSection />
    </div>
  );
}
```

- [ ] **Step 4: Commit**

```bash
cd frontend && git add src/components/settings/ && git commit -m "feat: add SettingsView with Profile, LabelManager, and Preferences sections"
```

---

## Task 9: Clean up Board.tsx

Remove the label editing UI from `Board` (it now lives in `SettingsView`).

**Files:**
- Modify: `frontend/src/components/Board.tsx`

- [ ] **Step 1: Remove label edit state and handlers**

In `Board.tsx`, delete:
- The `PALETTE` constant (lines 15-28)
- The `isEditingLabels` state
- The `newLabelText` state
- The `newLabelColor` state
- The `colorError` state
- The `showColorWheel` state
- The `colorWheelRef` ref
- The `useEffect` for outside-click on the color wheel
- The `usedColors` and `availableColors` derived values
- The `handleToggleEdit` function
- The `handleAddLabel` function

Also remove the following imports that are no longer needed after removing the above:
- `useRef` from react (if only used for `colorWheelRef`)
- `AnimatePresence` and `motion` from framer-motion (if only used in the color wheel)
- `addLabel` from `useStore`

- [ ] **Step 2: Simplify the label filter row in the JSX**

Replace the entire `{/* Label filter row */}` section so it only shows clickable filter chips (no edit mode, no add form, no "Edit labels" button):

```tsx
{/* Label filter row */}
<div className="board-summary__cluster">
  <div className="board-summary__label">Labels</div>
  <div className="filter-group" style={{ display: 'flex', alignItems: 'center', flexWrap: 'wrap', gap: '6px' }}>
    {labels.map((label) => {
      const isSelected = filter.label === label.id;
      return (
        <button
          key={label.id}
          type="button"
          className={`filter-chip${isSelected ? ' is-selected' : ''}`}
          onClick={() => {
            const next = filter.label === label.id ? null : label.id;
            setFilter({ label: next });
            setFeedback(next ? `${label.name} filter applied` : 'Label filter cleared');
          }}
          style={isSelected ? { color: label.color, background: label.bgColor, borderColor: `${label.color}33` } : undefined}
        >
          <span style={{ display: 'inline-block', width: '7px', height: '7px', borderRadius: '50%', background: label.color, flexShrink: 0 }} />
          {label.name}
        </button>
      );
    })}
  </div>
</div>
```

Also remove `addLabel` and `deleteLabel` from the `useStore` selectors at the top of the component since they're no longer needed in `Board`.

- [ ] **Step 3: Run tests to confirm nothing breaks**

```bash
cd frontend && pnpm test 2>&1 | grep -E "FAIL|PASS|✓|×"
```

Expected: all tests still pass.

- [ ] **Step 4: Commit**

```bash
cd frontend && git add src/components/Board.tsx && git commit -m "refactor: remove label edit UI from Board (moved to Settings)"
```

---

## Task 10: Update Card.tsx and StandbyTray.tsx

Replace `markDone` / `updateReminder` with `archiveReminder` / `updateReminderInline`.

**Files:**
- Modify: `frontend/src/components/Card.tsx`
- Modify: `frontend/src/components/StandbyTray.tsx`

- [ ] **Step 1: Update `Card.tsx`**

In `Card.tsx`:

1. Replace `const markDone = useStore((s) => s.markDone);` with `const archiveReminder = useStore((s) => s.archiveReminder);`

2. Replace `const updateReminder = useStore((s) => s.updateReminder);` with `const updateReminderInline = useStore((s) => s.updateReminderInline);`

3. In the `commitEdits` function, replace:
   ```ts
   updateReminder(reminder.id, { title: t || title, description: d });
   ```
   with:
   ```ts
   updateReminderInline(reminder.id, { title: t || title, description: d });
   ```

4. In `onDragEnd`, replace:
   ```ts
   markDone(reminder.id);
   setFeedback(`Completed: ${title}`);
   ```
   with:
   ```ts
   archiveReminder(reminder.id);
   setFeedback(`Archived: ${title}`);
   ```

- [ ] **Step 2: Update `StandbyTray.tsx`**

1. Replace `const markDone = useStore((s) => s.markDone);` with `const archiveReminder = useStore((s) => s.archiveReminder);`

2. Replace the `topReminders` derived value to filter out archived reminders:
   ```ts
   const topReminders = useMemo(() => {
     return [...reminders].filter((r) => r.archivedAt === null).sort(sortByPriority).slice(0, 6);
   }, [reminders]);
   ```

3. In the `SwipeActionRow` `rightAction.onExecute`, replace `() => markDone(reminder.id)` with `() => archiveReminder(reminder.id)`.

- [ ] **Step 3: Run tests**

```bash
cd frontend && pnpm test 2>&1 | grep -E "FAIL|PASS|✓|×"
```

Expected: all tests pass.

- [ ] **Step 4: Commit**

```bash
cd frontend && git add src/components/Card.tsx src/components/StandbyTray.tsx && git commit -m "feat: swipe-to-done archives instead of deleting; filter archived reminders from tray"
```

---

## Task 11: Wire up BoardWindow and add theme effect to App.tsx

**Files:**
- Modify: `frontend/src/components/BoardWindow.tsx`
- Modify: `frontend/src/App.tsx`

- [ ] **Step 1: Update `BoardWindow.tsx`**

Replace the entire file with:

```tsx
// frontend/src/components/BoardWindow.tsx
import { useEffect } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { useStore } from '../store/use-store';
import { Board } from './Board';
import { ArchiveView } from './ArchiveView';
import { SettingsView } from './settings/SettingsView';
import { TabBar } from './TabBar';
import { NewReminderBar } from './NewReminderBar';

export function BoardWindow() {
  const showBoardWindow = useStore((s) => s.ui.showBoardWindow);
  const activeTab = useStore((s) => s.ui.activeTab);
  const setBoardWindow = useStore((s) => s.setBoardWindow);
  const setNewReminderBar = useStore((s) => s.setNewReminderBar);
  const clearFeedback = useStore((s) => s.clearFeedback);

  useEffect(() => {
    if (!showBoardWindow) return;
    const handler = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setBoardWindow(false);
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [showBoardWindow, setBoardWindow]);

  return (
    <AnimatePresence>
      {showBoardWindow && (
        <>
          <motion.div
            className="desktop-window-backdrop"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            onClick={() => { clearFeedback(); setBoardWindow(false); }}
          />
          <div className="desktop-window-shell">
            <motion.section
              className="desktop-window"
              initial={{ opacity: 0, y: 36, scale: 0.94 }}
              animate={{ opacity: 1, y: 0, scale: 1 }}
              exit={{ opacity: 0, y: 24, scale: 0.97 }}
              transition={{ type: 'spring', stiffness: 260, damping: 24 }}
            >
              <div className="desktop-toolbar">
                <div className="desktop-toolbar__brand">
                  <div>
                    <div className="desktop-toolbar__title">Actio board</div>
                  </div>
                </div>
                <div className="desktop-toolbar__actions">
                  <button
                    type="button"
                    className="secondary-button"
                    onClick={() => { clearFeedback(); setBoardWindow(false); }}
                  >
                    Return to tray
                  </button>
                  <button
                    type="button"
                    className="primary-button"
                    disabled={activeTab !== 'board'}
                    onClick={() => setNewReminderBar(true)}
                  >
                    Capture note
                  </button>
                </div>
              </div>

              <TabBar />

              <div className="desktop-window__body">
                {activeTab === 'board' && <Board />}
                {activeTab === 'archive' && <ArchiveView />}
                {activeTab === 'settings' && <SettingsView />}
              </div>

              <NewReminderBar />
            </motion.section>
          </div>
        </>
      )}
    </AnimatePresence>
  );
}
```

- [ ] **Step 2: Update `App.tsx` to apply theme preference**

Add a `useEffect` for theme after the existing effects. Add `useStore` selector for `preferences.theme`:

After the last `useEffect` in `App.tsx`, add:

```tsx
const theme = useStore((s) => s.preferences.theme);

useEffect(() => {
  const root = document.documentElement;
  if (theme === 'system') {
    root.removeAttribute('data-theme');
  } else {
    root.setAttribute('data-theme', theme);
  }
}, [theme]);
```

(Add `const theme = useStore(...)` with the other store selectors at the top of `App`, and add the `useEffect` alongside the others.)

- [ ] **Step 3: Run tests to confirm nothing breaks**

```bash
cd frontend && pnpm test 2>&1 | grep -E "FAIL|PASS|✓|×"
```

Expected: all tests pass.

- [ ] **Step 4: Commit**

```bash
cd frontend && git add src/components/BoardWindow.tsx src/App.tsx && git commit -m "feat: wire up tab bar navigation in BoardWindow; apply theme preference in App"
```

---

## Self-Review Checklist

- [x] **Spec coverage**
  - Tab bar with Board/Archive/Settings tabs → Tasks 5, 11
  - `activeTab` in UIState with `setActiveTab` → Task 2 (store), Task 3 (tests)
  - `setActiveTab` resets `expandedCardId` and `showNewReminderBar` → Task 2 (store), Task 3 (tests)
  - `markDone` → `archiveReminder` (sets `archivedAt`) → Task 2
  - `useFilteredReminders` excludes archived → Task 2 (in `filterReminders`)
  - Archive view: list sorted by `archivedAt` desc, restore + delete actions → Task 6
  - `restoreReminder` / `deleteReminder` → Task 2
  - Swipe gesture calls `archiveReminder` → Task 10
  - Tray filters archived reminders → Task 10
  - Settings: Profile (name, initials) → Task 8
  - Settings: LabelManager extracted from Board → Tasks 7, 9
  - Settings: Preferences (theme, notifications, launch at login) → Task 8
  - Dark mode CSS via `data-theme` → Task 4
  - Theme applied in App.tsx → Task 11
  - "Capture note" disabled on non-board tabs → Task 11

- [x] **Placeholder scan:** No TBDs. All steps have complete code.

- [x] **Type consistency:**
  - `archiveReminder(id: string)` — consistent across Tasks 2, 10
  - `restoreReminder(id: string)` — consistent across Tasks 2, 6
  - `deleteReminder(id: string)` — consistent across Tasks 2, 6
  - `updateReminderInline(id, patch)` — consistent across Tasks 2, 10
  - `updateLabelInline(id, patch)` — consistent across Tasks 2, 7
  - `activeTab: 'board' | 'archive' | 'settings'` — consistent across Tasks 2, 3, 5, 11
  - `Profile`, `Preferences` types defined in Task 2 (`types/index.ts`), consumed in Tasks 8
  - `setProfile`, `setPreferences` — consistent across Tasks 2, 8
