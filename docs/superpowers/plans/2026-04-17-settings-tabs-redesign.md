# Settings tabs redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the single-scroll Settings page with a five-tab top-tab layout that mirrors `ArchiveView`.

**Architecture:** `SettingsView.tsx` is rewritten to render a `.settings-view__section-tabs` tab bar (General, Board, Voice, AI, Shortcuts) and an `AnimatePresence`-wrapped panel that stacks the existing subsection components (no changes to any subsection). CSS for the tab bar is added next to the existing Archive tab styles so both views share the same visual language.

**Tech Stack:** React + TypeScript + framer-motion (already installed), Vitest + @testing-library/react for tests, Vite + Tauri.

**Spec:** `docs/superpowers/specs/2026-04-17-settings-tabs-redesign-design.md`

---

## File Structure

- **Modify:** `frontend/src/components/settings/SettingsView.tsx` — full rewrite from flat list to tabbed layout.
- **Modify:** `frontend/src/styles/globals.css` — remove `.settings-group-title` rules (no longer used), add `.settings-view__section-tabs`, `.settings-section-btn`, `.settings-section-btn__indicator`; keep `.settings-view` but adjust padding so the tab bar sits flush to the top like `.archive-view`.
- **Create:** `frontend/src/components/settings/__tests__/SettingsView.test.tsx` — Vitest component test for tab behavior.

No other files are touched. Subsection components (`ProfileSection`, `PreferencesSection`, `TraySection`, `LabelManager`, `AudioSettings`, `RecordingSection`, `LlmSettings`, `ModelSetup`, `KeyboardSettings`) are imported as-is.

---

## Task 1: Add tab-bar CSS

**Files:**
- Modify: `frontend/src/styles/globals.css:2721-2745` (Settings view block) and append new rules after the existing block.

- [ ] **Step 1: Update the `.settings-view` container so it matches `.archive-view` layout**

Replace the existing block at `frontend/src/styles/globals.css:2721-2745`:

```css
/* ─── Settings view ────────────────────────────────────────────────── */

.settings-view {
  display: flex;
  flex-direction: column;
  flex: 1;
  overflow: hidden;
}

.settings-view__panel {
  flex: 1;
  overflow-y: auto;
  padding: 12px 36px 28px;
  display: flex;
  flex-direction: column;
  gap: 18px;
}

.settings-view__section-tabs {
  display: flex;
  gap: 8px;
  padding: 16px 36px 12px;
  background: transparent;
}

.settings-section-btn {
  position: relative;
  padding: 8px 20px;
  border: none;
  border-radius: 0;
  font-size: 0.88rem;
  font-weight: 600;
  font-family: var(--font-sans);
  background: transparent;
  color: var(--color-text-tertiary);
  cursor: pointer;
  transition: color 0.15s ease;
}

.settings-section-btn:hover {
  color: var(--color-text-secondary);
}

.settings-section-btn.is-active {
  color: var(--color-accent-strong);
}

.settings-section-btn__indicator {
  position: absolute;
  bottom: -2px;
  left: 0;
  right: 0;
  height: 2px;
  background: var(--color-accent-strong);
  border-radius: 2px;
}
```

This removes the `.settings-group-title` and `.settings-group-title:first-child` rules (no longer referenced) and switches `.settings-view` to the Archive-style flex column. The new `.settings-view__panel` wraps the active-tab content and owns padding + scroll.

- [ ] **Step 2: Verify no other CSS rule references the removed classes**

Run: `rg "settings-group-title" frontend/src`
Expected: no matches (all references were in the CSS block just removed; the `<h2>` tags using this class are in `SettingsView.tsx` which Task 3 rewrites).

- [ ] **Step 3: Commit**

```bash
git add frontend/src/styles/globals.css
git commit -m "feat(settings): add tab-bar styles mirroring Archive"
```

---

## Task 2: Write failing component test

**Files:**
- Create: `frontend/src/components/settings/__tests__/SettingsView.test.tsx`

The test mocks each subsection component with a stub, then asserts:
1. Default tab is "General" with General's stubs visible.
2. Clicking "AI" switches to the AI panel.
3. `aria-selected` toggles correctly on tab buttons.

Mocking avoids the subsections' `fetch('http://127.0.0.1:3000/settings')` calls during the test.

- [ ] **Step 1: Create the test file**

```tsx
import { act, fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

vi.mock('../ProfileSection', () => ({ ProfileSection: () => <div>stub-profile</div> }));
vi.mock('../PreferencesSection', () => ({ PreferencesSection: () => <div>stub-preferences</div> }));
vi.mock('../TraySection', () => ({ TraySection: () => <div>stub-tray</div> }));
vi.mock('../LabelManager', () => ({ LabelManager: () => <div>stub-labels</div> }));
vi.mock('../AudioSettings', () => ({ AudioSettings: () => <div>stub-audio</div> }));
vi.mock('../RecordingSection', () => ({ RecordingSection: () => <div>stub-recording</div> }));
vi.mock('../LlmSettings', () => ({ LlmSettings: () => <div>stub-llm</div> }));
vi.mock('../ModelSetup', () => ({ ModelSetup: () => <div>stub-model-setup</div> }));
vi.mock('../KeyboardSettings', () => ({ KeyboardSettings: () => <div>stub-keyboard</div> }));

import { SettingsView } from '../SettingsView';

describe('SettingsView', () => {
  it('defaults to the General tab and renders its subsections', () => {
    render(<SettingsView />);
    expect(screen.getByRole('tab', { name: 'General' })).toHaveAttribute('aria-selected', 'true');
    expect(screen.getByText('stub-profile')).toBeInTheDocument();
    expect(screen.getByText('stub-preferences')).toBeInTheDocument();
    expect(screen.getByText('stub-tray')).toBeInTheDocument();
    expect(screen.queryByText('stub-llm')).not.toBeInTheDocument();
  });

  it('switches panels when a different tab is clicked', async () => {
    render(<SettingsView />);
    await act(async () => {
      fireEvent.click(screen.getByRole('tab', { name: 'AI' }));
    });
    expect(screen.getByRole('tab', { name: 'AI' })).toHaveAttribute('aria-selected', 'true');
    expect(screen.getByRole('tab', { name: 'General' })).toHaveAttribute('aria-selected', 'false');
    expect(await screen.findByText('stub-llm')).toBeInTheDocument();
    expect(await screen.findByText('stub-model-setup')).toBeInTheDocument();
    expect(screen.queryByText('stub-profile')).not.toBeInTheDocument();
  });

  it('renders all five tabs in order', () => {
    render(<SettingsView />);
    const labels = screen.getAllByRole('tab').map((el) => el.textContent);
    expect(labels).toEqual(['General', 'Board', 'Voice', 'AI', 'Shortcuts']);
  });
});
```

- [ ] **Step 2: Run the test and verify it fails**

Run: `pnpm test --run SettingsView`
Expected: FAIL — the current `SettingsView` has no tabs, no `role="tab"` elements, and renders every subsection simultaneously, so `getByRole('tab', …)` throws.

- [ ] **Step 3: Commit the failing test**

```bash
git add frontend/src/components/settings/__tests__/SettingsView.test.tsx
git commit -m "test(settings): add failing SettingsView tab tests"
```

---

## Task 3: Rewrite `SettingsView` to tabbed layout

**Files:**
- Modify: `frontend/src/components/settings/SettingsView.tsx` (full rewrite)

- [ ] **Step 1: Replace the file contents**

Overwrite `frontend/src/components/settings/SettingsView.tsx` with:

```tsx
import { useState } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { ProfileSection } from './ProfileSection';
import { PreferencesSection } from './PreferencesSection';
import { TraySection } from './TraySection';
import { LabelManager } from './LabelManager';
import { AudioSettings } from './AudioSettings';
import { RecordingSection } from './RecordingSection';
import { LlmSettings } from './LlmSettings';
import { ModelSetup } from './ModelSetup';
import { KeyboardSettings } from './KeyboardSettings';

type SettingsTab = 'general' | 'board' | 'voice' | 'ai' | 'shortcuts';

const SECTION_TABS: { id: SettingsTab; label: string }[] = [
  { id: 'general', label: 'General' },
  { id: 'board', label: 'Board' },
  { id: 'voice', label: 'Voice' },
  { id: 'ai', label: 'AI' },
  { id: 'shortcuts', label: 'Shortcuts' },
];

const panelMotion = {
  initial: { opacity: 0, x: -12 },
  animate: { opacity: 1, x: 0 },
  exit: { opacity: 0, x: 12 },
  transition: { duration: 0.18 },
};

export function SettingsView() {
  const [tab, setTab] = useState<SettingsTab>('general');

  return (
    <div className="settings-view">
      <div className="settings-view__section-tabs" role="tablist" aria-label="Settings sections">
        {SECTION_TABS.map(({ id, label }) => {
          const isActive = tab === id;
          return (
            <button
              key={id}
              type="button"
              role="tab"
              aria-selected={isActive}
              className={`settings-section-btn${isActive ? ' is-active' : ''}`}
              onClick={() => setTab(id)}
            >
              {label}
              {isActive && (
                <motion.div
                  layoutId="settingsSectionIndicator"
                  className="settings-section-btn__indicator"
                  initial={false}
                  transition={{ type: 'spring', stiffness: 500, damping: 32 }}
                />
              )}
            </button>
          );
        })}
      </div>

      <AnimatePresence mode="wait">
        {tab === 'general' && (
          <motion.div key="general" className="settings-view__panel" {...panelMotion}>
            <ProfileSection />
            <PreferencesSection />
            <TraySection />
          </motion.div>
        )}
        {tab === 'board' && (
          <motion.div key="board" className="settings-view__panel" {...panelMotion}>
            <LabelManager />
          </motion.div>
        )}
        {tab === 'voice' && (
          <motion.div key="voice" className="settings-view__panel" {...panelMotion}>
            <AudioSettings />
            <RecordingSection />
          </motion.div>
        )}
        {tab === 'ai' && (
          <motion.div key="ai" className="settings-view__panel" {...panelMotion}>
            <LlmSettings />
            <ModelSetup />
          </motion.div>
        )}
        {tab === 'shortcuts' && (
          <motion.div key="shortcuts" className="settings-view__panel" {...panelMotion}>
            <KeyboardSettings />
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
```

- [ ] **Step 2: Run the tests and verify they pass**

Run: `pnpm test --run SettingsView`
Expected: PASS (all three tests).

- [ ] **Step 3: Run the full test suite**

Run: `pnpm test --run`
Expected: PASS (in particular `use-store.settings.test.ts`, `use-store.api.test.ts`, `use-store.swipe.test.ts`, `SwipeActionRow.test.tsx` should be unaffected).

- [ ] **Step 4: Run the TypeScript check**

Run: `pnpm exec tsc --noEmit` (or whatever `pnpm run` entry this repo uses — inspect `package.json` if unclear).
Expected: no new type errors from `SettingsView.tsx`.

- [ ] **Step 5: Commit**

```bash
git add frontend/src/components/settings/SettingsView.tsx
git commit -m "feat(settings): rewrite SettingsView as tabbed layout"
```

---

## Task 4: Manual smoke test

**Files:** none changed in this task; this is a verification gate.

- [ ] **Step 1: Start the dev app**

Run the existing Tauri dev command (check `package.json` scripts — likely `pnpm tauri dev` or `pnpm dev` depending on the repo's convention).

- [ ] **Step 2: Walk through each tab**

In the running app, open the Settings view and verify for each of the five tabs (General, Board, Voice, AI, Shortcuts):

1. The tab label appears in the tab bar.
2. Clicking the tab animates the blue underline indicator over to it.
3. The previous panel slides out and the new panel slides in (x-axis, ~180 ms).
4. Every subsection card inside the panel renders with its existing visual styling (no broken layout, no stray `settings-group-title` heading left over).
5. Interactive controls inside each subsection still work:
   - **General** — Profile fields save, Preferences toggles persist, Tray toggle persists.
   - **Board** — Label Manager can add/rename/delete labels.
   - **Voice** — Microphone dropdown populates; Recording model selector still works.
   - **AI** — Language Model radio switches between Disabled/Local/Remote; Model Setup still lists models.
   - **Shortcuts** — Every shortcut row is editable; "Reset to defaults" still works.

- [ ] **Step 3: Confirm the Settings tab bar visually matches the Archive tab bar**

Open both views back-to-back. The tab bar height, padding, font, hover color, and underline indicator should be indistinguishable.

- [ ] **Step 4: Report completion**

If everything in Steps 2 and 3 passed, the feature is ready. If any subsection interaction regressed, file a fix as a follow-up task — do **not** patch the subsection component here; the scope of this plan is the view shell only.

---

## Self-review

**Spec coverage:**
- Five-tab IA with the exact mapping → Task 3 Step 1.
- `AnimatePresence` + x-slide → Task 3 Step 1 (`panelMotion`).
- `layoutId="settingsSectionIndicator"` indicator → Task 3 Step 1.
- Tab bar CSS parity with Archive → Task 1 Step 1.
- Removal of `.settings-group-title` headings → Task 1 Step 1 (CSS) + Task 3 Step 1 (JSX).
- No backend or subsection-component changes → confirmed by File Structure section.
- No tab-state persistence → Task 3 Step 1 (`useState<SettingsTab>('general')`, no storage).
- Manual verification → Task 4.

**Placeholder scan:** no `TBD`, `TODO`, or hand-wave phrases found.

**Type consistency:** `SettingsTab` union + `SECTION_TABS` entries match the five `tab === …` branches. Test mocks correspond 1:1 to the imports.
