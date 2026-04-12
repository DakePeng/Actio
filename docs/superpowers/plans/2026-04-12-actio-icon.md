# Actio Icon Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the tray's green pulsing dot and the board header's checkmark with an animated Grand Hotel "A" icon that flows color through three states (recording, processing, paused).

**Architecture:** A shared `ActioIcon` React component renders a styled `<span>` with Grand Hotel font and `background-clip: text`. CSS Houdini `@property` animates a conic gradient angle. The component accepts a `state` prop (`recording | processing | paused`) which maps to a CSS class. Both `StandbyTray` and `Header` import and use this component at their respective sizes.

**Tech Stack:** React, CSS Houdini `@property`, Grand Hotel font (OFL, bundled woff2)

---

### Task 1: Bundle the Grand Hotel font

**Files:**
- Create: `frontend/public/fonts/GrandHotel-Regular.woff2`
- Modify: `frontend/src/styles/globals.css:1-2` (add `@font-face`)

- [ ] **Step 1: Download the font file**

Run:
```bash
mkdir -p frontend/public/fonts
curl -L -o frontend/public/fonts/GrandHotel-Regular.woff2 \
  "https://fonts.gstatic.com/s/grandhotel/v19/7Au7p_IgjDKdCRWuR1azpmQNEl0O0kEx.woff2"
```

Expected: File appears at `frontend/public/fonts/GrandHotel-Regular.woff2` (~15-20KB)

- [ ] **Step 2: Add @font-face declaration to globals.css**

At the top of `frontend/src/styles/globals.css`, after the `@import "tailwindcss";` line, add:

```css
@font-face {
  font-family: 'Grand Hotel';
  src: url('/fonts/GrandHotel-Regular.woff2') format('woff2');
  font-weight: 400;
  font-style: normal;
  font-display: swap;
}
```

- [ ] **Step 3: Verify the font loads**

Run: `pnpm dev`

Open the browser devtools → Network tab. Confirm `GrandHotel-Regular.woff2` loads. You can add a temporary `<span style="font-family: 'Grand Hotel'">A</span>` anywhere to verify rendering.

- [ ] **Step 4: Commit**

```bash
git add frontend/public/fonts/GrandHotel-Regular.woff2 frontend/src/styles/globals.css
git commit -m "feat(icon): bundle Grand Hotel font for actio icon"
```

---

### Task 2: Add Houdini @property and icon CSS classes

**Files:**
- Modify: `frontend/src/styles/globals.css`

- [ ] **Step 1: Add the @property declaration and icon styles**

Add the following CSS after the `@font-face` declaration (before the `@theme` block):

```css
@property --gradient-angle {
  syntax: "<angle>";
  initial-value: 0deg;
  inherits: false;
}
```

Then add the following classes at the end of the file (before any alternative theme overrides):

```css
/* === Actio Icon: animated gradient letter === */
.actio-icon {
  display: inline-block;
  font-family: 'Grand Hotel', cursive;
  font-weight: 400;
  line-height: 1;
  -webkit-background-clip: text;
  background-clip: text;
  -webkit-text-fill-color: transparent;
  --gradient-angle: 0deg;
}

.actio-icon--recording {
  background-image: conic-gradient(
    from var(--gradient-angle) at 50% 55%,
    #8b5cf6 0deg,
    #8b5cf6 60deg,
    #9b8af5 90deg,
    #b4a4fc 120deg,
    #c4b5fd 150deg,
    #ddd6fe 180deg,
    #c4b5fd 210deg,
    #b4a4fc 230deg,
    #9b8af5 260deg,
    #8b5cf6 300deg,
    #8b5cf6 360deg
  );
  animation: actio-flow 4s linear infinite;
}

.actio-icon--processing {
  background-image: conic-gradient(
    from var(--gradient-angle) at 50% 55%,
    #4f46e5 0deg,
    #4f46e5 60deg,
    #6366f1 90deg,
    #06b6d4 130deg,
    #22d3ee 170deg,
    #67e8f9 190deg,
    #22d3ee 220deg,
    #06b6d4 250deg,
    #6366f1 280deg,
    #4f46e5 320deg,
    #4f46e5 360deg
  );
  animation: actio-flow 6s linear infinite;
}

.actio-icon--paused {
  background-image: conic-gradient(
    from 135deg at 50% 55%,
    #475569 0deg,
    #475569 90deg,
    #64748b 150deg,
    #94a3b8 180deg,
    #64748b 210deg,
    #475569 270deg,
    #475569 360deg
  );
}

@keyframes actio-flow {
  to { --gradient-angle: 360deg; }
}

/* Light background adjustment */
.light-bg .actio-icon--recording,
.light-bg .actio-icon--processing {
  filter: brightness(0.8) saturate(1.3);
}
```

- [ ] **Step 2: Verify CSS is valid**

Run: `pnpm dev`

No build errors. The classes exist but aren't used yet — that's fine.

- [ ] **Step 3: Commit**

```bash
git add frontend/src/styles/globals.css
git commit -m "feat(icon): add Houdini @property and actio-icon CSS classes"
```

---

### Task 3: Create the ActioIcon React component

**Files:**
- Create: `frontend/src/components/ActioIcon.tsx`

- [ ] **Step 1: Create the component**

Create `frontend/src/components/ActioIcon.tsx`:

```tsx
export type ActioIconState = 'recording' | 'processing' | 'paused';

interface ActioIconProps {
  state: ActioIconState;
  size: number;
  className?: string;
}

export function ActioIcon({ state, size, className = '' }: ActioIconProps) {
  return (
    <span
      className={`actio-icon actio-icon--${state} ${className}`}
      style={{ fontSize: size }}
      aria-hidden="true"
    >
      A
    </span>
  );
}
```

- [ ] **Step 2: Verify it compiles**

Run: `pnpm dev`

No build errors.

- [ ] **Step 3: Commit**

```bash
git add frontend/src/components/ActioIcon.tsx
git commit -m "feat(icon): create ActioIcon component"
```

---

### Task 4: Integrate into StandbyTray

**Files:**
- Modify: `frontend/src/components/StandbyTray.tsx:1-9` (imports), `136` (replace dot)

- [ ] **Step 1: Add imports**

At the top of `frontend/src/components/StandbyTray.tsx`, add:

```tsx
import { ActioIcon } from './ActioIcon';
import { useVoiceStore } from '../store/use-voice-store';
```

- [ ] **Step 2: Read voice state inside the component**

Inside the `StandbyTray` function, after the existing store hooks (around line 25), add:

```tsx
const isRecording = useVoiceStore((s) => s.isRecording);
```

- [ ] **Step 3: Replace the tray-brand-dot**

Replace line 136:
```tsx
<span className="tray-brand-dot" aria-hidden="true" />
```

With:
```tsx
<ActioIcon state={isRecording ? 'recording' : 'paused'} size={20} />
```

- [ ] **Step 4: Verify in browser**

Run: `pnpm dev`

The tray should now show the Grand Hotel "A" instead of the green dot. When recording is active, the gradient should flow through the letter. When paused, it should be static slate.

- [ ] **Step 5: Commit**

```bash
git add frontend/src/components/StandbyTray.tsx
git commit -m "feat(icon): replace tray dot with animated ActioIcon"
```

---

### Task 5: Integrate into Board Header

**Files:**
- Modify: `frontend/src/components/Header.tsx`

- [ ] **Step 1: Add imports**

At the top of `frontend/src/components/Header.tsx`, add:

```tsx
import { ActioIcon } from './ActioIcon';
import { useVoiceStore } from '../store/use-voice-store';
```

- [ ] **Step 2: Read voice state and replace the brand-mark**

Replace the entire `Header` component body:

```tsx
export function Header() {
  const isRecording = useVoiceStore((s) => s.isRecording);

  return (
    <header className="topbar">
      <div className="topbar__inner">
        <div className="brand-lockup">
          <div className="brand-mark" aria-hidden="true">
            <ActioIcon state={isRecording ? 'recording' : 'paused'} size={18} />
          </div>
          <div className="brand-copy">
            <div className="brand-title">actio</div>
            <div className="brand-subtitle">Voice-captured reminders, organized for action.</div>
          </div>
        </div>

        <div className="topbar-actions">
          <div className="topbar-avatar" aria-label="Current user">
            JD
          </div>
        </div>
      </div>
    </header>
  );
}
```

- [ ] **Step 3: Update brand-mark CSS to remove old styles**

In `frontend/src/styles/globals.css`, update the `.brand-mark` rule (line ~208) to remove the checkmark-specific styles:

Replace:
```css
.brand-mark {
  width: 42px;
  height: 42px;
  border-radius: 14px;
  display: grid;
  place-items: center;
  background: var(--color-accent);
  color: white;
  box-shadow: 0 14px 32px rgba(79, 70, 229, 0.28);
}
```

With:
```css
.brand-mark {
  width: 42px;
  height: 42px;
  display: grid;
  place-items: center;
}
```

- [ ] **Step 4: Verify in browser**

Run: `pnpm dev`

Open the board. The header should show the Grand Hotel "A" with flowing gradient instead of the old checkmark in a colored box.

- [ ] **Step 5: Commit**

```bash
git add frontend/src/components/Header.tsx frontend/src/styles/globals.css
git commit -m "feat(icon): replace board header checkmark with ActioIcon"
```

---

### Task 6: Clean up old tray-brand-dot CSS

**Files:**
- Modify: `frontend/src/styles/globals.css:1080-1086` (remove dot styles), `2144-2146` (remove dark override)

- [ ] **Step 1: Remove the tray-brand-dot styles**

In `frontend/src/styles/globals.css`, remove the `.tray-brand-dot` rule at line ~1080:

```css
.tray-brand-dot {
  width: 8px;
  height: 8px;
  border-radius: 999px;
  background: var(--color-accent);
  animation: pulse 2s infinite;
}
```

Also remove the dark-mode override `.tray-brand-dot` at line ~2144:

```css
.tray-brand-dot {
  animation: none;
}
```

- [ ] **Step 2: Remove the pulse keyframes (if unused elsewhere)**

Search for other uses of the `pulse` animation:

Run: `grep -r "pulse" frontend/src/styles/globals.css | grep -v "tray-brand-dot" | grep -v "@keyframes pulse"`

If no other rules reference `pulse`, remove the `@keyframes pulse` block at line ~1509:

```css
@keyframes pulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.5; }
}
```

If other rules reference it, leave it.

- [ ] **Step 3: Verify no visual regressions**

Run: `pnpm dev`

Check the tray and board — both should render the new icon correctly. No broken styles.

- [ ] **Step 4: Commit**

```bash
git add frontend/src/styles/globals.css
git commit -m "refactor(icon): remove old tray-brand-dot and pulse animation"
```

---

### Task 7: Add processing state support

The voice store currently only has `isRecording`. The "processing" state occurs when the pipeline is working on captured audio (between when recording stops and when the reminder is created). For now, we can wire this up when the infrastructure exists. This task adds a simple `useActioIconState` hook that centralizes the state derivation.

**Files:**
- Create: `frontend/src/hooks/useActioIconState.ts`
- Modify: `frontend/src/components/StandbyTray.tsx`
- Modify: `frontend/src/components/Header.tsx`

- [ ] **Step 1: Create the hook**

Create `frontend/src/hooks/useActioIconState.ts`:

```tsx
import { useVoiceStore } from '../store/use-voice-store';
import type { ActioIconState } from '../components/ActioIcon';

/**
 * Derives the current icon state from the voice store.
 *
 * - recording: microphone is active
 * - processing: recording just stopped and pipeline hasn't finished
 *               (for now, always false — will be wired when the backend
 *                emits a "processing complete" event)
 * - paused: idle
 */
export function useActioIconState(): ActioIconState {
  const isRecording = useVoiceStore((s) => s.isRecording);
  if (isRecording) return 'recording';
  return 'paused';
}
```

- [ ] **Step 2: Update StandbyTray to use the hook**

In `frontend/src/components/StandbyTray.tsx`:

Replace the import:
```tsx
import { useVoiceStore } from '../store/use-voice-store';
```
With:
```tsx
import { useActioIconState } from '../hooks/useActioIconState';
```

Replace:
```tsx
const isRecording = useVoiceStore((s) => s.isRecording);
```
With:
```tsx
const iconState = useActioIconState();
```

Replace:
```tsx
<ActioIcon state={isRecording ? 'recording' : 'paused'} size={20} />
```
With:
```tsx
<ActioIcon state={iconState} size={20} />
```

- [ ] **Step 3: Update Header to use the hook**

In `frontend/src/components/Header.tsx`:

Replace the import:
```tsx
import { useVoiceStore } from '../store/use-voice-store';
```
With:
```tsx
import { useActioIconState } from '../hooks/useActioIconState';
```

Replace:
```tsx
const isRecording = useVoiceStore((s) => s.isRecording);
```
With:
```tsx
const iconState = useActioIconState();
```

Replace:
```tsx
<ActioIcon state={isRecording ? 'recording' : 'paused'} size={18} />
```
With:
```tsx
<ActioIcon state={iconState} size={18} />
```

- [ ] **Step 4: Verify everything still works**

Run: `pnpm dev`

Tray and board header both show the animated icon. Recording state triggers the violet flow. Paused state shows static slate.

- [ ] **Step 5: Commit**

```bash
git add frontend/src/hooks/useActioIconState.ts frontend/src/components/StandbyTray.tsx frontend/src/components/Header.tsx
git commit -m "feat(icon): extract useActioIconState hook for state derivation"
```
