# Settings page: top-tab redesign

## Problem

`SettingsView` today renders all nine subsection components in a single long vertical scroll, grouped by four `<h2 class="settings-group-title">` headings (General, Board, Keyboard, Transcription & AI). Symptoms:

- Too much scrolling to reach any section.
- Everything is visible at once, which is overwhelming.
- The grouping is muddled — "Transcription & AI" is a dumping ground holding five unrelated sections (Audio, Recording, LLM, ModelSetup, Tray).

## Goal

Redesign the settings page as a top-tab layout that mirrors the existing `ArchiveView` pattern: a horizontal tab bar with an animated underline indicator and a single panel visible at a time, transitioning between tabs with an x-slide.

## Information architecture

Five tabs, each rendering a stack of existing `.settings-section` cards. No new subsection components are introduced.

| Tab           | Contents                                                |
| ------------- | ------------------------------------------------------- |
| **General**   | `ProfileSection`, `PreferencesSection`, `TraySection`   |
| **Board**     | `LabelManager`                                          |
| **Voice**     | `AudioSettings`, `RecordingSection`                     |
| **AI**        | `LlmSettings`, `ModelSetup`                             |
| **Shortcuts** | `KeyboardSettings`                                      |

Rationale:

- `TraySection` moves into **General** because tray behavior is a window-level preference, not AI.
- Voice (microphone + Whisper transcription) and AI (LLM + model download) are split into separate tabs, since the two pipelines are configured independently.
- `KeyboardSettings` gets its own tab because it is long (≈12 rows) and conceptually distinct.
- **Board** stays minimal for now (just labels) but leaves room to grow.

## Component design

`SettingsView.tsx` is rewritten to mirror the structure of `ArchiveView.tsx`:

```tsx
type SettingsTab = 'general' | 'board' | 'voice' | 'ai' | 'shortcuts';

const SECTION_TABS: { id: SettingsTab; label: string }[] = [
  { id: 'general',   label: 'General' },
  { id: 'board',     label: 'Board' },
  { id: 'voice',     label: 'Voice' },
  { id: 'ai',        label: 'AI' },
  { id: 'shortcuts', label: 'Shortcuts' },
];
```

- Local state: `const [tab, setTab] = useState<SettingsTab>('general')`.
- Tab bar: `<div className="settings-view__section-tabs" role="tablist" aria-label="Settings sections">` containing one `<button className="settings-section-btn{ is-active}" role="tab" aria-selected>` per entry.
- Active indicator: `motion.div` with `layoutId="settingsSectionIndicator"` and the same spring transition as Archive (`{ type: 'spring', stiffness: 500, damping: 32 }`).
- Panels: `<AnimatePresence mode="wait">` with one `<motion.div key={tab}>` per active tab. Every panel uses the same entrance/exit (`initial={{ opacity: 0, x: -12 }}`, `animate={{ opacity: 1, x: 0 }}`, `exit={{ opacity: 0, x: 12 }}`, `transition={{ duration: 0.18 }}`). Archive tracks direction per tab because it only has two; with five tabs the payoff is not worth the bookkeeping, so all panels slide the same way.
- Each panel just renders the existing subsection components stacked, e.g.:

```tsx
{tab === 'general' && (
  <motion.div key="general" ...>
    <ProfileSection />
    <PreferencesSection />
    <TraySection />
  </motion.div>
)}
```

No props or public API of the existing subsection components change. They continue to render and manage their own state exactly as today.

The four `<h2 class="settings-group-title">` headings are removed; the tab bar replaces them.

## Styling

Add parallel CSS rules to `frontend/src/styles/globals.css` next to the existing `/* ─── Archive section tabs ─── */` block:

- `.settings-view__section-tabs` — same flex row, padding, and border-bottom as `.archive-view__section-tabs`.
- `.settings-section-btn` — same button reset, font, color, hover rule.
- `.settings-section-btn.is-active` — same active text color.
- `.settings-section-btn__indicator` — same absolute-positioned underline bar.

These styles should be visually identical to the Archive tabs so the two views feel cohesive. The existing `.settings-section`, `.settings-row`, `.settings-row__label`, and related classes used by individual subsection components are unchanged.

## State persistence

The active tab is **not persisted** across navigations. Opening Settings always starts on `general`. This matches `ArchiveView`, which always starts on `tasks`.

## Migration and risk

- No backend changes. No HTTP or Tauri IPC changes.
- No changes to any subsection component's file, props, or behavior.
- Existing tests (`frontend/src/store/__tests__/use-store.settings.test.ts`) are unaffected — they exercise the store, not this view.
- Keyboard shortcuts that toggle tabs (`tab_settings` → `Ctrl+5`) continue to open the Settings view; the default first tab will be General.
- Risk: if a user's muscle memory relies on scrolling to a section near the bottom (e.g. "Model Setup"), they now need to click the "AI" tab. Accept this as part of the IA fix.

## Testing

Manual verification after implementation:

- Each tab renders the expected subsection components with no visual regressions inside the cards.
- Clicking each tab animates the underline indicator and slides the panel.
- The tab bar aligns visually with the Archive tab bar when switching between views.
- Keyboard focus on tab buttons advances naturally with `Tab`; `role="tab"` and `aria-selected` are present.
- Existing Vitest suites pass (`pnpm test`).

## Out of scope

- Search across settings.
- Settings deep-links (URL fragments).
- Animating tabs based on direction of travel (left-vs-right slide).
- Any visual changes inside the existing `.settings-section` cards.
- Persisting the last-visited tab.
