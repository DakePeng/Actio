# Always-On Listening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Surface the existing `audio.always_listening` backend setting as a tray mic button + Live-tab toggle + `Ctrl+Shift+M` global hotkey. Rename the Recording tab to Live and drop its manual record/stop UI. Make the tray wordmark a visual proxy for the toggle.

**Architecture:** UI plumbing on top of an already-shipped backend flag. The store gets a derived `ui.listeningEnabled` shadow plus a `setListening` action that calls the existing `patchSettings` helper. A new shared `<ListeningToggle>` component is consumed by both the tray and the renamed `LiveTab`. `useActioState` learns one new branch (`listeningEnabled ? 'listening' : 'standby'`). A new global shortcut hits the same store action. A small backend migration copies a stale `tab_recording` shortcut binding to `tab_live` once.

**Tech Stack:** React 19, TypeScript, Zustand, Tauri v2, Vite + Vitest (frontend); Rust 2021 + SQLx + axum (backend).

**Spec:** `docs/superpowers/specs/2026-04-25-always-on-listening-design.md` (commit `20a2add`).

---

## File Structure

**New:**
- `frontend/src/components/ListeningToggle.tsx` — shared mic button (tray + Live tab).
- `frontend/src/components/__tests__/ListeningToggle.test.tsx`.
- `frontend/src/components/LiveTab.tsx` — replacement for `RecordingTab.tsx` (file rename + rewrite).
- `frontend/src/components/__tests__/LiveTab.test.tsx`.
- `frontend/src/api/settings-client.ts` — thin wrapper around `PATCH /settings` so `setListening` doesn't duplicate fetch boilerplate.

**Modified:**
- `frontend/src/types/index.ts` — `Tab` literal + `UIState` extension (`listeningEnabled`, `listeningStartedAt`).
- `frontend/src/store/use-store.ts` — initial UI state + `setListening` + boot fetch of `always_listening`.
- `frontend/src/hooks/useActioState.ts` — splice `listeningEnabled` into priority chain.
- `frontend/src/hooks/useGlobalShortcuts.ts` — `toggle_listening` default + handler branch.
- `frontend/src/components/StandbyTray.tsx` — render `<ListeningToggle>` next to chevron.
- `frontend/src/components/TabBar.tsx` — `tab.recording` → `tab.live`.
- `frontend/src/components/BoardWindow.tsx` — switch arm renamed to `'live'`.
- `frontend/src/components/settings/KeyboardSettings.tsx` — `toggle_listening` action + tab rename.
- `frontend/src/i18n/locales/en.ts`, `frontend/src/i18n/locales/zh-CN.ts` — new keys + `tab.recording` rename.
- `frontend/src/styles/globals.css` — mic-button styles, `recording-tab__*` → `live-tab__*` rename, `live-tab__header` styles.
- `frontend/src/hooks/__tests__/useActioState.test.tsx` — new branch tests.
- `frontend/src/components/__tests__/StandbyTray.test.tsx` — mic-button assertions.
- `backend/actio-core/src/engine/app_settings.rs` — one-shot `tab_recording → tab_live` shortcut migration in `SettingsManager::new`.

**Deleted:**
- `frontend/src/components/RecordingTab.tsx` (renamed to LiveTab).

---

## Task Ordering Rationale

Bottom-up: types → store → hook → presentational component → integration. The backend migration lands last so the rename test in step 8 doesn't accidentally pass for the wrong reason (we want to see the rename actually flip behavior in the frontend first).

Each task ends with a green test run + commit. No batched commits.

---

### Task 1: Type + UIState extension

**Files:**
- Modify: `frontend/src/types/index.ts`

- [ ] **Step 1: Update `Tab` literal**

In `frontend/src/types/index.ts:6`, change:

```ts
export type Tab = 'board' | 'needs-review' | 'archive' | 'settings' | 'recording' | 'people';
```

to:

```ts
export type Tab = 'board' | 'needs-review' | 'archive' | 'settings' | 'live' | 'people';
```

- [ ] **Step 2: Extend `UIState`**

In `frontend/src/types/index.ts:157-177`, add the new fields **at the bottom of the interface, before the closing brace**:

```ts
  /**
   * User-facing toggle for the always-on background pipeline. Mirrors
   * `settings.audio.always_listening`; the canonical source is the backend.
   * `null` while the boot fetch hasn't resolved yet — UI shows a neutral
   * disabled state in that window.
   */
  listeningEnabled: boolean | null;
  /**
   * Wall-clock timestamp (Date.now()) of the most recent off → on flip,
   * or null when listening is off. Drives the "Listening since" header
   * timer in the Live tab. Not persisted across restarts.
   */
  listeningStartedAt: number | null;
```

- [ ] **Step 3: Run typecheck to verify nothing else broke**

Run: `cd frontend && pnpm tsc --noEmit`
Expected output: many errors referencing `'recording'` no longer matching `Tab` (these are real call sites we'll fix in later tasks). The store's `initialUI` block will also flag missing required props. **Note them — do not fix them yet.**

- [ ] **Step 4: Commit**

```bash
git add frontend/src/types/index.ts
git commit -m "types: extend Tab literal and UIState for listening toggle"
```

---

### Task 2: Store wiring (initial state + setListening action)

**Files:**
- Create: `frontend/src/api/settings-client.ts`
- Modify: `frontend/src/store/use-store.ts`
- Test: `frontend/src/store/__tests__/use-store.listening.test.ts` (new)

- [ ] **Step 1: Write the failing test**

Create `frontend/src/store/__tests__/use-store.listening.test.ts`:

```ts
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useStore } from '../use-store';

describe('useStore — listening toggle', () => {
  beforeEach(() => {
    vi.useFakeTimers().setSystemTime(new Date('2026-04-25T12:00:00Z'));
    useStore.setState((state) => ({
      ui: {
        ...state.ui,
        listeningEnabled: null,
        listeningStartedAt: null,
      },
    }));
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true, json: () => Promise.resolve({}) }));
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it('setListening(true) updates the store and stamps listeningStartedAt', async () => {
    await useStore.getState().setListening(true);
    const { ui } = useStore.getState();
    expect(ui.listeningEnabled).toBe(true);
    expect(ui.listeningStartedAt).toBe(Date.parse('2026-04-25T12:00:00Z'));
  });

  it('setListening(false) clears listeningStartedAt', async () => {
    await useStore.getState().setListening(true);
    await useStore.getState().setListening(false);
    const { ui } = useStore.getState();
    expect(ui.listeningEnabled).toBe(false);
    expect(ui.listeningStartedAt).toBeNull();
  });

  it('setListening reverts state and pushes failure feedback when PATCH fails', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: false, status: 500 }));
    useStore.setState((state) => ({
      ui: { ...state.ui, listeningEnabled: true, listeningStartedAt: 1 },
    }));

    await useStore.getState().setListening(false);

    const { ui } = useStore.getState();
    expect(ui.listeningEnabled).toBe(true);
    expect(ui.listeningStartedAt).toBe(1);
    expect(ui.feedback?.message).toBe('feedback.listeningToggleFailed');
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd frontend && pnpm exec vitest run src/store/__tests__/use-store.listening.test.ts`
Expected: FAIL — `setListening` is not defined on the store.

- [ ] **Step 3: Create the settings client wrapper**

Create `frontend/src/api/settings-client.ts`:

```ts
const API_BASE = 'http://127.0.0.1:3000';

export async function patchSettings(patch: Record<string, unknown>): Promise<void> {
  const res = await fetch(`${API_BASE}/settings`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(patch),
  });
  if (!res.ok) throw new Error(`PATCH /settings failed: ${res.status}`);
}

export async function fetchSettings(): Promise<{ audio?: { always_listening?: boolean } }> {
  const res = await fetch(`${API_BASE}/settings`);
  if (!res.ok) throw new Error(`GET /settings failed: ${res.status}`);
  return res.json();
}
```

- [ ] **Step 4: Wire `listeningEnabled` + `listeningStartedAt` into `initialUI`**

In `frontend/src/store/use-store.ts:84-97`, change `initialUI` to include the new fields:

```ts
const initialUI: UIState = {
  showBoardWindow: false,
  trayExpanded: false,
  expandedCardId: null,
  highlightedCardId: null,
  showNewReminderBar: false,
  hasSeenOnboarding: localStorage.getItem('actio-onboarded') === 'true',
  activeTab: 'board',
  focusedCardIndex: null,
  isDictating: false,
  isDictationTranscribing: false,
  dictationTranscript: '',
  feedback: null,
  listeningEnabled: null,
  listeningStartedAt: null,
};
```

- [ ] **Step 5: Add `setListening` to the `AppState` interface**

In `frontend/src/store/use-store.ts:17-61`, add this method declaration after `setDictationTranscript`:

```ts
  setListening: (enabled: boolean) => Promise<void>;
  loadListening: () => Promise<void>;
```

- [ ] **Step 6: Add the import for the settings client**

At the top of `frontend/src/store/use-store.ts:1-15`, add:

```ts
import { fetchSettings, patchSettings } from '../api/settings-client';
```

- [ ] **Step 7: Implement the action**

Inside the `create<AppState>(...)` body in `frontend/src/store/use-store.ts`, add these two methods near the other UI setters (after `setDictationTranscript`):

```ts
  setListening: async (enabled) => {
    const prev = {
      enabled: useStore.getState().ui.listeningEnabled,
      startedAt: useStore.getState().ui.listeningStartedAt,
    };
    set((state) => ({
      ui: {
        ...state.ui,
        listeningEnabled: enabled,
        listeningStartedAt: enabled ? Date.now() : null,
      },
    }));
    try {
      await patchSettings({ audio: { always_listening: enabled } });
    } catch {
      set((state) => ({
        ui: {
          ...state.ui,
          listeningEnabled: prev.enabled,
          listeningStartedAt: prev.startedAt,
        },
      }));
      pushFeedback(set, 'feedback.listeningToggleFailed');
    }
  },

  loadListening: async () => {
    try {
      const settings = await fetchSettings();
      const v = settings.audio?.always_listening;
      if (typeof v !== 'boolean') return;
      set((state) => ({
        ui: {
          ...state.ui,
          listeningEnabled: v,
          listeningStartedAt: v ? Date.now() : null,
        },
      }));
    } catch {
      // Silent — boot UI tolerates the null sentinel until a later attempt
      // succeeds (e.g. when the user toggles, which performs its own PATCH).
    }
  },
```

- [ ] **Step 8: Run test to verify it passes**

Run: `cd frontend && pnpm exec vitest run src/store/__tests__/use-store.listening.test.ts`
Expected: PASS — all 3 cases green.

- [ ] **Step 9: Run the full frontend test suite to confirm nothing else regressed**

Run: `cd frontend && pnpm test`
Expected: PASS overall. The Tab-rename consumers (`TabBar`, `BoardWindow`, etc.) compile because Vitest evaluates source on demand and they still pass strings; tsc errors only surface when we run `pnpm tsc --noEmit`. We'll fix those in Task 6.

- [ ] **Step 10: Commit**

```bash
git add frontend/src/api/settings-client.ts frontend/src/store/use-store.ts frontend/src/store/__tests__/use-store.listening.test.ts
git commit -m "store: add setListening/loadListening with optimistic update + revert"
```

---

### Task 3: useActioState — splice `listeningEnabled` into the priority chain

**Files:**
- Modify: `frontend/src/hooks/useActioState.ts`
- Test: `frontend/src/hooks/__tests__/useActioState.test.tsx`

- [ ] **Step 1: Write the failing tests**

Append to `frontend/src/hooks/__tests__/useActioState.test.tsx` (just before the final closing `});`):

```tsx
  it('returns "listening" when the toggle is enabled and nothing transient applies', () => {
    useStore.setState((state) => ({
      ui: { ...state.ui, listeningEnabled: true },
    }));

    render(<Probe />);

    expect(screen.getByTestId('state')).toHaveTextContent('listening');
  });

  it('returns "standby" when the toggle is disabled', () => {
    useStore.setState((state) => ({
      ui: { ...state.ui, listeningEnabled: false },
    }));

    render(<Probe />);

    expect(screen.getByTestId('state')).toHaveTextContent('standby');
  });

  it('dictation phases still beat the listening toggle', () => {
    useStore.setState((state) => ({
      ui: {
        ...state.ui,
        listeningEnabled: true,
        isDictating: true,
        isDictationTranscribing: false,
      },
    }));

    render(<Probe />);

    expect(screen.getByTestId('state')).toHaveTextContent('transcribing');
  });
```

Also extend the `beforeEach` block (around line 14-29) to reset `listeningEnabled`:

```tsx
    useStore.setState((state) => ({
      reminders: [],
      ui: {
        ...state.ui,
        isDictating: false,
        isDictationTranscribing: false,
        dictationTranscript: '',
        feedback: null,
        listeningEnabled: null,
        listeningStartedAt: null,
      },
    }));
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd frontend && pnpm exec vitest run src/hooks/__tests__/useActioState.test.tsx`
Expected: FAIL — the new "listening when toggle on" test currently returns `'standby'` because `useActioState` doesn't read `listeningEnabled`.

- [ ] **Step 3: Splice the new branch into `useActioState`**

In `frontend/src/hooks/useActioState.ts`, modify the body so the priority chain reads:

```ts
  // ... preserve existing imports + selectors ...
  const listeningEnabled = useStore((s) => s.ui.listeningEnabled);

  // ... keep the existing useEffect for transient feedback ...

  if (preview) return preview;
  if (flash) return flash;
  if (transient) return transient;
  if (isDictationTranscribing) return 'processing';
  if (isExtracting) return 'processing';
  if (isDictating) return 'transcribing';
  if (listeningEnabled) return 'listening';
  if (isRecording) return 'listening';
  return 'standby';
```

Concretely: add the `listeningEnabled` selector after the existing `useStore`/`useVoiceStore` reads, then insert `if (listeningEnabled) return 'listening';` immediately above the existing `if (isRecording) return 'listening';` line.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd frontend && pnpm exec vitest run src/hooks/__tests__/useActioState.test.tsx`
Expected: PASS — all cases green (existing 5 + 3 new).

- [ ] **Step 5: Commit**

```bash
git add frontend/src/hooks/useActioState.ts frontend/src/hooks/__tests__/useActioState.test.tsx
git commit -m "hook: useActioState reflects listeningEnabled toggle"
```

---

### Task 4: i18n keys — new strings + `tab.recording` rename

**Files:**
- Modify: `frontend/src/i18n/locales/en.ts`
- Modify: `frontend/src/i18n/locales/zh-CN.ts`

The `parity.test.ts` already enforces key parity, so adding to `en.ts` without `zh-CN.ts` will fail loudly. Doing both at once.

- [ ] **Step 1: Rename `tab.recording` → `tab.live` in both locales**

In `frontend/src/i18n/locales/en.ts:179`, change:

```ts
  'tab.recording': 'Transcribe',
```

to:

```ts
  'tab.live': 'Live',
```

In `frontend/src/i18n/locales/zh-CN.ts:177`, change:

```ts
  'tab.recording': '转写',
```

to:

```ts
  'tab.live': '实时',
```

- [ ] **Step 2: Add new tray + Live-tab + feedback keys to en.ts**

Append the following keys to `frontend/src/i18n/locales/en.ts`. Add them in two clusters: tray strings near the existing `tray.aria.*` block (around line 154), and Live-tab + feedback near the existing `feedback.*` block. Do not invent a new section header.

Tray block (insert near `tray.aria.openBoard`):

```ts
  'tray.aria.toggleListening.on': 'Listening — click to mute',
  'tray.aria.toggleListening.off': 'Muted — click to start listening',
  'tray.tooltip.listening': 'Listening',
  'tray.tooltip.muted': 'Muted',
```

Live-tab block (group near other tab-related strings, around line 200):

```ts
  'live.header.on': 'Listening',
  'live.header.off': 'Muted',
  'live.listeningSince': 'Listening since {time} • {duration}',
  'live.pausedHint': 'Listening is paused. Turn it on in the tray or here to start capturing.',
  'live.aria.toggleListening': 'Toggle listening',
```

Feedback block (next to other `feedback.*` lines):

```ts
  'feedback.listeningOn': 'Listening on',
  'feedback.listeningOff': 'Listening off',
  'feedback.listeningToggleFailed': "Couldn't change listening state",
```

Settings shortcut action label:

```ts
  'settings.shortcuts.action.toggle_listening': 'Toggle listening',
  'settings.shortcuts.action.tab_live': 'Live tab',
```

Remove the now-stale entry `'settings.shortcuts.action.tab_recording'` if present.

- [ ] **Step 3: Add the same keys to zh-CN.ts with translations**

Append to `frontend/src/i18n/locales/zh-CN.ts` in matching positions:

```ts
  'tray.aria.toggleListening.on': '正在聆听 — 点击静音',
  'tray.aria.toggleListening.off': '已静音 — 点击开始聆听',
  'tray.tooltip.listening': '聆听中',
  'tray.tooltip.muted': '已静音',
  'live.header.on': '聆听中',
  'live.header.off': '已静音',
  'live.listeningSince': '从 {time} 开始 • 已运行 {duration}',
  'live.pausedHint': '聆听已暂停。在托盘或此处开启以开始捕捉。',
  'live.aria.toggleListening': '切换聆听',
  'feedback.listeningOn': '已开启聆听',
  'feedback.listeningOff': '已关闭聆听',
  'feedback.listeningToggleFailed': '无法切换聆听状态',
  'settings.shortcuts.action.toggle_listening': '切换聆听',
  'settings.shortcuts.action.tab_live': '实时标签页',
```

Remove `'settings.shortcuts.action.tab_recording'` if present.

- [ ] **Step 4: Run the parity test**

Run: `cd frontend && pnpm exec vitest run src/i18n/__tests__/parity.test.ts`
Expected: PASS — both locales have identical key sets.

- [ ] **Step 5: Commit**

```bash
git add frontend/src/i18n/locales/en.ts frontend/src/i18n/locales/zh-CN.ts
git commit -m "i18n: rename tab.recording → tab.live, add listening toggle keys"
```

---

### Task 5: ListeningToggle component

**Files:**
- Create: `frontend/src/components/ListeningToggle.tsx`
- Test: `frontend/src/components/__tests__/ListeningToggle.test.tsx`

- [ ] **Step 1: Write the failing test**

Create `frontend/src/components/__tests__/ListeningToggle.test.tsx`:

```tsx
import { fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { ListeningToggle } from '../ListeningToggle';
import { LanguageProvider } from '../../i18n';
import { useStore } from '../../store/use-store';

function renderToggle() {
  return render(
    <LanguageProvider>
      <ListeningToggle />
    </LanguageProvider>,
  );
}

describe('ListeningToggle', () => {
  beforeEach(() => {
    Object.defineProperty(navigator, 'language', { value: 'en-US', configurable: true });
    useStore.setState((state) => ({
      ui: { ...state.ui, listeningEnabled: true, listeningStartedAt: 1 },
    }));
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true, json: () => Promise.resolve({}) }));
  });

  it('renders the on-state aria label when listening', () => {
    renderToggle();
    expect(screen.getByRole('button', { name: /click to mute/i })).toHaveAttribute(
      'aria-pressed',
      'true',
    );
  });

  it('renders the off-state aria label when muted', () => {
    useStore.setState((state) => ({
      ui: { ...state.ui, listeningEnabled: false, listeningStartedAt: null },
    }));
    renderToggle();
    expect(screen.getByRole('button', { name: /click to start listening/i })).toHaveAttribute(
      'aria-pressed',
      'false',
    );
  });

  it('disables itself while the toggle is null (boot)', () => {
    useStore.setState((state) => ({
      ui: { ...state.ui, listeningEnabled: null, listeningStartedAt: null },
    }));
    renderToggle();
    expect(screen.getByRole('button')).toBeDisabled();
  });

  it('clicking calls setListening with the inverted value', () => {
    const spy = vi.spyOn(useStore.getState(), 'setListening').mockResolvedValue();
    renderToggle();
    fireEvent.click(screen.getByRole('button'));
    expect(spy).toHaveBeenCalledWith(false);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd frontend && pnpm exec vitest run src/components/__tests__/ListeningToggle.test.tsx`
Expected: FAIL — `ListeningToggle` module does not exist.

- [ ] **Step 3: Implement the component**

Create `frontend/src/components/ListeningToggle.tsx`:

```tsx
import { memo } from 'react';
import { useStore } from '../store/use-store';
import { useT } from '../i18n';

export interface ListeningToggleProps {
  /** Optional className applied alongside the base class. */
  className?: string;
  /** Render at this pixel size (square). Defaults to 28. */
  size?: number;
}

export const ListeningToggle = memo(function ListeningToggle({
  className,
  size = 28,
}: ListeningToggleProps) {
  const enabled = useStore((s) => s.ui.listeningEnabled);
  const setListening = useStore((s) => s.setListening);
  const t = useT();

  const isOn = enabled === true;
  const ariaLabel = enabled === null
    ? t('tray.tooltip.listening') // neutral while booting
    : isOn
      ? t('tray.aria.toggleListening.on')
      : t('tray.aria.toggleListening.off');
  const tooltip = isOn ? t('tray.tooltip.listening') : t('tray.tooltip.muted');

  return (
    <button
      type="button"
      className={`listening-toggle${className ? ` ${className}` : ''}`}
      style={{ width: size, height: size }}
      aria-pressed={enabled === null ? undefined : isOn}
      aria-label={ariaLabel}
      title={tooltip}
      disabled={enabled === null}
      onClick={() => {
        if (enabled === null) return;
        void setListening(!isOn);
      }}
    >
      <svg
        viewBox="0 0 24 24"
        fill={isOn ? 'currentColor' : 'none'}
        stroke="currentColor"
        strokeWidth="1.8"
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden="true"
      >
        {/* Mic capsule */}
        <rect x="9" y="3" width="6" height="12" rx="3" />
        <path d="M5 11a7 7 0 0 0 14 0" fill="none" />
        <line x1="12" y1="18" x2="12" y2="22" fill="none" />
        {!isOn && enabled !== null && (
          <line
            x1="4"
            y1="4"
            x2="20"
            y2="20"
            stroke="currentColor"
            strokeWidth="1.8"
            strokeLinecap="round"
          />
        )}
      </svg>
    </button>
  );
});
```

- [ ] **Step 4: Add the matching CSS class**

In `frontend/src/styles/globals.css`, append (placement: near the existing `.tray-chevron-button` rule):

```css
.listening-toggle {
  appearance: none;
  background: transparent;
  border: 0;
  border-radius: 6px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  color: var(--color-text-muted);
  cursor: pointer;
  padding: 0;
  transition: background-color 0.16s ease, color 0.16s ease;
}
.listening-toggle:hover:not(:disabled) {
  background: var(--color-surface-hover);
  color: var(--color-text);
}
.listening-toggle:disabled {
  opacity: 0.45;
  cursor: default;
}
.listening-toggle[aria-pressed="true"] {
  color: var(--color-text);
}
.listening-toggle svg {
  width: 18px;
  height: 18px;
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cd frontend && pnpm exec vitest run src/components/__tests__/ListeningToggle.test.tsx`
Expected: PASS — all 4 cases green.

- [ ] **Step 6: Commit**

```bash
git add frontend/src/components/ListeningToggle.tsx frontend/src/components/__tests__/ListeningToggle.test.tsx frontend/src/styles/globals.css
git commit -m "feat: ListeningToggle shared mic-button component"
```

---

### Task 6: TabBar + BoardWindow + KeyboardSettings rename

**Files:**
- Modify: `frontend/src/components/TabBar.tsx`
- Modify: `frontend/src/components/BoardWindow.tsx`
- Modify: `frontend/src/components/settings/KeyboardSettings.tsx`

- [ ] **Step 1: Update `TabBar` to use the new id and label key**

In `frontend/src/components/TabBar.tsx:6-13`, change the `'recording'` entry to:

```ts
  { id: 'live', labelKey: 'tab.live' },
```

- [ ] **Step 2: Update `BoardWindow` switch arm**

In `frontend/src/components/BoardWindow.tsx:218`, change:

```tsx
                {activeTab === 'recording' && <RecordingTab />}
```

to:

```tsx
                {activeTab === 'live' && <LiveTab />}
```

And replace the `import { RecordingTab }` line near the top with:

```tsx
import { LiveTab } from './LiveTab';
```

(`LiveTab` is created in Task 8, so the build will be temporarily red after this step. We'll add a placeholder in step 4 of this task to keep tsc happy until Task 8 lands.)

- [ ] **Step 3: Update `KeyboardSettings` action map and shortcut group**

In `frontend/src/components/settings/KeyboardSettings.tsx`:

(a) Line 17, change `tab_recording: 'Ctrl+3'` to:

```ts
  tab_live: 'Ctrl+3',
```

(b) Line 33, change:

```ts
  tab_recording: 'settings.shortcuts.action.tab_recording',
```

to:

```ts
  tab_live: 'settings.shortcuts.action.tab_live',
```

(c) Line 146, change:

```ts
      actions: ['tab_board', 'tab_people', 'tab_recording', 'tab_archive', 'tab_settings'],
```

to:

```ts
      actions: ['tab_board', 'tab_people', 'tab_live', 'tab_archive', 'tab_settings'],
```

- [ ] **Step 4: Stub LiveTab so the build compiles**

Create a temporary `frontend/src/components/LiveTab.tsx` that matches RecordingTab's signature so tsc + tests pass during this intermediate state:

```tsx
import { RecordingTab } from './RecordingTab';

/** TEMP placeholder — replaced in Task 8 with the new Live-tab UI. */
export function LiveTab() {
  return <RecordingTab />;
}
```

- [ ] **Step 5: Run typecheck + tests**

Run: `cd frontend && pnpm tsc --noEmit && pnpm test`
Expected: PASS — all suites green. Any remaining errors trace to call sites that reference `'recording'` as a `Tab` literal in tests (e.g. `__tests__/NewReminderBar.test.tsx`). Fix them by replacing `activeTab: 'recording'` with `activeTab: 'live'` wherever it appears in tests; run `git grep -n "'recording'" frontend/src/` to find them, and update each.

- [ ] **Step 6: Commit**

```bash
git add frontend/src/components/TabBar.tsx frontend/src/components/BoardWindow.tsx frontend/src/components/settings/KeyboardSettings.tsx frontend/src/components/LiveTab.tsx frontend/src/components/**/__tests__/*.tsx
git commit -m "refactor: rename Recording tab to Live (id + label + shortcut)"
```

---

### Task 7: StandbyTray integration

**Files:**
- Modify: `frontend/src/components/StandbyTray.tsx`
- Modify: `frontend/src/styles/globals.css`
- Modify: `frontend/src/components/__tests__/StandbyTray.test.tsx`

- [ ] **Step 1: Write the failing test**

In `frontend/src/components/__tests__/StandbyTray.test.tsx`, append a new case (before the final `});`):

```tsx
  it('renders the listening toggle in the collapsed tray', () => {
    useStore.setState((state) => ({
      ui: { ...state.ui, listeningEnabled: true, listeningStartedAt: 1 },
    }));

    render(
      <LanguageProvider>
        <StandbyTray />
      </LanguageProvider>,
    );

    const toggle = screen.getByRole('button', { name: /click to mute/i });
    expect(toggle).toBeInTheDocument();
    expect(toggle).toHaveAttribute('aria-pressed', 'true');
  });
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd frontend && pnpm exec vitest run src/components/__tests__/StandbyTray.test.tsx`
Expected: FAIL — no element matches the toggle aria label.

- [ ] **Step 3: Render `<ListeningToggle>` in the tray**

In `frontend/src/components/StandbyTray.tsx:189-212`, the chevron button block currently looks like:

```tsx
          <button
            type="button"
            className="tray-chevron-button"
            ...
```

Add the import at the top:

```tsx
import { ListeningToggle } from './ListeningToggle';
```

And insert `<ListeningToggle />` immediately before the chevron button:

```tsx
          <ListeningToggle className="tray-mic-button" />
          <button
            type="button"
            className="tray-chevron-button"
            ...
```

- [ ] **Step 4: Add the placement-specific CSS**

In `frontend/src/styles/globals.css`, append next to `.tray-chevron-button`:

```css
.tray-mic-button {
  margin-right: 4px;
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cd frontend && pnpm exec vitest run src/components/__tests__/StandbyTray.test.tsx`
Expected: PASS — both old and new cases green.

- [ ] **Step 6: Boot-fetch — call `loadListening` on app mount**

In `frontend/src/App.tsx`, find the existing `useEffect` that calls `loadBoard()` (or equivalent boot fetch) and add a sibling call:

```tsx
useEffect(() => {
  void useStore.getState().loadBoard();
  void useStore.getState().loadListening();
}, []);
```

If the existing block already wraps `loadBoard`, just append `loadListening`. If `App.tsx` does not already have such a hook, add one inside the component body.

- [ ] **Step 7: Run the full frontend test suite**

Run: `cd frontend && pnpm test`
Expected: PASS — all suites.

- [ ] **Step 8: Commit**

```bash
git add frontend/src/components/StandbyTray.tsx frontend/src/components/__tests__/StandbyTray.test.tsx frontend/src/styles/globals.css frontend/src/App.tsx
git commit -m "feat: tray mic toggle wires to setListening; boot fetch hydrates store"
```

---

### Task 8: LiveTab — replace RecordingTab with the new shape

**Files:**
- Create: `frontend/src/components/LiveTab.tsx` (overwrite the stub)
- Delete: `frontend/src/components/RecordingTab.tsx`
- Test: `frontend/src/components/__tests__/LiveTab.test.tsx`

- [ ] **Step 1: Write the failing tests**

Create `frontend/src/components/__tests__/LiveTab.test.tsx`:

```tsx
import { render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { LiveTab } from '../LiveTab';
import { LanguageProvider } from '../../i18n';
import { useStore } from '../../store/use-store';
import { useVoiceStore } from '../../store/use-voice-store';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));

function renderTab() {
  return render(
    <LanguageProvider>
      <LiveTab />
    </LanguageProvider>,
  );
}

describe('LiveTab', () => {
  beforeEach(() => {
    Object.defineProperty(navigator, 'language', { value: 'en-US', configurable: true });
    useStore.setState((state) => ({
      ui: { ...state.ui, listeningEnabled: true, listeningStartedAt: Date.now() },
    }));
    useVoiceStore.setState({ currentSession: null, isRecording: false });
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true, json: () => Promise.resolve({}) }));
  });

  it('shows the on-state header label when listening', () => {
    renderTab();
    expect(screen.getByText('Listening')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /click to mute/i })).toBeInTheDocument();
  });

  it('shows the muted hint when listening is off', () => {
    useStore.setState((state) => ({
      ui: { ...state.ui, listeningEnabled: false, listeningStartedAt: null },
    }));
    renderTab();
    expect(screen.getByText('Muted')).toBeInTheDocument();
    expect(screen.getByText(/Listening is paused/i)).toBeInTheDocument();
  });

  it('does not render manual record button', () => {
    renderTab();
    expect(screen.queryByRole('button', { name: /start.*transcribing/i })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /stop.*transcribing/i })).not.toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd frontend && pnpm exec vitest run src/components/__tests__/LiveTab.test.tsx`
Expected: FAIL — the stub LiveTab still wraps `RecordingTab`, which renders the manual record button.

- [ ] **Step 3: Replace `LiveTab.tsx` with the real implementation**

Overwrite `frontend/src/components/LiveTab.tsx`:

```tsx
import { useEffect, useRef, useState } from 'react';
import { useStore } from '../store/use-store';
import { useVoiceStore } from '../store/use-voice-store';
import { LiveTranscript } from './LiveTranscript';
import { ListeningToggle } from './ListeningToggle';
import { useT } from '../i18n';

function formatDuration(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const pad = (n: number) => String(n).padStart(2, '0');
  return h > 0 ? `${pad(h)}:${pad(m)}:${pad(s)}` : `${pad(m)}:${pad(s)}`;
}

function formatTime(ts: number): string {
  return new Date(ts).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
}

export function LiveTab() {
  const listeningEnabled = useStore((s) => s.ui.listeningEnabled);
  const listeningStartedAt = useStore((s) => s.ui.listeningStartedAt);
  const currentSession = useVoiceStore((s) => s.currentSession);
  const t = useT();

  const transcriptRef = useRef<HTMLDivElement>(null);
  const [now, setNow] = useState(Date.now());

  // Tick the "Listening since" duration once a second when on.
  useEffect(() => {
    if (!listeningEnabled || !listeningStartedAt) return;
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [listeningEnabled, listeningStartedAt]);

  useEffect(() => {
    if (transcriptRef.current) {
      transcriptRef.current.scrollTop = transcriptRef.current.scrollHeight;
    }
  }, [currentSession?.lines, currentSession?.pendingPartial]);

  const isOn = listeningEnabled === true;
  const headerLabel = isOn ? t('live.header.on') : t('live.header.off');

  return (
    <div className="live-tab">
      <div className="live-tab__header">
        <span className={`live-tab__status${isOn ? ' is-on' : ''}`}>{headerLabel}</span>
        <ListeningToggle size={32} />
      </div>

      {isOn && listeningStartedAt && (
        <p className="live-tab__since" aria-live="polite">
          {t('live.listeningSince', {
            time: formatTime(listeningStartedAt),
            duration: formatDuration(now - listeningStartedAt),
          })}
        </p>
      )}

      {!isOn && (
        <p className="live-tab__paused-hint">{t('live.pausedHint')}</p>
      )}

      {isOn && currentSession && (
        <div className="live-tab__transcript" ref={transcriptRef} aria-live="polite">
          <LiveTranscript
            lines={currentSession.lines}
            pendingPartial={currentSession.pendingPartial}
          />
        </div>
      )}
    </div>
  );
}
```

- [ ] **Step 4: Update `useT` interpolation if needed**

If the existing `useT` does not support the `{time}` and `{duration}` interpolation tokens, check `frontend/src/i18n/index.ts` for the implementation. The codebase already uses `{count}` etc. — the same `replaceAll('{key}', value)` loop should work as-is.

Verify by running:

```bash
cd frontend && pnpm exec vitest run src/i18n/__tests__/useT.test.tsx
```

Expected: existing tests still PASS (no interpolation regression).

- [ ] **Step 5: Update CSS — rename `recording-tab__*` selectors to `live-tab__*`**

Run a global search-and-replace in `frontend/src/styles/globals.css`: every `.recording-tab__` occurrence becomes `.live-tab__`. Then add the new header rules:

```css
.live-tab {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 16px;
}
.live-tab__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  font-weight: 600;
}
.live-tab__status {
  color: var(--color-text-muted);
  letter-spacing: 0.02em;
  text-transform: uppercase;
  font-size: 12px;
}
.live-tab__status.is-on {
  color: var(--color-accent);
}
.live-tab__since {
  font-size: 13px;
  color: var(--color-text-muted);
  margin: 0;
}
.live-tab__paused-hint {
  font-size: 13px;
  color: var(--color-text-muted);
  background: var(--color-surface-muted);
  border-radius: 8px;
  padding: 12px;
  margin: 0;
}
```

(The `recording-btn`, `recording-btn__halo`, `loading-dots*`, `recording-tab__*` rules that were specific to manual-record UI can be removed entirely since LiveTab no longer references them. Run `git grep -n recording-tab__ frontend/` after the rename to verify nothing else references the old class names.)

- [ ] **Step 6: Delete `RecordingTab.tsx`**

```bash
git rm frontend/src/components/RecordingTab.tsx
```

- [ ] **Step 7: Run the full frontend test suite**

Run: `cd frontend && pnpm test`
Expected: PASS — including the new LiveTab tests and the existing parity test.

- [ ] **Step 8: Commit**

```bash
git add frontend/src/components/LiveTab.tsx frontend/src/components/__tests__/LiveTab.test.tsx frontend/src/styles/globals.css
git commit -m "feat: LiveTab replaces RecordingTab; drops manual record/stop UI"
```

---

### Task 9: Global hotkey — `toggle_listening` default + handler

**Files:**
- Modify: `frontend/src/hooks/useGlobalShortcuts.ts`
- Test: `frontend/src/hooks/__tests__/useGlobalShortcuts.test.tsx` (new)

- [ ] **Step 1: Write the failing test**

Create `frontend/src/hooks/__tests__/useGlobalShortcuts.test.tsx`:

```tsx
import { renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useGlobalShortcuts } from '../useGlobalShortcuts';
import { useStore } from '../../store/use-store';

const listeners: Record<string, ((e: { payload: string }) => void)> = {};

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue(undefined),
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn((eventName: string, cb: (e: { payload: string }) => void) => {
    listeners[eventName] = cb;
    return Promise.resolve(() => {});
  }),
}));

describe('useGlobalShortcuts — toggle_listening', () => {
  beforeEach(() => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    useStore.setState((state) => ({
      ui: { ...state.ui, listeningEnabled: true, listeningStartedAt: 1 },
    }));
  });

  afterEach(() => {
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
    for (const key of Object.keys(listeners)) delete listeners[key];
  });

  it('inverts the listening state when the shortcut fires', async () => {
    const spy = vi.spyOn(useStore.getState(), 'setListening').mockResolvedValue();
    renderHook(() => useGlobalShortcuts());
    // wait one microtask for listen() promise to resolve
    await Promise.resolve();
    listeners['shortcut-triggered']?.({ payload: 'toggle_listening' });
    expect(spy).toHaveBeenCalledWith(false);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd frontend && pnpm exec vitest run src/hooks/__tests__/useGlobalShortcuts.test.tsx`
Expected: FAIL — `setListening` is never called because `useGlobalShortcuts` has no branch for `toggle_listening`.

- [ ] **Step 3: Add the default shortcut and handler**

In `frontend/src/hooks/useGlobalShortcuts.ts`:

(a) Find `DEFAULT_GLOBAL_SHORTCUTS` (around line 9). Add the new entry:

```ts
const DEFAULT_GLOBAL_SHORTCUTS: Record<string, string> = {
  toggle_board_tray: 'Ctrl+\\',
  start_dictation: 'Ctrl+Shift+Space',
  new_todo: 'Ctrl+N',
  toggle_listening: 'Ctrl+Shift+M',
};
```

(b) In the `shortcut-triggered` listener (around line 83-130), add a new `else if` branch alongside the existing ones:

```ts
      } else if (action === 'toggle_listening') {
        const current = useStore.getState().ui.listeningEnabled;
        if (current === null) return;
        const next = !current;
        void useStore.getState().setListening(next);
        useStore.getState().setFeedback(
          next ? 'feedback.listeningOn' : 'feedback.listeningOff',
          'success',
        );
      }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd frontend && pnpm exec vitest run src/hooks/__tests__/useGlobalShortcuts.test.tsx`
Expected: PASS.

- [ ] **Step 5: Add the action to KeyboardSettings**

In `frontend/src/components/settings/KeyboardSettings.tsx`:

(a) `DEFAULT_SHORTCUTS` (line 9-25), add:

```ts
  toggle_listening: 'Ctrl+Shift+M',
```

immediately under `new_todo`.

(b) `ACTION_LABEL_KEYS` (line 27-40), add:

```ts
  toggle_listening: 'settings.shortcuts.action.toggle_listening',
```

(c) `GLOBAL_ACTIONS` set (line 42), update to:

```ts
const GLOBAL_ACTIONS = new Set(['toggle_board_tray', 'start_dictation', 'new_todo', 'toggle_listening']);
```

(d) `groups[0].actions` (line 142), update to:

```ts
      actions: ['toggle_board_tray', 'start_dictation', 'new_todo', 'toggle_listening'],
```

- [ ] **Step 6: Run the full frontend test suite**

Run: `cd frontend && pnpm test`
Expected: PASS — all suites.

- [ ] **Step 7: Commit**

```bash
git add frontend/src/hooks/useGlobalShortcuts.ts frontend/src/hooks/__tests__/useGlobalShortcuts.test.tsx frontend/src/components/settings/KeyboardSettings.tsx
git commit -m "feat: global shortcut Ctrl+Shift+M toggles listening with toast feedback"
```

---

### Task 10: Backend — one-shot `tab_recording → tab_live` shortcut migration

**Files:**
- Modify: `backend/actio-core/src/engine/app_settings.rs`

- [ ] **Step 1: Write the failing test**

In `backend/actio-core/src/engine/app_settings.rs`, find the existing `mod tests` block (or create one if absent — search the file for `#[cfg(test)]`). Add:

```rust
    #[test]
    fn migrates_tab_recording_shortcut_to_tab_live() {
        let mut shortcuts = std::collections::HashMap::new();
        shortcuts.insert("tab_recording".to_string(), "Ctrl+9".to_string());
        let mut keyboard = KeyboardSettings { shortcuts };

        super::migrate_tab_recording_shortcut(&mut keyboard);

        assert!(!keyboard.shortcuts.contains_key("tab_recording"));
        assert_eq!(keyboard.shortcuts.get("tab_live"), Some(&"Ctrl+9".to_string()));
    }

    #[test]
    fn migrate_no_op_when_tab_live_already_set() {
        let mut shortcuts = std::collections::HashMap::new();
        shortcuts.insert("tab_recording".to_string(), "Ctrl+9".to_string());
        shortcuts.insert("tab_live".to_string(), "Ctrl+3".to_string());
        let mut keyboard = KeyboardSettings { shortcuts };

        super::migrate_tab_recording_shortcut(&mut keyboard);

        assert_eq!(keyboard.shortcuts.get("tab_live"), Some(&"Ctrl+3".to_string()));
        assert!(!keyboard.shortcuts.contains_key("tab_recording"));
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd backend && cargo test -p actio-core --lib --no-default-features --features local-llm migrates_tab_recording`
Expected: FAIL — `migrate_tab_recording_shortcut` does not exist.

- [ ] **Step 3: Add the migration function**

In `backend/actio-core/src/engine/app_settings.rs`, near the existing `migrate_legacy_selection` function (around line 89), add:

```rust
/// Post-deser migration: copy a stale `tab_recording` shortcut binding to
/// `tab_live` (renamed in the always-on listening feature) and drop the
/// old key. No-op if `tab_live` is already set or `tab_recording` is
/// absent. Runs once per process via `SettingsManager::new`.
pub fn migrate_tab_recording_shortcut(keyboard: &mut KeyboardSettings) {
    let Some(value) = keyboard.shortcuts.remove("tab_recording") else {
        return;
    };
    keyboard.shortcuts.entry("tab_live".to_string()).or_insert(value);
}
```

(`entry().or_insert()` preserves an existing `tab_live` rather than overwriting it.)

- [ ] **Step 4: Wire it into `SettingsManager::new`**

In `backend/actio-core/src/engine/app_settings.rs`, find the migration block in `SettingsManager::new` (around line 346-348):

```rust
        // Post-deser migration: promote Disabled → Remote for legacy users
        migrate_legacy_selection(&mut settings.llm);
```

Add immediately after:

```rust
        migrate_tab_recording_shortcut(&mut settings.keyboard);
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cd backend && cargo test -p actio-core --lib --no-default-features --features local-llm migrate`
Expected: PASS — both new tests green.

- [ ] **Step 6: Run the full backend test suite to confirm no regressions**

Run: `cd backend && cargo test -p actio-core --lib --no-default-features --features local-llm`
Expected: PASS — 107+ tests green.

- [ ] **Step 7: Commit**

```bash
git add backend/actio-core/src/engine/app_settings.rs
git commit -m "backend: migrate stale tab_recording shortcut to tab_live"
```

---

### Task 11: End-to-end verification

**Files:** None — runtime-only validation.

- [ ] **Step 1: Run the full frontend + backend test suite**

```bash
cd frontend && pnpm test
cd backend && cargo test -p actio-core --lib --no-default-features --features local-llm
```

Expected: PASS in both. No new warnings introduced by these tasks.

- [ ] **Step 2: Run typecheck + lint**

```bash
cd frontend && pnpm tsc --noEmit
cd backend && cargo clippy -p actio-core --no-default-features --features local-llm -- -D warnings
```

Expected: PASS — no errors.

- [ ] **Step 3: Manual smoke test**

Start the app:

```bash
cd backend && tauri dev
```

Verify each of the spec's verification points (paraphrased here):

1. **Toggle off in tray** → mic LED turns off within 1–2 s; wordmark snaps to standby; mic icon shows the slashed glyph.
2. **Toggle on** → mic LED on; ~2 s warm-up; first transcript chunk arrives; wordmark animates with sonar rings.
3. **Press `Ctrl+Shift+Space` while toggle is off** → dictation works; success flash on paste; pipeline hibernates again.
4. **Press `Ctrl+Shift+M` while tray is occluded** → toast shows "Listening on" / "Listening off"; setting persists across restart (close + reopen the app and check the mic icon state).
5. **Open Live tab** → header label matches tray; "Listening since" timer ticks; toggle button works identically.
6. **Restart the app** with `tab_recording: Ctrl+9` in `%APPDATA%/com.actio.desktop/settings.json` (manually edit before the restart) → after restart, the file shows `tab_live: Ctrl+9` and `tab_recording` is gone.

- [ ] **Step 4: If everything passes, no further commits.** If any verification step fails, stop and reopen the relevant earlier task to debug.

---

## Self-Review

**Spec coverage**

- Tray mic button: Tasks 5 + 7. ✓
- Wordmark behavior: Task 3. ✓
- Live tab rename + drop manual UI: Tasks 4, 6, 8. ✓
- Hotkey + KeyboardSettings: Task 9. ✓
- Backend `tab_recording → tab_live` migration: Task 10. ✓
- Edge cases: dictation while off (verified in step 11.3), settings PATCH fail (covered by setListening test in Task 2), boot null state (covered by ListeningToggle test in Task 5). ✓
- Tests: every task has a test step before its implementation step. ✓
- New i18n keys: all present in Task 4 and matched in zh-CN. parity test enforces it. ✓

**Placeholder scan**

- No "TBD" / "TODO" / vague-handling phrases. Every code step shows the exact code.
- One soft escape: Task 6 step 5 says "fix call sites that reference `'recording'` as a Tab literal" via grep — concrete enough since the engineer can run the grep and see them.

**Type consistency**

- `setListening: (enabled: boolean) => Promise<void>` — used identically in Tasks 2, 5, 7, 9. ✓
- `loadListening: () => Promise<void>` — defined in Task 2, called in Task 7. ✓
- `Tab = 'live' | ...` — set in Task 1, consumed in Task 6. ✓
- `listeningEnabled: boolean | null` — defined in Task 1, read in Tasks 3 + 5 + 8 + 9. ✓
- `migrate_tab_recording_shortcut` — defined and tested in Task 10. ✓

No issues found.
