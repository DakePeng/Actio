# Frontend Polish & Feature Pass — Design Spec

**Date:** 2026-04-07  
**Scope:** Bug fixes, CSS theme consolidation, and four new features  
**Approach:** Layer 1 (foundation) then Layer 2 (features)

---

## Background

A full audit of the frontend revealed three critical bugs, five style inconsistencies rooted in a layered CSS theme system, and six feature gaps. This spec covers the agreed subset: all bugs, full CSS consolidation, and four features (priority editing, label assignment, due time in capture, tray swipe-to-complete).

---

## Layer 1: Foundation

### 1a — CSS Theme Consolidation

The codebase currently has two `@theme` blocks in `globals.css`. The first (lines 3–43) defines an indigo/slate palette with Plus Jakarta Sans. The second (lines 1445–1990+) overrides it with a teal/green palette, Manrope sans, and Newsreader serif. This creates ~550 lines of overrides that shadow the original declarations and leave ~22 hardcoded `rgba(190, 91, 49, ...)` rust-orange values from the original theme unupdated.

**Resolution:** Merge into a single `@theme` block using the indigo/slate palette. Remove the second block and all its overrides entirely.

**Final token set:**

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
  /* label colors preserved as-is */
  --radius-card: 12px;
  --radius-sm: 12px;
  --radius-pill: 999px;
  --shadow-card-sm: 0 1px 2px rgba(0,0,0,0.05);
  --shadow-card-md: 0 4px 6px rgba(0,0,0,0.05);
  --shadow-card-lg: 0 10px 15px rgba(0,0,0,0.05);
  --font-sans: 'Plus Jakarta Sans', 'Segoe UI', sans-serif;
}
```

No `--font-display`. All typography uses `--font-sans`. Card titles, hero text, and sheet titles that currently use `font-family: var(--font-display)` revert to `--font-sans` with tight `letter-spacing: -0.04em` and `font-weight: 700` to maintain visual weight.

**Replace all hardcoded rust-orange values** (`rgba(190, 91, 49, ...)`) with the appropriate token:
- Focus rings / borders → `var(--color-accent)` or `var(--color-accent-light)`  
- Button shadows → `rgba(79, 70, 229, 0.22)` (indigo)  
- `cardSpotlight` animation border/background tints → indigo equivalents

**Add to `index.html`:**
```html
<link rel="preconnect" href="https://fonts.googleapis.com">
<link href="https://fonts.googleapis.com/css2?family=Plus+Jakarta+Sans:wght@400;600;700;800&display=swap" rel="stylesheet">
```

**Fill empty hover states:**
```css
.primary-button:hover   { filter: brightness(1.06); transform: scale(1.01); }
.secondary-button:hover { border-color: var(--color-accent-light); }
.ghost-button:hover     { background: var(--color-accent-wash); }
.pill-button:hover      { background: var(--color-accent-wash); border-color: var(--color-accent-light); }
```

**Remove dead CSS:**
- `.board-hero::before` transparent overlay
- `.ambient-orb--left` / `.ambient-orb--right` transparent overrides
- `.filters-panel` class (no component renders this element)
- All `@theme` override block and trailing overrides (~lines 1443–1990)

---

### 1b — Bug Fixes

**Bug 1: `BUILTIN_LABELS` export missing**

`labels.ts` exports `INITIAL_LABELS`. `LabelsPanel.tsx` imports `BUILTIN_LABELS`. Fix: rename export in `labels.ts` to `BUILTIN_LABELS`. Update `LabelsPanel.tsx` import accordingly (already uses the correct name).

**Bug 2: `getLabelById` signature mismatch**

`labels.ts` signature: `getLabelById(labels: Label[], id: string)`.  
`Card.tsx` calls: `getLabelById(labelId)` — one argument, wrong.

Fix in `Card.tsx`:
```ts
import { BUILTIN_LABELS, getLabelById } from '../utils/labels';
// ...
const label = getLabelById(BUILTIN_LABELS, labelId);
```

Custom labels are not yet assignable from cards in Layer 1; they become available in Layer 2 when the label assignment feature ships.

**Bug 3: Card drag — replace with SwipeActionRow**

`Card.tsx` uses Framer Motion `drag="x"` with `useMotionValue` / `useTransform` for swipe-to-dismiss. The `SwipeActionRow` + `SwipeActionCoordinator` system in `components/swipe/` provides the same behavior with coordinator-managed mutual exclusivity.

`SwipeActionRow` currently requires both `leftAction` and `rightAction` (both non-optional). To support a right-only "done" action, make both props optional in `SwipeActionRow` — when a side's prop is absent, that side renders nothing and swipe/key triggers for that side are ignored.

Remove from `Card.tsx`:
- `x`, `rot`, `opac`, `dragFeedbackOpacity`, `dragFeedbackScale` motion values
- `drag`, `dragConstraints`, `onDragEnd` props on the outer `motion.div`
- The drag-feedback overlay `motion.div` (green "Mark done" overlay)
- `whileTap={{ cursor: 'grabbing' }}` and `style={{ cursor: 'grab' }}`

Replace: wrap the `<article>` in `<SwipeActionRow rowId={reminder.id} rightAction={{ label: 'Done', confirmLabel: 'Confirm', onExecute: () => markDone(reminder.id) }}>`. The outer `motion.div` for layout animation (`layout`, `initial`, `animate`, `exit`) stays.

The interaction is two-step: swipe right reveals a "Done" button; clicking it confirms and executes. This matches `SwipeActionRow`'s existing `phase: 'confirm'` flow — no changes to the hook.

`Board.tsx` must wrap its card list in `<SwipeActionCoordinatorProvider>` so mutual exclusivity works across cards. Add the import and wrap the `AnimatePresence` block.

The done-action reveal style (green `#e4f9f4` background + checkmark) moves into `SwipeActionRow` as its standard done-action visual, replacing the ad-hoc inline implementation.

**Bug 4: `OnboardingCard` inline styles → CSS classes**

`OnboardingCard.tsx` ignores `.onboarding__header`, `.onboarding__mark`, `.onboarding__eyebrow`, `.onboarding__title`, `.onboarding__copy`, `.onboarding__action` class names defined in CSS and uses raw inline styles instead.

Fix: replace inline styles with the existing class names. The progress bar motion div can keep its inline `width` style (dynamic value) but its static styles move to `.onboarding__progress-bar` in CSS.

---

## Layer 2: Features

### 2a — Card: Expand-to-Edit

When a card is expanded, an "Edit" section appears below the transcript/context block, separated by a divider (`border-top: 1px solid var(--color-border)`).

**Priority row:**
```
Priority   [High]  [Medium]  [Low]
```
Three segmented buttons. Active button uses the priority's bg/text colors from `priorityColors` in `Card.tsx`. Inactive buttons use neutral `--color-bg` bg with `--color-border` border. Clicking fires `setPriority(reminder.id, newPriority)`.

**Labels row:**
```
Labels     [Work ×]  [Meeting ×]  [+ add ▾]
```
Existing label chips gain a `×` button that calls `setLabels(id, labels.filter(...))`. The `+ add` chip opens an inline dropdown listing all labels (builtin + custom) not already assigned. Selecting one calls `setLabels(id, [...current, newId])` and closes the dropdown. Dropdown closes on outside click or Escape.

`getLabelById` in this context uses `[...BUILTIN_LABELS, ...customLabels]` as the label source.

**New store actions:**
```ts
setPriority: (id: string, priority: Priority) => void;
setLabels: (id: string, labels: string[]) => void;
```
Both update the matching reminder in `state.reminders` and call `pushFeedback`.

**CSS additions:**
- `.card-edit` — edit section container with top border and padding
- `.card-edit__label` — small uppercase row label (`font-size: 0.72rem`, `--color-text-tertiary`)
- `.card-edit__row` — flex row with gap for priority/label controls
- `.priority-btn` — segmented priority button base styles
- `.priority-btn.is-active` — active state (dynamic color via inline style for bg/text)
- `.label-add-dropdown` — absolute-positioned dropdown for label picker

---

### 2b — NewReminderBar: Due Time Field

Add a third optional field to the capture form between "Details" and the action buttons:

```
Due time   [e.g. 3pm, tomorrow 9am          ]
```

- `<input type="text">` with placeholder `e.g. 3pm, tomorrow 9am`
- Optional — no validation, stored as-is
- State: `const [dueTime, setDueTime] = useState('')`
- On submit: `dueTime: dueTime.trim() || undefined` passed to `addReminder`
- On close/cancel: reset `dueTime` to `''` alongside title and description

No parsing library. `formatTimeShort` already handles free-form strings gracefully. The `Reminder.dueTime` type is already `string | undefined`.

---

### 2c — StandbyTray: Swipe to Complete

Wrap each tray item in `SwipeActionRow` with the same done-action configuration as cards:

```tsx
<SwipeActionCoordinatorProvider>
  {topReminders.map((reminder) => (
    <SwipeActionRow
      key={reminder.id}
      rowId={reminder.id}
      rightAction={{ label: 'Done', confirmLabel: 'Confirm', onExecute: () => markDone(reminder.id) }}
    >
      <motion.button className="tray-item" onClick={...}>
        ...
      </motion.button>
    </SwipeActionRow>
  ))}
</SwipeActionCoordinatorProvider>
```

- Import `markDone` from store in `StandbyTray.tsx`
- Import `SwipeActionRow` and `SwipeActionCoordinatorProvider` from `../components/swipe/`
- The `onClick` handler (open board, highlight card) is unaffected — tap and swipe remain independent
- The `SwipeActionCoordinatorProvider` wraps only the tray item list, separate from the one in `Board.tsx` — each list manages its own active row independently

---

## What Is Explicitly Out of Scope

- Sort options (by date / due time)
- "Mark done" button on expanded card
- Batch actions
- User identity / avatar (hardcoded "JD" stays)
- Date parsing for due time input
- Tray item label/priority display changes

---

## File Change Summary

| File | Change |
|---|---|
| `frontend/index.html` | Add Plus Jakarta Sans Google Fonts link |
| `frontend/src/styles/globals.css` | Merge themes, tokenize colors, fill hovers, remove dead code |
| `frontend/src/utils/labels.ts` | Rename `INITIAL_LABELS` → `BUILTIN_LABELS` |
| `frontend/src/components/Card.tsx` | Fix `getLabelById` call; swap drag for `SwipeActionRow`; add expand-to-edit section |
| `frontend/src/components/Board.tsx` | Wrap card list in `SwipeActionCoordinatorProvider` |
| `frontend/src/components/OnboardingCard.tsx` | Replace inline styles with CSS classes |
| `frontend/src/components/NewReminderBar.tsx` | Add due time field |
| `frontend/src/components/StandbyTray.tsx` | Wrap tray items in `SwipeActionRow`; import `markDone` |
| `frontend/src/components/swipe/SwipeActionRow.tsx` | Add standard done-action reveal style |
| `frontend/src/store/use-store.ts` | Add `setPriority` and `setLabels` actions |
| `frontend/src/styles/globals.css` (additions) | `.card-edit`, `.card-edit__label`, `.card-edit__row`, `.priority-btn`, `.label-add-dropdown` |
