# Unified Swipe Interaction Design

Date: 2026-04-06  
Status: Approved in brainstorming, ready for implementation planning

## 1. Problem Statement

Actio currently has uneven horizontal interactions:
- Cards support horizontal drag, but behavior is tied to a "mark done" action.
- Standby tray items do not support swipe actions.
- Label rows do not support swipe actions.

The target behavior is uniform interaction logic across cards, tray items, and label rows:
- Swipe left to delete.
- Swipe right to edit.
- Deletion is second-tap confirm.
- Card/tray delete archives item (restorable).
- Label delete is hard delete.
- Edit is inline on the current surface.

## 2. Goals And Non-Goals

Goals:
- One gesture model shared across all three row types.
- One confirmation model (`tap action once to arm`, `tap again to execute`).
- Input-aware thresholds (lower for touch/trackpad-like gestures, higher for mouse drag).
- Keep domain mutations in store logic, not UI components.

Non-goals:
- No modal edit flow for this feature.
- No undo toast workflow for deletes.
- No backend persistence redesign in this spec (store-level behavior first).

## 3. Scope

In scope surfaces:
- Board cards (`Card`)
- Standby tray rows (`tray-item`)
- Labels panel rows (`label-row-item`)

In scope outcomes:
- Shared swipe interaction primitive
- Archive flow for reminder entities
- Hard delete flow for labels with cascade removal from reminders
- Inline quick edit on the same surface

Out of scope:
- Archive analytics/reporting
- Bulk multi-select actions

## 4. Interaction Contract (Uniform)

For every swipe-enabled row:
1. Horizontal drag reveals side action.
2. Left reveal action is `Delete`.
3. Right reveal action is `Edit`.
4. Action is never executed on reveal alone.
5. First tap on revealed action arms confirmation.
6. Second tap on the same action executes.
7. Tap outside row (or open another row) closes reveal/confirm state.

Single-open rule:
- Only one row may remain revealed at a time across the active surface.

Conflict rule:
- If vertical intent dominates gesture, scroll wins and swipe is canceled.

## 5. Gesture Profiles And Thresholds

Two runtime gesture profiles:
- `indirect-pointer` (mouse drag): higher reveal threshold.
- `direct-or-gesture` (touch/pen drag and horizontal trackpad gesture interpretation): lower reveal threshold.

Profile resolution:
- Pointer drag with `pointerType === "mouse"` uses `indirect-pointer`.
- Pointer drag with `pointerType === "touch"` or `pointerType === "pen"` uses `direct-or-gesture`.
- Horizontal trackpad gesture path (wheel `deltaX` dominant over `deltaY`) uses `direct-or-gesture`.

Threshold values:
- `mouseRevealThresholdPx = 72`
- `touchOrGestureRevealThresholdPx = 52`
- `closeThresholdPx = 24` (if released near center, snap closed)
- `horizontalIntentRatio = 1.25` (`abs(dx) / max(abs(dy), 1)`)

Snap points:
- `x = 0` closed
- `x = -actionWidth` delete revealed
- `x = +actionWidth` edit revealed

Recommended action width:
- `actionWidth = 92px` for consistent touch target and text fit

Animation timings:
- Drag follow: immediate
- Snap/release: 170-220ms spring
- Confirm-state emphasis: 120ms color/label transition
- Remove/archive exit: 180-240ms

## 6. UI States (Per Row)

State machine:
- `idle`
- `dragging`
- `revealed-delete`
- `revealed-edit`
- `confirm-delete`
- `confirm-edit`
- `executing`

Transitions:
- `idle -> dragging`: pointer/gesture starts and passes horizontal intent.
- `dragging -> revealed-*`: release past reveal threshold.
- `revealed-* -> confirm-*`: first tap on revealed action.
- `confirm-* -> executing`: second tap on same action.
- `revealed-* | confirm-* -> idle`: outside tap, escape, or another row opens.

Row behavior while `executing`:
- Disable further pointer input.
- Show in-row busy affordance (brief).
- Then either complete mutation + close row, or fail + return to `revealed-*`.

## 7. Domain Action Mapping

Reminder entities (card and tray item):
- Swipe left delete action: archive reminder, not hard delete.
- Swipe right edit action: inline quick edit for reminder fields.

Label entities:
- Swipe left delete action: hard delete label.
- Cascade: remove deleted label id from all reminders immediately in the same transaction.
- Swipe right edit action: inline quick edit for label fields.

Deletion result semantics:
- Reminder delete feedback: `Moved to archive`
- Label delete feedback: `Label deleted`

## 8. Data Model And Store Changes

Reminder model:
- Add archive marker:
  - preferred: `archivedAt?: string` (ISO timestamp)
  - alternative: `isArchived: boolean`

Store API additions:
- `archiveReminder(id: string): void`
- `restoreReminder(id: string): void`
- `updateReminderInline(id: string, patch: Partial<ReminderEditableFields>): void`
- `deleteLabel(id: string): void`
- `updateLabelInline(id: string, patch: Partial<LabelEditableFields>): void`

Label mutability rule:
- Default labels and custom labels are both represented in store state so both can be edited/deleted uniformly.

`deleteLabel` requirements:
- Remove label from label collection.
- Remove label id from every reminder.labels array.
- If current `filter.label` equals deleted id, clear it in the same update.
- Emit one feedback event after successful completion.

Archive filtering:
- Board active list excludes archived reminders.
- Standby tray excludes archived reminders.
- Archive surface shows archived reminders and supports restore.

## 9. Shared Component Architecture

Create a reusable interaction layer:
- `useSwipeActionRow` hook
- `SwipeActionRow` wrapper component
- `SwipeActionCoordinator` (context/state) for single-open enforcement

`SwipeActionRow` public contract:
- `rowId`
- `leftAction` config (label, confirmLabel, handler, destructive flag)
- `rightAction` config (label, confirmLabel, handler)
- `renderContent` child
- `gestureProfileResolver`
- `disabled` (true during inline edit/executing)

Per-surface integration:
- Card wraps existing card content in `SwipeActionRow`.
- Tray row wraps each `.tray-item`.
- Label panel wraps each `.label-row-item`.

## 10. Inline Quick Edit Rules

Uniform edit entry:
- `Swipe right -> reveal Edit -> tap once to arm -> tap again to enter edit mode`.

Row-level edit mode behavior:
- Swipe disabled while editing.
- Enter saves by default.
- Escape cancels and restores previous values.
- Clicking another row exits current edit mode (save or cancel policy explicit per surface).

Editable fields:
- Card: title, description, due time (optional if already in quick-edit scope).
- Tray item: title and due time only (compact fields).
- Label row: label name and color.

## 11. Accessibility And Keyboard Parity

Keyboard parity rules:
- Focused row `Delete` key: reveal delete action; second `Enter` confirms.
- Focused row `E`: reveal edit action and allow confirm/edit entry.
- `Escape`: close reveal/confirm/edit mode for focused row.

A11y requirements:
- Revealed actions are real buttons with clear `aria-label`.
- Confirm state text changes to explicit wording: `Tap again to confirm`.
- Focus ring remains visible in revealed and edit states.

## 12. Error Handling

Mutation failure rules:
- Keep row visible.
- Exit `executing` state.
- Return to prior revealed state so user can retry or cancel.
- Show error toast with specific action context.

Atomicity requirements:
- Label hard delete plus reminder-label cascade must be a single atomic state update.
- No partial UI state where label is removed from list but still active in reminders/filter.

## 13. Testing Strategy

Unit tests:
- Gesture threshold resolver by input profile.
- State machine transitions for reveal/confirm/execute/cancel.
- Store mutations:
  - archive/restore reminder
  - deleteLabel cascade + filter cleanup
  - inline update methods

Component tests:
- Swipe left/right reveal behavior on card/tray/label rows via shared test harness.
- Second-tap confirm enforcement.
- Single-open-row behavior.
- Edit mode disables swipe.

Regression tests:
- Reminder archived from tray disappears from tray and board active list.
- Restored reminder reappears in active lists.
- Deleting active filter label clears filter without crash.

## 14. Assumptions

- "Uniform across all" applies to gesture mechanics and confirmation behavior, while domain outcomes differ by entity type (archive vs hard delete).
- Label rows in the panel are treated as swipe-enabled management rows, not just filter shortcuts.
- Archive surface exists (or will be reinstated) so archived reminders are restorable.

## 15. Acceptance Criteria

Feature is accepted when all of the following are true:
- Cards, tray items, and label rows all support left/delete and right/edit with identical reveal and confirm mechanics.
- Reminder delete archives instead of hard deleting, and archived reminders can be restored.
- Label delete performs hard delete and removes label ids from all reminders in one atomic update.
- Edit action opens inline quick-edit on the current surface for each row type.
- Input-aware threshold behavior is in place (lower for touch/gesture profile, higher for mouse drag).
- Automated tests cover shared swipe behavior and store mutation correctness.
