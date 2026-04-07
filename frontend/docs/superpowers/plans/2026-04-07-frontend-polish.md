# Frontend Polish & Feature Pass — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix three critical bugs, consolidate the dual-theme CSS to a single indigo/slate system, and ship four features: priority editing, label assignment, due-time capture, and tray swipe-to-complete.

**Architecture:** Layer 1 (types → store → swipe component → CSS) establishes a clean foundation. Layer 2 (card edit → capture form → tray) builds features on top. Each task is an independent commit.

**Tech Stack:** React 18, TypeScript, Zustand, Framer Motion, Tailwind CSS v4 (`@theme`), Vitest + React Testing Library. Run tests with `pnpm test`.

---

## File Map

| File | What changes |
|---|---|
| `src/types/index.ts` | Add `archivedAt: string \| null` to `Reminder` |
| `src/utils/labels.ts` | Rename `INITIAL_LABELS` → `BUILTIN_LABELS` |
| `src/tauri/mock-data.ts` | Add `archivedAt: null` to every mock reminder |
| `src/store/use-store.ts` | Unify labels field; add `archiveReminder`, `restoreReminder`, `deleteLabel`, `updateReminderInline`, `updateLabelInline`; remove `markDone` |
| `src/components/swipe/SwipeActionRow.tsx` | Make `leftAction` / `rightAction` optional |
| `src/styles/globals.css` | Merge @theme blocks; replace all rust-orange + warm-tan values; fill hover states; add swipe CSS; add card-edit CSS |
| `index.html` | Replace Manrope/Newsreader font link with Plus Jakarta Sans |
| `src/components/OnboardingCard.tsx` | Replace all inline styles with `.onboarding__*` CSS classes |
| `src/components/Card.tsx` | Fix `getLabelById` call; swap Framer Motion drag for `SwipeActionRow`; add expand-to-edit section |
| `src/components/Board.tsx` | Wrap card list in `SwipeActionCoordinatorProvider` |
| `src/components/NewReminderBar.tsx` | Add `dueTime` text field; reset on close |
| `src/components/StandbyTray.tsx` | Wrap tray items in `SwipeActionRow` + `SwipeActionCoordinatorProvider` |

---

## Task 1: Types — add `archivedAt` + rename `BUILTIN_LABELS`

**Files:**
- Modify: `src/types/index.ts`
- Modify: `src/utils/labels.ts`
- Modify: `src/tauri/mock-data.ts`

- [ ] **Step 1: Add `archivedAt` to the Reminder interface**

In `src/types/index.ts`, update `Reminder`:

```ts
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
```

- [ ] **Step 2: Rename the labels export**

In `src/utils/labels.ts`, rename `INITIAL_LABELS` to `BUILTIN_LABELS`:

```ts
export const BUILTIN_LABELS: Label[] = [
  { id: 'work',     name: 'Work',     color: '#6366F1', bgColor: '#EEF2FF' },
  { id: 'urgent',   name: 'Urgent',   color: '#DC2626', bgColor: '#FEF2F2' },
  { id: 'meeting',  name: 'Meeting',  color: '#D97706', bgColor: '#FFFBEB' },
  { id: 'personal', name: 'Personal', color: '#16A34A', bgColor: '#F0FDF4' },
  { id: 'health',   name: 'Health',   color: '#CA8A04', bgColor: '#FFFBEB' },
  { id: 'finance',  name: 'Finance',  color: '#0284C7', bgColor: '#F0F9FF' },
] as const;
```

Keep `getLabelById` and `computeLabelCounts` unchanged.

- [ ] **Step 3: Add `archivedAt: null` to every mock reminder**

In `src/tauri/mock-data.ts`, add `archivedAt: null` to all six entries. Example for the first:

```ts
{
  id: '1',
  title: 'Prepare Q3 report',
  // ... existing fields ...
  createdAt: new Date(Date.now() - 86400000).toISOString(),
  archivedAt: null,
},
```

Repeat for ids `'2'` through `'6'`.

- [ ] **Step 4: Verify TypeScript compiles cleanly**

```bash
cd frontend && pnpm exec tsc --noEmit
```

Expected: zero errors.

- [ ] **Step 5: Commit**

```bash
git add frontend/src/types/index.ts frontend/src/utils/labels.ts frontend/src/tauri/mock-data.ts
git commit -m "feat: add archivedAt to Reminder type and rename BUILTIN_LABELS"
```

---

## Task 2: Store — unify labels, archive actions, inline-update

**Files:**
- Modify: `src/store/use-store.ts`
- Test: `src/store/__tests__/use-store.swipe.test.ts` (already written — run it)

- [ ] **Step 1: Run the existing test to confirm it fails**

```bash
cd frontend && pnpm test src/store/__tests__/use-store.swipe.test.ts
```

Expected: multiple failures (missing `archiveReminder`, `labels` field, etc.).

- [ ] **Step 2: Rewrite `use-store.ts`**

Replace the full file contents:

```ts
import { create } from 'zustand';
import { BUILTIN_LABELS } from '../utils/labels';
import type { Reminder, FilterState, UIState, Label, Priority } from '../types';

interface AppState {
  reminders: Reminder[];
  labels: Label[];
  filter: FilterState;
  ui: UIState;

  setReminders: (reminders: Reminder[]) => void;
  addReminder: (reminder: Omit<Reminder, 'id' | 'isNew' | 'archivedAt'>) => void;
  archiveReminder: (id: string) => void;
  restoreReminder: (id: string) => void;
  updateReminderInline: (id: string, patch: Partial<Pick<Reminder, 'title' | 'description' | 'dueTime' | 'priority' | 'labels'>>) => void;
  addCustomLabel: (label: Omit<Label, 'id'>) => void;
  deleteLabel: (id: string) => void;
  updateLabelInline: (id: string, patch: Partial<Pick<Label, 'name' | 'color' | 'bgColor'>>) => void;
  setFilter: (filter: Partial<FilterState>) => void;
  clearFilter: () => void;
  setBoardWindow: (show: boolean) => void;
  setTrayExpanded: (expanded: boolean) => void;
  toggleLabelsPanel: () => void;
  setExpandedCard: (id: string | null) => void;
  highlightCard: (id: string | null) => void;
  setNewReminderBar: (show: boolean) => void;
  setHasSeenOnboarding: (seen: boolean) => void;
  setFeedback: (message: string, tone?: 'neutral' | 'success') => void;
  clearFeedback: () => void;
  clearNewFlag: (id: string) => void;
  reset: () => void;
}

const initialFilter: FilterState = { priority: null, label: null, search: '' };

const initialUI: UIState = {
  showBoardWindow: false,
  showLabelsPanel: false,
  trayExpanded: false,
  expandedCardId: null,
  highlightedCardId: null,
  showNewReminderBar: false,
  hasSeenOnboarding: localStorage.getItem('actio-onboarded') === 'true',
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

  setReminders: (reminders) => set({ reminders }),

  addReminder: (reminder) => {
    set((state) => ({
      reminders: [
        ...state.reminders,
        { ...reminder, id: crypto.randomUUID(), isNew: true, archivedAt: null },
      ],
    }));
    pushFeedback(set, 'Reminder added to the board', 'success');
  },

  archiveReminder: (id) => {
    set((state) => ({
      reminders: state.reminders.map((r) =>
        r.id === id ? { ...r, archivedAt: new Date().toISOString() } : r,
      ),
    }));
    pushFeedback(set, 'Reminder marked done', 'success');
  },

  restoreReminder: (id) => {
    set((state) => ({
      reminders: state.reminders.map((r) =>
        r.id === id ? { ...r, archivedAt: null } : r,
      ),
    }));
    pushFeedback(set, 'Reminder restored', 'neutral');
  },

  updateReminderInline: (id, patch) => {
    set((state) => ({
      reminders: state.reminders.map((r) =>
        r.id === id ? { ...r, ...patch } : r,
      ),
    }));
  },

  addCustomLabel: (label) => {
    set((state) => ({
      labels: [...state.labels, { ...label, id: crypto.randomUUID() }],
    }));
    pushFeedback(set, 'Label created', 'success');
  },

  deleteLabel: (id) => {
    set((state) => ({
      labels: state.labels.filter((l) => l.id !== id),
      reminders: state.reminders.map((r) => ({
        ...r,
        labels: r.labels.filter((lId) => lId !== id),
      })),
      filter: {
        ...state.filter,
        label: state.filter.label === id ? null : state.filter.label,
      },
    }));
    pushFeedback(set, 'Label deleted', 'neutral');
  },

  updateLabelInline: (id, patch) => {
    set((state) => ({
      labels: state.labels.map((l) => (l.id === id ? { ...l, ...patch } : l)),
    }));
  },

  setFilter: (filter) =>
    set((state) => ({ filter: { ...state.filter, ...filter } })),

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
        showLabelsPanel: show ? state.ui.showLabelsPanel : false,
        showNewReminderBar: show ? state.ui.showNewReminderBar : false,
      },
    })),

  setTrayExpanded: (expanded) =>
    set((state) => ({ ui: { ...state.ui, trayExpanded: expanded } })),

  toggleLabelsPanel: () =>
    set((state) => ({ ui: { ...state.ui, showLabelsPanel: !state.ui.showLabelsPanel } })),

  setExpandedCard: (id) =>
    set((state) => ({ ui: { ...state.ui, expandedCardId: id } })),

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

  setNewReminderBar: (show) =>
    set((state) => ({ ui: { ...state.ui, showNewReminderBar: show } })),

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

  reset: () => set({ reminders: [], labels: [...BUILTIN_LABELS], filter: initialFilter, ui: initialUI }),
}));

export function useFilteredReminders() {
  const reminders = useStore((s) => s.reminders);
  const filter = useStore((s) => s.filter);
  return filterReminders(reminders, filter);
}
```

- [ ] **Step 3: Run the store tests**

```bash
cd frontend && pnpm test src/store/__tests__/use-store.swipe.test.ts
```

Expected: all 5 tests pass.

- [ ] **Step 4: Compile check**

```bash
cd frontend && pnpm exec tsc --noEmit
```

Expected: zero errors. Fix any type errors before continuing.

- [ ] **Step 5: Commit**

```bash
git add frontend/src/store/use-store.ts
git commit -m "feat: unify labels in store, add archive/restore/inline-update actions"
```

---

## Task 3: SwipeActionRow — make actions optional

**Files:**
- Modify: `src/components/swipe/SwipeActionRow.tsx`
- Test: `src/components/swipe/__tests__/SwipeActionRow.test.tsx` (existing — must keep passing)

- [ ] **Step 1: Run existing tests to confirm they pass before the change**

```bash
cd frontend && pnpm test src/components/swipe/__tests__/SwipeActionRow.test.tsx
```

Expected: all 3 tests pass.

- [ ] **Step 2: Make both action props optional**

In `src/components/swipe/SwipeActionRow.tsx`, update the type and guard both sides:

```ts
export type SwipeActionRowProps = {
  rowId: string;
  leftAction?: SwipeActionConfig;
  rightAction?: SwipeActionConfig;
  disabled?: boolean;
  children: ReactNode;
};
```

Update `handleReveal` to bail early if the action is absent:

```ts
const handleReveal = (target: Exclude<SwipeSide, null>) => {
  if (disabled) return;
  if (target === 'left' && !leftAction) return;
  if (target === 'right' && !rightAction) return;
  setActiveRowId(rowId);
  reveal(target);
};
```

Update the keyboard handler to skip missing sides:

```ts
const handleKeyDown = async (event: KeyboardEvent<HTMLDivElement>) => {
  if (event.key === 'Delete' && leftAction) {
    event.preventDefault();
    handleReveal('left');
    return;
  }
  if (event.key.toLowerCase() === 'e' && rightAction) {
    event.preventDefault();
    handleReveal('right');
    return;
  }
  if (event.key === 'Enter' && side === 'left' && leftAction) {
    event.preventDefault();
    await handleConfirm('left', leftAction.onExecute);
    return;
  }
  if (event.key === 'Enter' && side === 'right' && rightAction) {
    event.preventDefault();
    await handleConfirm('right', rightAction.onExecute);
    return;
  }
  if (event.key === 'Escape') {
    close();
    setActiveRowId(null);
  }
};
```

Update the render to only show each action side when its prop exists:

```tsx
<div className="swipe-row__actions swipe-row__actions--left">
  {side === 'left' && leftAction && (
    <button
      type="button"
      className={`swipe-row__action${leftAction.destructive ? ' is-destructive' : ''}`}
      aria-label={getRevealLabel(leftAction, phase)}
      onClick={() => handleConfirm('left', leftAction.onExecute)}
    >
      {getRevealLabel(leftAction, phase)}
    </button>
  )}
</div>
<div className="swipe-row__actions swipe-row__actions--right">
  {side === 'right' && rightAction && (
    <button
      type="button"
      className="swipe-row__action"
      aria-label={getRevealLabel(rightAction, phase)}
      onClick={() => handleConfirm('right', rightAction.onExecute)}
    >
      {getRevealLabel(rightAction, phase)}
    </button>
  )}
</div>
```

Also update the reveal buttons to only render when the corresponding action exists:

```tsx
<div className={`swipe-row__body${isOpen ? ' is-open' : ''}`}>
  {leftAction && (
    <button
      type="button"
      className="swipe-row__reveal swipe-row__reveal--left"
      aria-label="Reveal delete action"
      onClick={() => handleReveal('left')}
    />
  )}
  {rightAction && (
    <button
      type="button"
      className="swipe-row__reveal swipe-row__reveal--right"
      aria-label="Reveal edit action"
      onClick={() => handleReveal('right')}
    />
  )}
  {children}
</div>
```

- [ ] **Step 3: Run existing tests — they must still pass**

```bash
cd frontend && pnpm test src/components/swipe/__tests__/SwipeActionRow.test.tsx
```

Expected: all 3 tests pass.

- [ ] **Step 4: Commit**

```bash
git add frontend/src/components/swipe/SwipeActionRow.tsx
git commit -m "feat: make SwipeActionRow left/right actions optional"
```

---

## Task 4: CSS — swipe row styles + button hover states

**Files:**
- Modify: `src/styles/globals.css`

Add the swipe row CSS and fill the empty hover state blocks. All additions go at the end of the file (before the `@keyframes` section works fine, or append after the last existing rule).

- [ ] **Step 1: Add swipe row CSS**

Append the following block to `src/styles/globals.css` (after the last existing rule, before `@keyframes`):

```css
/* ─── Swipe action row ─────────────────────────────── */

.swipe-row {
  position: relative;
  overflow: hidden;
}

.swipe-row__actions {
  position: absolute;
  top: 0;
  bottom: 0;
  display: flex;
  align-items: stretch;
  min-width: 92px;
}

.swipe-row__actions--left {
  left: 0;
}

.swipe-row__actions--right {
  right: 0;
}

.swipe-row__action {
  min-width: 92px;
  padding: 0 18px;
  border: 0;
  font-size: 0.82rem;
  font-weight: 700;
  cursor: pointer;
  background: var(--color-success-light);
  color: var(--color-success);
  transition: background 0.16s ease;
}

.swipe-row__action.is-destructive {
  background: var(--color-priority-high-bg);
  color: var(--color-priority-high-text);
}

.swipe-row__body {
  position: relative;
  z-index: 1;
  transition: transform 0.24s cubic-bezier(0.22, 1, 0.36, 1);
}

.is-right-open .swipe-row__body.is-open {
  transform: translateX(-92px);
}

.is-left-open .swipe-row__body.is-open {
  transform: translateX(92px);
}

.swipe-row__reveal--left,
.swipe-row__reveal--right {
  position: absolute;
  top: 0;
  bottom: 0;
  width: 40%;
  background: transparent;
  border: 0;
  z-index: 2;
  cursor: grab;
}

.swipe-row__reveal--left  { left: 0; }
.swipe-row__reveal--right { right: 0; }
```

- [ ] **Step 2: Fill the empty button hover states**

Find the empty hover block (around line 292 — the rule for `.pill-button:hover, .secondary-button:hover, .primary-button:hover, .ghost-button:hover { }`). Replace that empty block and add specifics:

```css
.primary-button:hover {
  filter: brightness(1.06);
  transform: scale(1.01);
}

.secondary-button:hover {
  border-color: var(--color-accent-light);
}

.ghost-button:hover {
  background: var(--color-accent-wash);
}

.pill-button:hover {
  background: var(--color-accent-wash);
  border-color: var(--color-accent-light);
}
```

- [ ] **Step 3: Run tests to confirm nothing regressed**

```bash
cd frontend && pnpm test
```

Expected: all tests pass.

- [ ] **Step 4: Commit**

```bash
git add frontend/src/styles/globals.css
git commit -m "feat: add swipe row CSS and fill button hover states"
```

---

## Task 5: CSS — theme consolidation

This is the largest single task. Read the whole task before starting.

**Files:**
- Modify: `src/styles/globals.css`
- Modify: `index.html`

**What we're doing:** Replace the dual-theme system (two `@theme` blocks, ~22 hardcoded rust-orange values, ~20 warm-tan border values, mixed warm backgrounds) with a single clean indigo/slate system. The second `@theme` block (from the `/* Unified UI system overrides */` comment at ~line 1443 onward) will be deleted — but it contains ~15 CSS classes that are only defined there and used by components. Those must be extracted first.

- [ ] **Step 1: Extract the "preserve" classes from the override section**

Before deleting anything, read `globals.css` lines 1443–1990 and extract the following selectors **verbatim** into a temporary holding area (e.g., a new file or clipboard). These are used by components and don't exist in the first section:

```
.sheet-eyebrow
.desktop-toolbar__eyebrow
.empty-shell__eyebrow
.card-meta__item
.card-meta__count
.empty-shell__inner
.empty-shell__mark
.empty-shell__copy
.quick-add__header
.quick-add__actions
.sheet-header
.sheet-title
.sheet-copy
.label-row-item__meta
.label-row-item__dot
.label-row-item__count
.onboarding__header
.onboarding__mark
.onboarding__eyebrow
.onboarding__title
.onboarding__copy
.onboarding__action
.desktop-toolbar__mark
```

Also extract the `@media (max-width: 768px)` block at the very end of the override section — it has useful responsive rules for `.board-hero__title` and `.sheet-title`.

- [ ] **Step 2: Replace the `@theme` block**

Replace the entire first `@theme` block (lines 3–43) with:

```css
@theme {
  --color-bg: #f8fafc;
  --color-surface: #ffffff;
  --color-surface-strong: #ffffff;
  --color-border: #e2e8f0;
  --color-border-strong: #cbd5e1;
  --color-text: #0f172a;
  --color-text-secondary: #475569;
  --color-text-tertiary: #94a3b8;
  --color-accent: #4f46e5;
  --color-accent-strong: #4338ca;
  --color-accent-light: #e0e7ff;
  --color-accent-wash: #eef2ff;
  --color-success: #1e7a53;
  --color-success-light: #e7f4ee;
  --color-priority-high-bg: #fef2f2;
  --color-priority-high-text: #dc2626;
  --color-priority-med-bg: #fffbeb;
  --color-priority-med-text: #d97706;
  --color-priority-low-bg: #f0fdf4;
  --color-priority-low-text: #15803d;
  --color-label-work-bg: #EEF2FF;
  --color-label-work-text: #6366F1;
  --color-label-urgent-bg: #FEF2F2;
  --color-label-urgent-text: #DC2626;
  --color-label-meeting-bg: #FFFBEB;
  --color-label-meeting-text: #D97706;
  --color-label-personal-bg: #F0FDF4;
  --color-label-personal-text: #16A34A;
  --color-label-health-bg: #FFFBEB;
  --color-label-health-text: #CA8A04;
  --color-label-finance-bg: #F0F9FF;
  --color-label-finance-text: #0284C7;
  --radius-card: 12px;
  --radius-sm: 12px;
  --radius-pill: 999px;
  --shadow-card-sm: 0 1px 2px rgba(0,0,0,0.05);
  --shadow-card-md: 0 4px 6px rgba(0,0,0,0.05);
  --shadow-card-lg: 0 10px 15px rgba(0,0,0,0.05);
  --font-sans: 'Plus Jakarta Sans', 'Segoe UI', sans-serif;
}
```

- [ ] **Step 3: Replace warm-tan border values**

All `rgba(185, 159, 130, ...)` values are warm-tan borders from the old design. Replace them with neutral values:

Run this in your editor's find-replace (regex mode):

- Find: `rgba(185, 159, 130, 0\.\d+)` → Replace: `var(--color-border)`

This covers all ~20 instances. After replacement, spot-check that `.topbar`, `.search-input`, `.reminder-card`, `.tray`, `.labels-panel` borders look correct.

Also fix the two background uses:
- Line ~186: `.topbar` background `rgba(255, 250, 244, 0.76)` → `rgba(248, 250, 252, 0.9)`
- Line ~1149: `.onboarding__progress` background `rgba(185, 159, 130, 0.18)` → already replaced by `var(--color-border)` above

- [ ] **Step 4: Replace rust-orange accent values**

All `rgba(190, 91, 49, ...)` are leftover from the old orange accent. Replace with indigo equivalents using these rules:

| Old pattern | Replace with |
|---|---|
| `rgba(190, 91, 49, 0.1)` | `rgba(79, 70, 229, 0.1)` |
| `rgba(190, 91, 49, 0.12)` | `rgba(79, 70, 229, 0.12)` |
| `rgba(190, 91, 49, 0.22)` | `rgba(79, 70, 229, 0.22)` |
| `rgba(190, 91, 49, 0.24)` | `rgba(79, 70, 229, 0.22)` |
| `rgba(190, 91, 49, 0.26)` | `rgba(79, 70, 229, 0.22)` |
| `rgba(190, 91, 49, 0.28)` | `rgba(79, 70, 229, 0.28)` |
| `rgba(190, 91, 49, 0.32)` | `rgba(79, 70, 229, 0.28)` |
| `rgba(190, 91, 49, 0.34)` | `rgba(79, 70, 229, 0.3)` |
| `rgba(190, 91, 49, 0.35)` | `rgba(79, 70, 229, 0.3)` |
| `rgba(190, 91, 49, 0.4)` | `rgba(79, 70, 229, 0.4)` |
| `rgba(190, 91, 49, 0.42)` | `rgba(79, 70, 229, 0.3)` |
| `rgba(190, 91, 49, 0.5)` | `rgba(79, 70, 229, 0.5)` |
| `rgba(190, 91, 49, 0.55)` | `rgba(79, 70, 229, 0.5)` |
| `rgba(190, 91, 49, 0.72)` | `var(--color-accent)` |

Run find-replace for each row. There are 23 total instances (verified by grep).

- [ ] **Step 5: Replace warm background tints**

Find and replace these warm background colors used in `.topbar`, `.quick-add__panel`, `.tray`, `.reminder-card`:

| Old | Replace with |
|---|---|
| `rgba(255, 253, 248, 0.88)` | `#ffffff` |
| `rgba(255, 252, 247, 0.96)` | `rgba(255, 255, 255, 0.96)` |
| `rgba(255, 252, 247, 0.74)` | `rgba(248, 250, 252, 0.74)` |
| `rgba(255, 252, 247, 0.96)` | `rgba(255, 255, 255, 0.96)` |
| `rgba(255, 252, 247, 0.72)` | `rgba(248, 250, 252, 0.72)` |
| `rgba(255, 252, 247, 0.84)` | `#ffffff` |
| `rgba(50, 31, 15, 0.22)` | `rgba(15, 23, 42, 0.22)` |
| `rgba(10, 10, 18, 0.32)` | `rgba(15, 23, 42, 0.3)` |
| `rgba(29, 25, 22, 0.9)` | `rgba(15, 23, 42, 0.92)` |
| `rgba(24, 79, 57, 0.92)` | `rgba(30, 122, 83, 0.92)` |
| `#152233` → any hardcoded near-black warm | `#0f172a` |

- [ ] **Step 6: Remove `font-family: var(--font-display)` references**

Search for `var(--font-display)` in `globals.css`. Remove the `font-family` declaration from each rule that uses it — the element will inherit `var(--font-sans)` from `body`. Do not change `font-size`, `font-weight`, or `letter-spacing` on those rules.

The affected rules include `.card-title`, `.sheet-title`, `.desktop-toolbar__title`, `.empty-shell__title`, `.onboarding__title`, `.board-hero__title`, `.board-stat__value`, `.tray-brand-name`.

- [ ] **Step 7: Delete the override section**

Delete everything from the `/* Unified UI system overrides */` comment (~line 1443) through the end of the file. This removes the second `@theme` block and all class overrides.

- [ ] **Step 8: Re-add the extracted "preserve" classes**

Paste the CSS classes extracted in Step 1 back in, at the end of the file (before the `@keyframes` block). Clean them up as you paste: they came from the override section so their color values reference teal (`rgba(15, 118, 110, ...)`) — update any teal values to the correct indigo tokens. Specifically:

- `rgba(15, 118, 110, ...)` borders/backgrounds → `rgba(79, 70, 229, ...)` at similar opacity, or `var(--color-border)`
- `var(--color-accent)` references are fine — they now resolve to indigo

Here are the classes with their correct final definitions:

```css
/* ─── Preserved from unified pass ─────────────────── */

.sheet-eyebrow,
.desktop-toolbar__eyebrow,
.empty-shell__eyebrow {
  font-size: 0.72rem;
  font-weight: 800;
  letter-spacing: 0.14em;
  text-transform: uppercase;
  color: var(--color-text-tertiary);
}

.card-meta__item {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}

.card-meta__count {
  color: var(--color-text-tertiary);
}

.empty-shell__inner {
  max-width: 520px;
}

.empty-shell__mark {
  display: flex;
  justify-content: center;
  margin-bottom: 18px;
}

.empty-shell__copy {
  margin: 12px auto 0;
  max-width: 42ch;
  color: var(--color-text-secondary);
  line-height: 1.7;
}

.quick-add__header {
  margin-bottom: 18px;
}

.quick-add__actions {
  display: flex;
  gap: 10px;
  align-items: flex-end;
  justify-content: flex-end;
  flex-wrap: wrap;
}

.sheet-header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
}

.sheet-title {
  margin-top: 8px;
  font-size: 1.6rem;
  font-weight: 700;
  line-height: 1.1;
  letter-spacing: -0.04em;
}

.sheet-copy {
  margin-top: 8px;
  color: var(--color-text-secondary);
  line-height: 1.6;
  font-size: 0.93rem;
}

.label-row-item__meta {
  display: flex;
  align-items: center;
  gap: 8px;
}

.label-row-item__dot {
  width: 8px;
  height: 8px;
  border-radius: 4px;
  flex: none;
}

.label-row-item__count {
  font-size: 0.82rem;
  color: var(--color-text-tertiary);
}

.onboarding__header {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-bottom: 12px;
}

.onboarding__mark {
  width: 40px;
  height: 40px;
  border-radius: 14px;
  background: var(--color-accent);
  display: flex;
  align-items: center;
  justify-content: center;
  color: white;
  flex: none;
}

.onboarding__eyebrow {
  font-size: 0.88rem;
  font-weight: 700;
  color: var(--color-text);
}

.onboarding__title {
  font-size: 1.5rem;
  font-weight: 700;
  letter-spacing: -0.04em;
  line-height: 1.1;
  margin-bottom: 8px;
}

.onboarding__copy {
  font-size: 0.95rem;
  color: var(--color-text-secondary);
  line-height: 1.6;
}

.onboarding__action {
  margin-top: 16px;
  padding: 0;
  height: auto;
  color: var(--color-accent-strong);
  font-weight: 700;
}

.onboarding__progress-bar {
  height: 100%;
  background: var(--color-accent-light);
  transition: width 0.1s linear;
}

.desktop-toolbar__mark {
  width: 42px;
  height: 42px;
  border-radius: 14px;
  display: grid;
  place-items: center;
  background: var(--color-accent);
  color: white;
  flex: none;
}

@media (max-width: 768px) {
  .sheet-title,
  .desktop-toolbar__title,
  .empty-shell__title {
    font-size: 1.4rem;
  }
}
```

- [ ] **Step 9: Update `index.html` font link**

In `frontend/index.html`, replace the existing font links:

```html
<link rel="preconnect" href="https://fonts.googleapis.com" />
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin />
<link href="https://fonts.googleapis.com/css2?family=Plus+Jakarta+Sans:wght@400;600;700;800&display=swap" rel="stylesheet" />
```

- [ ] **Step 10: Run all tests and compile check**

```bash
cd frontend && pnpm exec tsc --noEmit && pnpm test
```

Expected: zero TS errors, all tests pass.

- [ ] **Step 11: Commit**

```bash
git add frontend/src/styles/globals.css frontend/index.html
git commit -m "feat: consolidate CSS to single indigo/slate theme, remove dual-@theme system"
```

---

## Task 6: Fix OnboardingCard — use CSS classes

**Files:**
- Modify: `src/components/OnboardingCard.tsx`

- [ ] **Step 1: Rewrite `OnboardingCard.tsx`**

Replace the component with this version (no inline styles, uses CSS classes):

```tsx
import { useEffect, useState } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { useStore } from '../store/use-store';

export function OnboardingCard() {
  const setHasSeenOnboarding = useStore((s) => s.setHasSeenOnboarding);
  const [visible, setVisible] = useState(true);
  const [progressWidth, setProgressWidth] = useState(100);

  useEffect(() => {
    const timer = setTimeout(() => {
      setVisible(false);
      setProgressWidth(0);
      setTimeout(() => setHasSeenOnboarding(true), 500);
    }, 5000);

    const interval = setInterval(() => {
      setProgressWidth((prev) => Math.max(0, prev - 2));
    }, 100);

    return () => {
      clearTimeout(timer);
      clearInterval(interval);
    };
  }, [setHasSeenOnboarding]);

  return (
    <AnimatePresence>
      {visible && (
        <motion.div
          initial={{ opacity: 0, y: 40, scale: 0.96 }}
          animate={{ opacity: 1, y: 0, scale: 1 }}
          exit={{ opacity: 0, y: 20, scale: 0.96 }}
          transition={{ duration: 0.4, ease: 'easeOut' }}
          className="onboarding"
        >
          <div className="onboarding__panel">
            <div className="onboarding__content">
              <div className="onboarding__header">
                <div className="onboarding__mark" aria-hidden="true">
                  <svg width="18" height="18" viewBox="0 0 18 18" fill="none">
                    <path
                      d="M3 5L7.5 9L3 13"
                      stroke="white"
                      strokeWidth="2"
                      strokeLinecap="round"
                      strokeLinejoin="round"
                    />
                    <path d="M10 13H15" stroke="white" strokeWidth="2" strokeLinecap="round" />
                  </svg>
                </div>
                <span className="onboarding__eyebrow">Welcome to Actio</span>
              </div>
              <p className="onboarding__title">Capture first, organize second.</p>
              <p className="onboarding__copy">
                Your board is set up for fast scanning, quick completion, and label-based focus.
                Start talking and refine later.
              </p>
              <button
                onClick={() => { setVisible(false); setHasSeenOnboarding(true); }}
                type="button"
                className="ghost-button onboarding__action"
              >
                Got it →
              </button>
            </div>
            <div className="onboarding__progress">
              <div
                className="onboarding__progress-bar"
                style={{ width: `${progressWidth}%` }}
              />
            </div>
          </div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
```

- [ ] **Step 2: Verify TypeScript compiles**

```bash
cd frontend && pnpm exec tsc --noEmit
```

Expected: zero errors.

- [ ] **Step 3: Commit**

```bash
git add frontend/src/components/OnboardingCard.tsx
git commit -m "fix: use CSS classes in OnboardingCard instead of inline styles"
```

---

## Task 7: Fix Card — `getLabelById` call + swap drag for SwipeActionRow

**Files:**
- Modify: `src/components/Card.tsx`
- Modify: `src/components/Board.tsx`

- [ ] **Step 1: Rewrite `Card.tsx`**

Replace the full file:

```tsx
import type { Reminder } from '../types';
import { useStore } from '../store/use-store';
import { BUILTIN_LABELS, getLabelById } from '../utils/labels';
import { formatTimeShort } from '../utils/time';
import { AnimatePresence, motion } from 'framer-motion';
import { SwipeActionRow } from './swipe/SwipeActionRow';

interface CardProps {
  reminder: Reminder;
  isExpanded: boolean;
  onToggle: () => void;
}

const lineClampStyle: React.CSSProperties = {
  display: '-webkit-box',
  WebkitLineClamp: 2,
  WebkitBoxOrient: 'vertical',
  overflow: 'hidden',
};

export function Card({ reminder, isExpanded, onToggle }: CardProps) {
  const setFilter = useStore((s) => s.setFilter);
  const archiveReminder = useStore((s) => s.archiveReminder);
  const setFeedback = useStore((s) => s.setFeedback);
  const labels = useStore((s) => s.labels);
  const highlightedCardId = useStore((s) => s.ui.highlightedCardId);
  const { title, description, priority: p, labels: labelIds, dueTime, transcript, context } = reminder;
  const displayLabels = labelIds.slice(0, 3);
  const timeDisplay = dueTime ? formatTimeShort(dueTime) : 'No deadline';
  const isHighlighted = highlightedCardId === reminder.id;

  const priority = p || 'medium';
  const priorityColors = {
    high:   { accent: '#dc2626', bg: '#fef2f2', text: '#b91c1c', label: 'High priority' },
    medium: { accent: '#d97706', bg: '#fff7df', text: '#a16207', label: 'Medium priority' },
    low:    { accent: '#1e7a53', bg: '#edf9f1', text: '#166534', label: 'Low priority' },
  }[priority];

  return (
    <motion.div
      layout
      initial={{ opacity: 0, y: 30 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, scale: 0.8, transition: { duration: 0.15 } }}
    >
      <SwipeActionRow
        rowId={reminder.id}
        rightAction={{
          label: 'Done',
          confirmLabel: 'Confirm done',
          onExecute: () => {
            archiveReminder(reminder.id);
            setFeedback(`Completed: ${title}`, 'success');
          },
        }}
      >
        <article className={`reminder-card${isExpanded ? ' is-expanded' : ''}${isHighlighted ? ' is-highlighted' : ''}`}>
          <div className="reminder-accent" style={{ background: priorityColors.accent }} aria-hidden="true" />
          <div className="card-shell">
            <div className="card-head">
              <span
                className="card-badge"
                style={{ background: priorityColors.bg, color: priorityColors.text }}
              >
                {priorityColors.label}
              </span>
              <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
                {reminder.isNew && <span className="mini-badge">New</span>}
                <button
                  type="button"
                  className="card-expand"
                  onClick={onToggle}
                  aria-expanded={isExpanded}
                  aria-label={isExpanded ? `Collapse ${title}` : `Expand ${title}`}
                >
                  <span
                    aria-hidden="true"
                    style={{
                      display: 'inline-block',
                      transition: 'transform 0.18s ease',
                      transform: isExpanded ? 'rotate(180deg)' : 'rotate(0deg)',
                    }}
                  >
                    ↓
                  </span>
                </button>
              </div>
            </div>

            <div className="card-title">{title}</div>

            {description && (
              <div className="card-description" style={!isExpanded ? lineClampStyle : undefined}>
                {description}
              </div>
            )}

            <div className="card-meta">
              <div className="card-meta__item">
                <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" style={{ width: '15px', height: '15px' }}>
                  <circle cx="12" cy="12" r="10" />
                  <path d="M12 6v6l4 2" />
                </svg>
                <span>{timeDisplay}</span>
              </div>
              <span className="card-meta__count">{labelIds.length} labels</span>
            </div>

            <div className="label-row">
              {displayLabels.map((labelId) => {
                const label = getLabelById([...BUILTIN_LABELS, ...labels], labelId);
                if (!label) return null;
                return (
                  <button
                    key={labelId}
                    type="button"
                    onClick={() => setFilter({ label: labelId })}
                    className="label-chip"
                    style={{
                      background: label.bgColor,
                      color: label.color,
                      borderColor: `${label.color}22`,
                    }}
                  >
                    {label.name}
                  </button>
                );
              })}
            </div>

            <AnimatePresence>
              {isExpanded && (transcript || context) && (
                <motion.div
                  initial={{ opacity: 0, height: 0 }}
                  animate={{ opacity: 1, height: 'auto' }}
                  exit={{ opacity: 0, height: 0 }}
                  transition={{ duration: 0.2 }}
                  className="card-detail"
                >
                  {transcript && <div>{transcript}</div>}
                  {context && <div className="card-context">{context}</div>}
                </motion.div>
              )}
            </AnimatePresence>
          </div>
        </article>
      </SwipeActionRow>
    </motion.div>
  );
}
```

- [ ] **Step 2: Add `SwipeActionCoordinatorProvider` to `Board.tsx`**

In `src/components/Board.tsx`, import and wrap the `AnimatePresence` block:

```tsx
import { SwipeActionCoordinatorProvider } from './swipe/SwipeActionCoordinator';
```

Wrap the `<AnimatePresence mode="popLayout">` block:

```tsx
<div className="board-grid">
  <SwipeActionCoordinatorProvider>
    <AnimatePresence mode="popLayout">
      {sorted.map((reminder) => (
        <Card
          key={reminder.id}
          reminder={reminder}
          isExpanded={reminder.id === expandedCardId}
          onToggle={() => {
            const nextExpanded = reminder.id === expandedCardId ? null : reminder.id;
            setExpandedCard(nextExpanded);
            if (nextExpanded && reminder.isNew) {
              clearNewFlag(reminder.id);
            }
          }}
        />
      ))}
    </AnimatePresence>
  </SwipeActionCoordinatorProvider>
</div>
```

- [ ] **Step 3: Compile check + tests**

```bash
cd frontend && pnpm exec tsc --noEmit && pnpm test
```

Expected: zero errors, all tests pass.

- [ ] **Step 4: Commit**

```bash
git add frontend/src/components/Card.tsx frontend/src/components/Board.tsx
git commit -m "fix: replace framer-motion drag with SwipeActionRow, fix getLabelById call"
```

---

## Task 8: Card — expand-to-edit section

**Files:**
- Modify: `src/components/Card.tsx`
- Modify: `src/styles/globals.css`

- [ ] **Step 1: Add card-edit CSS to `globals.css`**

Append to `globals.css`:

```css
/* ─── Card expand-to-edit ──────────────────────────── */

.card-edit {
  margin-top: 16px;
  padding-top: 16px;
  border-top: 1px solid var(--color-border);
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.card-edit__row {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

.card-edit__label {
  font-size: 0.72rem;
  font-weight: 800;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  color: var(--color-text-tertiary);
  min-width: 56px;
}

.priority-btn {
  height: 30px;
  padding: 0 12px;
  border-radius: 8px;
  border: 1px solid var(--color-border);
  background: var(--color-bg);
  color: var(--color-text-secondary);
  font-size: 0.78rem;
  font-weight: 600;
  cursor: pointer;
  transition: background 0.14s ease, border-color 0.14s ease, color 0.14s ease;
}

.label-chip--remove {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 5px 8px 5px 10px;
  border-radius: var(--radius-pill);
  font-size: 0.74rem;
  font-weight: 700;
  border: 1px solid transparent;
  cursor: pointer;
  transition: opacity 0.14s ease;
}

.label-chip--remove:hover {
  opacity: 0.75;
}

.label-add-dropdown {
  position: absolute;
  top: calc(100% + 6px);
  left: 0;
  z-index: 50;
  background: var(--color-surface);
  border: 1px solid var(--color-border);
  border-radius: 12px;
  box-shadow: var(--shadow-card-md);
  min-width: 160px;
  padding: 6px;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.label-add-dropdown__item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 7px 10px;
  border-radius: 8px;
  border: 0;
  background: transparent;
  font-size: 0.82rem;
  font-weight: 600;
  text-align: left;
  cursor: pointer;
  transition: background 0.12s ease;
}

.label-add-dropdown__item:hover {
  background: var(--color-accent-wash);
}

.label-add-trigger {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  height: 30px;
  padding: 0 10px;
  border-radius: var(--radius-pill);
  border: 1px dashed var(--color-border-strong);
  background: transparent;
  color: var(--color-text-tertiary);
  font-size: 0.74rem;
  font-weight: 700;
  cursor: pointer;
  transition: border-color 0.14s ease, color 0.14s ease;
  position: relative;
}

.label-add-trigger:hover {
  border-color: var(--color-accent-light);
  color: var(--color-accent);
}
```

- [ ] **Step 2: Add the edit section to `Card.tsx`**

In `Card.tsx`, add these imports at the top:

```tsx
import { useState, useEffect, useRef } from 'react';
import type { Priority } from '../types';
```

Add store selectors in the component body (after existing ones):

```tsx
const updateReminderInline = useStore((s) => s.updateReminderInline);
```

Add a state variable for the label dropdown:

```tsx
const [showLabelDropdown, setShowLabelDropdown] = useState(false);
const dropdownRef = useRef<HTMLDivElement>(null);
```

Add a click-outside handler:

```tsx
useEffect(() => {
  if (!showLabelDropdown) return;
  const handler = (e: MouseEvent) => {
    if (dropdownRef.current && !dropdownRef.current.contains(e.target as Node)) {
      setShowLabelDropdown(false);
    }
  };
  document.addEventListener('mousedown', handler);
  return () => document.removeEventListener('mousedown', handler);
}, [showLabelDropdown]);
```

After the existing `<AnimatePresence>` block (transcript/context), add the edit section inside `AnimatePresence`:

```tsx
<AnimatePresence>
  {isExpanded && (
    <motion.div
      key="edit"
      initial={{ opacity: 0, height: 0 }}
      animate={{ opacity: 1, height: 'auto' }}
      exit={{ opacity: 0, height: 0 }}
      transition={{ duration: 0.2 }}
      className="card-edit"
    >
      {/* Priority row */}
      <div className="card-edit__row">
        <span className="card-edit__label">Priority</span>
        {(['high', 'medium', 'low'] as Priority[]).map((p) => {
          const isActive = priority === p;
          const colors = {
            high:   { bg: '#fef2f2', text: '#b91c1c' },
            medium: { bg: '#fff7df', text: '#a16207' },
            low:    { bg: '#edf9f1', text: '#166534' },
          }[p];
          return (
            <button
              key={p}
              type="button"
              className="priority-btn"
              style={isActive ? { background: colors.bg, color: colors.text, borderColor: 'transparent' } : undefined}
              onClick={() => updateReminderInline(reminder.id, { priority: p })}
            >
              {p.charAt(0).toUpperCase() + p.slice(1)}
            </button>
          );
        })}
      </div>

      {/* Labels row */}
      <div className="card-edit__row">
        <span className="card-edit__label">Labels</span>
        {labelIds.map((labelId) => {
          const label = getLabelById([...BUILTIN_LABELS, ...labels], labelId);
          if (!label) return null;
          return (
            <button
              key={labelId}
              type="button"
              className="label-chip--remove"
              style={{ background: label.bgColor, color: label.color }}
              onClick={() =>
                updateReminderInline(reminder.id, {
                  labels: labelIds.filter((id) => id !== labelId),
                })
              }
            >
              {label.name}
              <span aria-hidden="true">×</span>
            </button>
          );
        })}
        <div style={{ position: 'relative' }} ref={dropdownRef}>
          <button
            type="button"
            className="label-add-trigger"
            onClick={() => setShowLabelDropdown((v) => !v)}
          >
            + add
          </button>
          {showLabelDropdown && (
            <div className="label-add-dropdown">
              {[...BUILTIN_LABELS, ...labels]
                .filter((l) => !labelIds.includes(l.id))
                .map((label) => (
                  <button
                    key={label.id}
                    type="button"
                    className="label-add-dropdown__item"
                    onClick={() => {
                      updateReminderInline(reminder.id, { labels: [...labelIds, label.id] });
                      setShowLabelDropdown(false);
                    }}
                  >
                    <span
                      style={{
                        width: 8,
                        height: 8,
                        borderRadius: '50%',
                        background: label.color,
                        flex: 'none',
                      }}
                    />
                    {label.name}
                  </button>
                ))}
              {[...BUILTIN_LABELS, ...labels].filter((l) => !labelIds.includes(l.id)).length === 0 && (
                <span style={{ padding: '6px 10px', fontSize: '0.8rem', color: 'var(--color-text-tertiary)' }}>
                  All labels assigned
                </span>
              )}
            </div>
          )}
        </div>
      </div>
    </motion.div>
  )}
</AnimatePresence>
```

Note: the existing `<AnimatePresence>` for transcript/context stays — add this new one as a sibling after it, still inside `.card-shell`.

- [ ] **Step 3: Close the label dropdown on Escape**

Add to the existing keyboard handler on the card-expand button, or handle globally by adding to the click-outside effect:

In the `useEffect` above, add Escape key handling:

```tsx
useEffect(() => {
  if (!showLabelDropdown) return;
  const mouseHandler = (e: MouseEvent) => {
    if (dropdownRef.current && !dropdownRef.current.contains(e.target as Node)) {
      setShowLabelDropdown(false);
    }
  };
  const keyHandler = (e: KeyboardEvent) => {
    if (e.key === 'Escape') setShowLabelDropdown(false);
  };
  document.addEventListener('mousedown', mouseHandler);
  document.addEventListener('keydown', keyHandler);
  return () => {
    document.removeEventListener('mousedown', mouseHandler);
    document.removeEventListener('keydown', keyHandler);
  };
}, [showLabelDropdown]);
```

- [ ] **Step 4: Compile + test**

```bash
cd frontend && pnpm exec tsc --noEmit && pnpm test
```

Expected: zero errors, all tests pass.

- [ ] **Step 5: Commit**

```bash
git add frontend/src/components/Card.tsx frontend/src/styles/globals.css
git commit -m "feat: add expand-to-edit section to Card (priority + label assignment)"
```

---

## Task 9: NewReminderBar — due time field

**Files:**
- Modify: `src/components/NewReminderBar.tsx`
- Create: `src/components/__tests__/NewReminderBar.test.tsx`

- [ ] **Step 1: Write the failing test**

Create `src/components/__tests__/NewReminderBar.test.tsx`:

```tsx
import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, beforeEach } from 'vitest';
import { NewReminderBar } from '../NewReminderBar';
import { useStore } from '../../store/use-store';

function renderBar() {
  useStore.getState().reset();
  useStore.getState().setNewReminderBar(true);
  return render(<NewReminderBar />);
}

describe('NewReminderBar', () => {
  beforeEach(() => {
    useStore.getState().reset();
  });

  it('submits a reminder with dueTime when the field is filled', () => {
    renderBar();
    fireEvent.change(screen.getByPlaceholderText('What needs attention?'), {
      target: { value: 'Call dentist' },
    });
    fireEvent.change(screen.getByPlaceholderText('e.g. 3pm, tomorrow 9am'), {
      target: { value: '3pm' },
    });
    fireEvent.click(screen.getByRole('button', { name: /add reminder/i }));
    const reminders = useStore.getState().reminders;
    expect(reminders).toHaveLength(1);
    expect(reminders[0].dueTime).toBe('3pm');
  });

  it('submits without dueTime when the field is empty', () => {
    renderBar();
    fireEvent.change(screen.getByPlaceholderText('What needs attention?'), {
      target: { value: 'Call dentist' },
    });
    fireEvent.click(screen.getByRole('button', { name: /add reminder/i }));
    const reminders = useStore.getState().reminders;
    expect(reminders[0].dueTime).toBeUndefined();
  });

  it('resets dueTime after successful submission', () => {
    renderBar();
    fireEvent.change(screen.getByPlaceholderText('What needs attention?'), {
      target: { value: 'Call dentist' },
    });
    fireEvent.change(screen.getByPlaceholderText('e.g. 3pm, tomorrow 9am'), {
      target: { value: '3pm' },
    });
    fireEvent.click(screen.getByRole('button', { name: /add reminder/i }));
    // bar closes — re-open it
    useStore.getState().setNewReminderBar(true);
    render(<NewReminderBar />);
    expect((screen.getByPlaceholderText('e.g. 3pm, tomorrow 9am') as HTMLInputElement).value).toBe('');
  });
});
```

- [ ] **Step 2: Run the test to confirm it fails**

```bash
cd frontend && pnpm test src/components/__tests__/NewReminderBar.test.tsx
```

Expected: FAIL — no element with placeholder `e.g. 3pm, tomorrow 9am`.

- [ ] **Step 3: Update `NewReminderBar.tsx`**

Add `dueTime` state and the new field. Replace the file:

```tsx
import { useState, useRef, useEffect } from 'react';
import { AnimatePresence, motion } from 'framer-motion';
import { useStore } from '../store/use-store';

export function NewReminderBar() {
  const show = useStore((s) => s.ui.showNewReminderBar);
  const setNewReminderBar = useStore((s) => s.setNewReminderBar);
  const addReminder = useStore((s) => s.addReminder);

  const [title, setTitle] = useState('');
  const [description, setDescription] = useState('');
  const [dueTime, setDueTime] = useState('');
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (show && inputRef.current) {
      inputRef.current.focus();
    }
  }, [show]);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setNewReminderBar(false);
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [setNewReminderBar]);

  const handleSubmit = () => {
    if (!title.trim()) return;
    addReminder({
      title: title.trim(),
      description: description.trim(),
      priority: 'medium',
      labels: [],
      createdAt: new Date().toISOString(),
      ...(dueTime.trim() ? { dueTime: dueTime.trim() } : {}),
    });
    setTitle('');
    setDescription('');
    setDueTime('');
    setNewReminderBar(false);
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
      e.preventDefault();
      handleSubmit();
    }
  };

  return (
    <AnimatePresence>
      {show && (
        <>
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            className="sheet-overlay"
            onClick={() => setNewReminderBar(false)}
          />
          <motion.div
            initial={{ y: '100%' }}
            animate={{ y: 0 }}
            exit={{ y: '100%' }}
            transition={{ type: 'spring', damping: 25, stiffness: 200 }}
            className="quick-add"
          >
            <div className="quick-add__panel">
              <div className="sheet-header quick-add__header">
                <div>
                  <div className="sheet-eyebrow">Manual capture</div>
                  <div className="sheet-title">Add a note without leaving the board</div>
                  <div className="sheet-copy">Keep the entry short. Triage and labeling can happen after capture.</div>
                </div>
                <div className="active-pill">Cmd/Ctrl + Enter to save</div>
              </div>
              <div className="quick-add__grid">
                <label>
                  <span className="field-label">Title</span>
                  <input
                    ref={inputRef}
                    type="text"
                    placeholder="What needs attention?"
                    value={title}
                    onChange={(e) => setTitle(e.target.value)}
                    onKeyDown={handleKeyDown}
                    className="field-input"
                  />
                </label>
                <label>
                  <span className="field-label">Details</span>
                  <textarea
                    rows={2}
                    placeholder="Optional context, owner, or timing"
                    value={description}
                    onChange={(e) => setDescription(e.target.value)}
                    onKeyDown={handleKeyDown}
                    className="field-input"
                  />
                </label>
                <label>
                  <span className="field-label">Due time</span>
                  <input
                    type="text"
                    placeholder="e.g. 3pm, tomorrow 9am"
                    value={dueTime}
                    onChange={(e) => setDueTime(e.target.value)}
                    onKeyDown={handleKeyDown}
                    className="field-input"
                  />
                </label>
                <div className="quick-add__actions">
                  <button type="button" onClick={() => setNewReminderBar(false)} className="secondary-button">
                    Cancel
                  </button>
                  <button type="button" onClick={handleSubmit} disabled={!title.trim()} className="primary-button">
                    Add reminder
                  </button>
                </div>
              </div>
            </div>
          </motion.div>
        </>
      )}
    </AnimatePresence>
  );
}
```

- [ ] **Step 4: Run tests**

```bash
cd frontend && pnpm test src/components/__tests__/NewReminderBar.test.tsx
```

Expected: all 3 tests pass.

- [ ] **Step 5: Full test suite**

```bash
cd frontend && pnpm test
```

Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add frontend/src/components/NewReminderBar.tsx frontend/src/components/__tests__/NewReminderBar.test.tsx
git commit -m "feat: add due time field to NewReminderBar"
```

---

## Task 10: StandbyTray — swipe to complete

**Files:**
- Modify: `src/components/StandbyTray.tsx`

- [ ] **Step 1: Update `StandbyTray.tsx`**

Add imports:

```tsx
import { SwipeActionRow } from './swipe/SwipeActionRow';
import { SwipeActionCoordinatorProvider } from './swipe/SwipeActionCoordinator';
```

Add `markDone` selector (using the new `archiveReminder`):

```tsx
const archiveReminder = useStore((s) => s.archiveReminder);
```

Wrap the tray item list in a `SwipeActionCoordinatorProvider` and each item in `SwipeActionRow`. Replace the `{topReminders.map(...)}` block and the following `<motion.div className="tray-cta">`:

```tsx
<SwipeActionCoordinatorProvider>
  {topReminders.map((reminder, index) => (
    <SwipeActionRow
      key={reminder.id}
      rowId={reminder.id}
      rightAction={{
        label: 'Done',
        confirmLabel: 'Confirm done',
        onExecute: () => archiveReminder(reminder.id),
      }}
    >
      <motion.button
        type="button"
        className="tray-item"
        initial={false}
        animate={{ opacity: expanded ? 1 : 0, x: expanded ? 0 : 8 }}
        transition={{ duration: 0.2, delay: expanded ? 0.04 + index * 0.025 : 0 }}
        onClick={() => {
          setExpandedCard(reminder.id);
          highlightCard(reminder.id);
          setExpanded(false);
          setBoardWindow(true);
        }}
      >
        <div className="tray-item-header">
          <span className="tray-item-priority" style={{ background: priorityDotColor(reminder.priority) }} />
          <span className="tray-item-title">{reminder.title}</span>
          {reminder.dueTime && <span className="tray-item-time">{formatTimeShort(reminder.dueTime)}</span>}
        </div>
      </motion.button>
    </SwipeActionRow>
  ))}

  <motion.div
    className="tray-cta"
    initial={false}
    animate={{ opacity: expanded ? 1 : 0, y: expanded ? 0 : 6 }}
    transition={{ duration: 0.2, delay: expanded ? 0.12 : 0 }}
  >
    <button
      type="button"
      className="primary-button"
      onClick={() => { setExpanded(false); setBoardWindow(true); }}
    >
      View full board
    </button>
  </motion.div>
</SwipeActionCoordinatorProvider>
```

- [ ] **Step 2: Full compile + test**

```bash
cd frontend && pnpm exec tsc --noEmit && pnpm test
```

Expected: zero errors, all tests pass.

- [ ] **Step 3: Commit**

```bash
git add frontend/src/components/StandbyTray.tsx
git commit -m "feat: swipe-to-complete on standby tray items"
```

---

## Self-Review Checklist

**Spec coverage:**
- [x] CSS theme consolidation (Task 5)
- [x] Replace `rgba(185,159,130)` warm-tan borders (Task 5, Step 3)
- [x] Replace `rgba(190,91,49)` rust-orange accent (Task 5, Step 4)
- [x] Fill empty hover states (Task 4)
- [x] Add Plus Jakarta Sans font (Task 5, Step 9)
- [x] Remove dead CSS + `--font-display` (Task 5, Steps 6-7)
- [x] Bug: `BUILTIN_LABELS` export (Task 1)
- [x] Bug: `getLabelById` signature (Task 7)
- [x] Bug: SwipeActionRow replacing Card drag (Task 7)
- [x] Bug: OnboardingCard inline styles (Task 6)
- [x] Feature: priority editing on card (Task 8)
- [x] Feature: label assignment from card (Task 8)
- [x] Feature: due time in capture form (Task 9)
- [x] Feature: tray swipe-to-complete (Task 10)
- [x] `SwipeActionCoordinatorProvider` in Board (Task 7)
- [x] `SwipeActionCoordinatorProvider` in StandbyTray (Task 10)
- [x] `archiveReminder` replaces `markDone` throughout (Tasks 2, 7, 10)
- [x] Unified `labels` field in store (Task 2)
- [x] `archivedAt` on Reminder type (Task 1)

**Type consistency check:**
- `updateReminderInline(id, patch)` — defined in Task 2, used in Tasks 8 (priority + labels)
- `archiveReminder(id)` — defined in Task 2, used in Tasks 7 (Card) and 10 (StandbyTray)
- `getLabelById([...BUILTIN_LABELS, ...labels], id)` — consistent in Tasks 7 and 8
- `SwipeActionRow` props `rowId`, `rightAction: { label, confirmLabel, onExecute }` — consistent in Tasks 3, 7, 10
- `Reminder.archivedAt: string | null` — added in Task 1, used in Task 2 (`filterReminders`)
