# Actio Frontend Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Tauri desktop app with a minimal sticky-note board for voice-detected reminders, featuring one signature card-appearance animation, swipe-to-dismiss, standby tray widget, and system notifications.

**Architecture:** Tauri v2 shell with a single-page React frontend. Four surfaces: Main Board grid, System Notification handler, Standby Tray widget, and Card Detail drawer. Rust backend pushes reminder events via Tauri events; frontend fetches data via Tauri invoke commands. Zustand for state management, Tailwind for styling, Framer Motion for the two signature animations (card appear, swipe dismiss).

**Tech Stack:** Tauri v2, React + Vite + TypeScript, Zustand, Tailwind CSS, Framer Motion, tauri-plugin-notification

---

## File Structure

### Tauri Config (Root)
- Create: `package.json` — Project manifest, React + Vite scripts
- Create: `tsconfig.json` — Root TypeScript config
- Create: `vite.config.ts` — Vite config with React plugin
- Create: `src-tauri/Cargo.toml` — Rust dependencies
- Create: `src-tauri/tauri.conf.json` — Tauri v2 config (frameless window, system tray)
- Create: `src-tauri/capabilities/default.json` — Tauri capabilities

### Frontend Entry
- Create: `index.html` — Root HTML file
- Create: `src/main.tsx` — React entry point
- Create: `src/App.tsx` — App shell, keyboard handler, Tauri event listener

### State
- Create: `src/store/use-store.ts` — Zustand store: reminders array, filter state, UI state, tray state
- Create: `src/types/index.ts` — TypeScript types: Reminder, Label, Priority

### Tauri Bridge
- Create: `src/tauri/commands.ts` — Tauri invoke wrappers: fetchReminders, markDone, addReminder
- Create: `src/tauri/events.ts` — Tauri event listeners: new-reminder event handler

### Components
- Create: `src/components/Board.tsx` — Main grid layout, empty state, card list
- Create: `src/components/Card.tsx` — Reminder card component, click-to-expand detail
- Create: `src/components/CardDetail.tsx` — Expanded card: transcript, labels, context, dismiss
- Create: `src/components/CardAppearance.tsx` — The "wow moment" animation wrapper (paper-fold)
- Create: `src/components/SwipeDismiss.tsx` — Swipe-to-dismiss animation wrapper
- Create: `src/components/Header.tsx` — Fixed header: logo, search, labels button, avatar
- Create: `src/components/LabelsPanel.tsx` — Slide-in panel: label list with counts, filter by click
- Create: `src/components/NewReminderBar.tsx` — Inline input that slides up from bottom
- Create: `src/components/StandbyTray.tsx` — Corner widget: 3 states (collapsed/hover/click), badge, pulse
- Create: `src/components/SystemNotification.tsx` — Tauri notification bridge logic
- Create: `src/components/EmptyState.tsx` — "All clear. I'm listening." with pulsing dot
- Create: `src/components/OnboardingCard.tsx` — First-launch translucent hint card, auto-fades

### Utilities
- Create: `src/utils/priority.ts` — Priority type helpers, sort function
- Create: `src/utils/labels.ts` — Label color constants, filter logic
- Create: `src/utils/keyboard.ts` — Keyboard shortcuts: only fire when not in input
- Create: `src/utils/time.ts` — Time formatting relative ("Due today", "In 30 min")

### Design
- Create: `src/styles/globals.css` — Tailwind imports, custom animations, design tokens as CSS variables

### Test
- Create: `src/__tests__/CardAppearance.test.tsx` — Paper-fold animation triggers
- Create: `src/__tests__/SwipeDismiss.test.tsx` — Swipe threshold, done callback
- Create: `src/__tests__/store.test.ts` — Zustand store actions
- Create: `src/__tests__/keyboard.test.ts` — Shortcuts only fire outside inputs
- Create: `src/__tests__/priority.test.ts` — Sort order, type guards
- Create: `setupTests.ts` — Vitest + React Testing Library

### Mock Backend
- Create: `src-tauri/src/mock_data.rs` — Seed data for development (no real ASR yet)
- Create: `src-tauri/src/main.rs` — Tauri app init, mock data bridge commands

---

### Task 1: Project Scaffold

**Files:**
- Create: `package.json`, `tsconfig.json`, `vite.config.ts`
- Create: `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, `src-tauri/capabilities/default.json`
- Create: `index.html`

- [ ] **Step 1: Create package.json**

```json
{
  "name": "actio-frontend",
  "private": true,
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "tsc -b && vite build",
    "preview": "vite preview",
    "tauri": "tauri",
    "test": "vitest"
  },
  "dependencies": {
    "react": "^19",
    "react-dom": "^19",
    "zustand": "^5",
    "framer-motion": "^12"
  },
  "devDependencies": {
    "@tauri-apps/cli": "^2",
    "@tauri-apps/api": "^2",
    "@tauri-apps/plugin-notification": "^2",
    "@types/react": "^19",
    "@types/react-dom": "^19",
    "@testing-library/react": "^16",
    "@testing-library/jest-dom": "^6",
    "typescript": "^5",
    "vite": "^6",
    "@vitejs/plugin-react": "^4",
    "tailwindcss": "^4",
    "@tailwindcss/vite": "^4",
    "vitest": "^3",
    "jsdom": "^26"
  }
}
```

- [ ] **Step 2: Create tsconfig.json**

```json
{
  "compilerOptions": {
    "target": "ES2020",
    "useDefineForClassFields": true,
    "lib": ["ES2020", "DOM", "DOM.Iterable"],
    "module": "ESNext",
    "skipLibCheck": true,
    "moduleResolution": "bundler",
    "allowImportingTsExtensions": true,
    "isolatedModules": true,
    "moduleDetection": "force",
    "noEmit": true,
    "jsx": "react-jsx",
    "strict": true,
    "noUnusedLocals": true,
    "noUnusedParameters": true,
    "noFallthroughCasesInSwitch": true,
    "forceConsistentCasingInFileNames": true
  },
  "include": ["src"]
}
```

- [ ] **Step 3: Create vite.config.ts**

```typescript
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';

export default defineConfig({
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ['**/src-tauri/**'] },
  },
});
```

- [ ] **Step 4: Create index.html**

```html
<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>Actio</title>
    <script>
      document.documentElement.style.background = '#F8F9FB';
    </script>
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/src/main.tsx"></script>
  </body>
</html>
```

- [ ] **Step 5: Create src-tauri/Cargo.toml**

```toml
[package]
name = "actio"
version = "0.1.0"
description = "Actio - AI reminder desktop app"
authors = ["you"]
edition = "2021"

[[bin]]
name = "actio"
path = "src/main.rs"

[dependencies]
tauri = { version = "2", features = [] }
tauri-plugin-shell = "2"
tauri-plugin-notification = "2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
chrono = "0.4"

[features]
default = [ "custom-protocol" ]
custom-protocol = [ "tauri/custom-protocol" ]
```

- [ ] **Step 6: Create src-tauri/tauri.conf.json**

```json
{
  "productName": "Actio",
  "version": "0.1.0",
  "identifier": "com.actio.app",
  "build": {
    "beforeDevCommand": "bun dev",
    "frontendDist": "../dist"
  },
  "app": {
    "windows": [
      {
        "title": "Actio",
        "width": 1200,
        "height": 800,
        "resizable": true,
        "decorations": false,
        "transparent": true
      }
    ],
    "withGlobalTauri": true
  },
  "bundle": {
    "active": true,
    "targets": "all",
    "icon": [
      "icons/32x32.png",
      "icons/128x128.png",
      "icons/128x128@2x.png",
      "icons/icon.icns",
      "icons/icon.ico"
    ]
  }
}
```

- [ ] **Step 7: Create src-tauri/capabilities/default.json**

```json
{
  "identifier": "default",
  "windows": ["main"],
  "permissions": [
    "core:default",
    "shell:allow-open",
    "notification:default",
    "notification:allow-is-permission-granted",
    "notification:allow-request-permission",
    "notification:allow-notify"
  ]
}
```

- [ ] **Step 8: Create src-tauri/src/main.rs**

```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "scaffold: Tauri v2 + React + Vite + TypeScript project"
```

---

### Task 2: Types & Design Tokens

**Files:**
- Create: `src/types/index.ts`
- Create: `src/styles/globals.css`
- Create: `tailwind.config.ts`

- [ ] **Step 1: Create types** (`src/types/index.ts`)

```typescript
export type Priority = 'high' | 'medium' | 'low';

export interface Label {
  id: string;
  name: string;
  color: string;      // hex, e.g. "#6366F1"
  bgColor: string;    // hex, e.g. "#EEF2FF"
  count: number;
}

export interface Reminder {
  id: string;
  title: string;
  description: string;
  priority: Priority;
  dueTime?: string;   // ISO date string
  labels: string[];   // label IDs
  transcript?: string;
  context?: string;
  sourceTime?: string; // When it was detected from voice
  isNew?: boolean;    // True for card-appearance animation
  createdAt: string;
}

export type FilterState = {
  priority: Priority | null;
  label: string | null;
  search: string;
};

export type UIState = {
  showLabelsPanel: boolean;
  expandedCardId: string | null;
  showNewReminderBar: boolean;
  hasSeenOnboarding: boolean;
};
```

- [ ] **Step 2: Create globals.css** (`src/styles/globals.css`)

```css
@import "tailwindcss";

@theme {
  --color-bg: #F8F9FB;
  --color-surface: #FFFFFF;
  --color-border: #E5E7EB;
  --color-text: #111827;
  --color-text-secondary: #6B7280;
  --color-text-tertiary: #9CA3AF;
  --color-accent: #6366F1;
  --color-accent-light: #EEF2FF;
  --color-priority-high-bg: #FEF2F2;
  --color-priority-high-text: #DC2626;
  --color-priority-med-bg: #FFFBEB;
  --color-priority-med-text: #D97706;
  --color-priority-low-bg: #F0FDF4;
  --color-priority-low-text: #16A34A;
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
  --radius-card: 14px;
  --radius-sm: 8px;
  --shadow-card-sm: 0 1px 3px rgba(0, 0, 0, 0.06), 0 1px 2px rgba(0, 0, 0, 0.04);
  --shadow-card-md: 0 4px 12px rgba(0, 0, 0, 0.08), 0 2px 4px rgba(0, 0, 0, 0.04);
  --shadow-card-lg: 0 12px 36px rgba(0, 0, 0, 0.12);
  --font-sans: 'Inter', -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
}

* {
  margin: 0;
  padding: 0;
  box-sizing: border-box;
}

body {
  font-family: var(--font-sans);
  background: var(--color-bg);
  color: var(--color-text);
  overflow-x: hidden;
}

::selection {
  background: var(--color-accent-light);
}
```

- [ ] **Step 3: Create tailwind config** — Tailwind v4 uses `@theme` in CSS, so no separate `tailwind.config.ts` is needed. Skip.

- [ ] **Step 4: Commit**

```bash
git add src/types/index.ts src/styles/globals.css
git commit -m "types: define Reminder, Label, FilterState, UIState + design tokens"
```

---

### Task 3: Zustand Store + Tests

**Files:**
- Create: `src/store/use-store.ts`
- Create: `src/__tests__/store.test.ts`
- Create: `setupTests.ts`

- [ ] **Step 1: Create setupTests.ts**

```typescript
import '@testing-library/jest-dom';
import { expect, afterEach } from 'vitest';
import { cleanup } from '@testing-library/react';
import * as matchers from '@testing-library/jest-dom/matchers';

expect.extend(matchers);

afterEach(() => {
  cleanup();
});
```

- [ ] **Step 2: Create store tests** (`src/__tests__/store.test.ts`)

```typescript
import { describe, it, expect, beforeEach } from 'vitest';
import { useStore } from '../store/use-store';

beforeEach(() => {
  useStore.getState().reset();
});

describe('useStore - reminders', () => {
  it('has empty reminders on init', () => {
    expect(useStore.getState().reminders).toEqual([]);
  });

  it('adds a reminder', () => {
    const reminder = {
      title: 'Test reminder',
      description: 'Test',
      priority: 'high' as const,
      labels: [],
      createdAt: new Date().toISOString(),
    };
    useStore.getState().addReminder(reminder);
    expect(useStore.getState().reminders).toHaveLength(1);
    expect(useStore.getState().reminders[0].title).toBe('Test reminder');
    expect(useStore.getState().reminders[0].id).toBeTruthy();
  });

  it('marks a reminder as done (removes it)', () => {
    const reminder = {
      id: '1',
      title: 'Test',
      description: 'Test',
      priority: 'medium' as const,
      labels: [],
      createdAt: new Date().toISOString(),
    };
    useStore.getState().setReminders([reminder]);
    expect(useStore.getState().reminders).toHaveLength(1);
    useStore.getState().markDone('1');
    expect(useStore.getState().reminders).toHaveLength(0);
  });

  it('filters reminders by priority', () => {
    const reminders = [
      { id: '1', title: 'High', description: '', priority: 'high' as const, labels: [], createdAt: new Date().toISOString() },
      { id: '2', title: 'Low', description: '', priority: 'low' as const, labels: [], createdAt: new Date().toISOString() },
    ];
    useStore.getState().setReminders(reminders);
    useStore.getState().setFilter({ priority: 'high', label: null, search: '' });
    const filtered = useStore.getState().getFilteredReminders();
    expect(filtered).toHaveLength(1);
    expect(filtered[0].priority).toBe('high');
  });
});

describe('useStore - UI state', () => {
  it('starts with panels closed', () => {
    const ui = useStore.getState().ui;
    expect(ui.showLabelsPanel).toBe(false);
    expect(ui.expandedCardId).toBeNull();
    expect(ui.showNewReminderBar).toBe(false);
  });

  it('toggles labels panel', () => {
    useStore.getState().toggleLabelsPanel();
    expect(useStore.getState().ui.showLabelsPanel).toBe(true);
    useStore.getState().toggleLabelsPanel();
    expect(useStore.getState().ui.showLabelsPanel).toBe(false);
  });
});
```

- [ ] **Step 3: Create the store** (`src/store/use-store.ts`)

```typescript
import { create } from 'zustand';
import type { Reminder, FilterState, UIState } from '../types';

interface AppState {
  reminders: Reminder[];
  filter: FilterState;
  ui: UIState;

  // Actions
  setReminders: (reminders: Reminder[]) => void;
  addReminder: (reminder: Omit<Reminder, 'id'>) => void;
  markDone: (id: string) => void;
  setFilter: (filter: Partial<FilterState>) => void;
  clearFilter: () => void;
  toggleLabelsPanel: () => void;
  setExpandedCard: (id: string | null) => void;
  setNewReminderBar: (show: boolean) => void;
  setHasSeenOnboarding: (seen: boolean) => void;
  getFilteredReminders: () => Reminder[];
  reset: () => void;
}

const initialState = {
  reminders: [],
  filter: { priority: null, label: null, search: '' },
  ui: {
    showLabelsPanel: false,
    expandedCardId: null,
    showNewReminderBar: false,
    hasSeenOnboarding: localStorage.getItem('actio-onboarded') === 'true',
  },
};

export const useStore = create<AppState>((set, get) => ({
  ...initialState,

  setReminders: (reminders) => set({ reminders }),

  addReminder: (reminder) =>
    set((state) => ({
      reminders: [
        ...state.reminders,
        { ...reminder, id: crypto.randomUUID(), isNew: true },
      ],
    })),

  markDone: (id) =>
    set((state) => ({
      reminders: state.reminders.filter((r) => r.id !== id),
    })),

  setFilter: (filter) =>
    set((state) => ({
      filter: { ...state.filter, ...filter },
    })),

  clearFilter: () =>
    set({ filter: { priority: null, label: null, search: '' } }),

  toggleLabelsPanel: () =>
    set((state) => ({
      ui: { ...state.ui, showLabelsPanel: !state.ui.showLabelsPanel },
    })),

  setExpandedCard: (id) =>
    set((state) => ({
      ui: { ...state.ui, expandedCardId: id },
    })),

  setNewReminderBar: (show) =>
    set((state) => ({
      ui: { ...state.ui, showNewReminderBar: show },
    })),

  setHasSeenOnboarding: (seen) => {
    localStorage.setItem('actio-onboarded', 'true');
    set((state) => ({
      ui: { ...state.ui, hasSeenOnboarding: seen },
    }));
  },

  getFilteredReminders: () => {
    const { reminders, filter } = get();
    return reminders.filter((r) => {
      if (filter.priority && r.priority !== filter.priority) return false;
      if (filter.label && !r.labels.includes(filter.label)) return false;
      if (filter.search) {
        const s = filter.search.toLowerCase();
        if (
          !r.title.toLowerCase().includes(s) &&
          !r.description.toLowerCase().includes(s)
        )
          return false;
      }
      return true;
    });
  },

  reset: () => set(initialState),
}));
```

- [ ] **Step 4: Add vitest config to vite.config.ts** — Add test section to the existing vite config:

```typescript
// Add to the existing export:
test: {
  globals: true,
  environment: 'jsdom',
  setupFiles: './setupTests.ts',
},
```

- [ ] **Step 5: Run tests to verify**

```bash
bun test src/__tests__/store.test.ts -run
```

Expected: ALL PASS

- [ ] **Step 6: Commit**

```bash
git add src/store/use-store.ts src/__tests__/store.test.ts setupTests.ts
git commit -m "store: Zustand state management with tests"
```

---

### Task 4: Utility Modules + Tests

**Files:**
- Create: `src/utils/priority.ts`
- Create: `src/utils/labels.ts`
- Create: `src/utils/keyboard.ts`
- Create: `src/utils/time.ts`
- Create: `src/__tests__/priority.test.ts`
- Create: `src/__tests__/keyboard.test.ts`

- [ ] **Step 1: Create priority utils + test** (`src/utils/priority.ts`)

```typescript
import type { Priority } from '../types';

const priorityOrder: Record<Priority, number> = {
  high: 0,
  medium: 1,
  low: 2,
};

export function sortByPriority(a: { priority: Priority }, b: { priority: Priority }): number {
  return priorityOrder[a.priority] - priorityOrder[b.priority];
}

export function isPriority(value: string): value is Priority {
  return value === 'high' || value === 'medium' || value === 'low';
}
```

Test (`src/__tests__/priority.test.ts`):
```typescript
import { describe, it, expect } from 'vitest';
import { sortByPriority, isPriority } from '../utils/priority';

describe('sortByPriority', () => {
  it('sorts high before medium before low', () => {
    const items = [
      { priority: 'low' as const },
      { priority: 'high' as const },
      { priority: 'medium' as const },
    ];
    const sorted = [...items].sort(sortByPriority);
    expect(sorted[0].priority).toBe('high');
    expect(sorted[1].priority).toBe('medium');
    expect(sorted[2].priority).toBe('low');
  });
});

describe('isPriority', () => {
  it('returns true for valid priorities', () => {
    expect(isPriority('high')).toBe(true);
    expect(isPriority('medium')).toBe(true);
    expect(isPriority('low')).toBe(true);
  });

  it('returns false for invalid values', () => {
    expect(isPriority('urgent')).toBe(false);
    expect(isPriority('')).toBe(false);
  });
});
```

- [ ] **Step 2: Create keyboard utils + test** (`src/utils/keyboard.ts`)

```typescript
export function isInputEvent(event: KeyboardEvent): boolean {
  const target = event.target as HTMLElement;
  return (
    target.tagName === 'INPUT' ||
    target.tagName === 'TEXTAREA' ||
    target.isContentEditable
  );
}

export function shortcutsEnabled(event: KeyboardEvent): boolean {
  return !isInputEvent(event);
}
```

Test (`src/__tests__/keyboard.test.ts`):
```typescript
import { describe, it, expect } from 'vitest';
import { isInputEvent } from '../utils/keyboard';

describe('isInputEvent', () => {
  it('returns true for input elements', () => {
    const input = document.createElement('input');
    document.body.appendChild(input);
    const event = new KeyboardEvent('keydown', {});
    Object.defineProperty(event, 'target', { value: input });
    expect(isInputEvent(event)).toBe(true);
    document.body.removeChild(input);
  });

  it('returns false for non-input elements', () => {
    const div = document.createElement('div');
    document.body.appendChild(div);
    const event = new KeyboardEvent('keydown', {});
    Object.defineProperty(event, 'target', { value: div });
    expect(isInputEvent(event)).toBe(false);
    document.body.removeChild(div);
  });
});
```

- [ ] **Step 3: Create time utils** (`src/utils/time.ts`)

```typescript
export function formatRelativeTime(date: string): string {
  const now = new Date();
  const target = new Date(date);
  const diffMs = target.getTime() - now.getTime();
  const diffMin = Math.round(diffMs / 60000);

  if (diffMin < 0) return 'Overdue';
  if (diffMin < 60) return `In ${diffMin} min`;
  if (diffMin < 1440) return `In ${Math.round(diffMin / 60)} hours`;

  const diffDays = Math.round(diffMin / 1440);
  if (diffDays === 1) return 'Tomorrow';
  if (diffDays < 7) return `In ${diffDays} days`;

  return target.toLocaleDateString();
}

export function formatTimeShort(date: string): string {
  const now = new Date();
  const target = new Date(date);
  const diffMs = target.getTime() - now.getTime();
  const diffMin = Math.round(diffMs / 60000);

  if (diffMin < 0) return 'Due today';
  if (diffMin < 60) return `In ${diffMin} min`;

  const hours = target.getHours();
  const mins = target.getMinutes().toString().padStart(2, '0');
  const ampm = hours >= 12 ? 'PM' : 'AM';
  const h = hours % 12 || 12;
  return `${h}:${mins} ${ampm}`;
}
```

- [ ] **Step 4: Create label constants** (`src/utils/labels.ts`)

```typescript
export const BUILTIN_LABELS = [
  { id: 'work', name: 'Work', color: '#6366F1', bgColor: '#EEF2FF' },
  { id: 'urgent', name: 'Urgent', color: '#DC2626', bgColor: '#FEF2F2' },
  { id: 'meeting', name: 'Meeting', color: '#D97706', bgColor: '#FFFBEB' },
  { id: 'personal', name: 'Personal', color: '#16A34A', bgColor: '#F0FDF4' },
  { id: 'health', name: 'Health', color: '#CA8A04', bgColor: '#FFFBEB' },
  { id: 'finance', name: 'Finance', color: '#0284C7', bgColor: '#F0F9FF' },
];
```

- [ ] **Step 5: Run tests**

```bash
bun test src/__tests__/priority.test.ts src/__tests__/keyboard.test.ts -run
```

Expected: ALL PASS

- [ ] **Step 6: Commit**

```bash
git add src/utils/*.ts src/__tests__/priority.test.ts src/__tests__/keyboard.test.ts
git commit -m "utils: priority sort, keyboard guards, time formatting, label constants"
```

---

### Task 5: Core UI Shell (App + Header + Board)

**Files:**
- Create: `src/main.tsx`
- Create: `src/App.tsx`
- Create: `src/components/Header.tsx`
- Create: `src/components/Board.tsx`
- Create: `src/components/Card.tsx`
- Create: `src/components/EmptyState.tsx`

- [ ] **Step 1: Create entry point** (`src/main.tsx`)

```typescript
import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App';
import './styles/globals.css';

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
```

- [ ] **Step 2: Create App shell** (`src/App.tsx`)

```typescript
// NOTE: Hook created in Task 6. Comment out this import until then.
// import { useBoardKeyboardShortcuts } from './hooks/use-keyboard';
import Header from './components/Header';
import Board from './components/Board';

function App() {
  // NOTE: Uncomment after Task 6
  // useBoardKeyboardShortcuts();

  return (
    <div className="h-screen flex flex-col overflow-hidden">
      <Header />
      <main className="flex-1 overflow-auto px-6 py-20">
        <Board />
      </main>
    </div>
  );
}

export default App;
```

- [ ] **Step 3: Create Header** (`src/components/Header.tsx`)

```typescript
import { useStore } from '../store/use-store';

export default function Header() {
  const { filter, setFilter, toggleLabelsPanel } = useStore();

  const handleSearch = (e: React.ChangeEvent<HTMLInputElement>) => {
    setFilter({ search: e.target.value });
  };

  return (
    <header className="fixed top-0 left-0 right-0 z-50 bg-bg/85 backdrop-blur-lg border-b border-border">
      <div className="flex items-center gap-4 px-6 py-3">
        <h1 className="text-lg font-bold tracking-tight">actio</h1>
        <div className="relative flex-1 max-w-xs">
          <svg
            className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-text-tertiary"
            xmlns="http://www.w3.org/2000/svg"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
          >
            <circle cx="11" cy="11" r="7" />
            <path d="m21 21-4.3-4.3" />
          </svg>
          <input
            type="text"
            placeholder="Search reminders..."
            value={filter.search}
            onChange={handleSearch}
            className="w-full border border-border rounded-md pl-9 pr-3 py-2 text-sm
              focus:border-accent focus:outline-none focus:ring-2 focus:ring-accent/10
              bg-surface"
          />
        </div>
        <div className="flex items-center gap-3 ml-auto">
          <button
            onClick={toggleLabelsPanel}
            className="px-3.5 py-1.5 text-sm font-medium border border-border rounded-md
              text-text-secondary hover:border-accent hover:text-accent transition-colors"
          >
            Labels
          </button>
          <div className="w-8 h-8 rounded-full bg-accent/10 flex items-center justify-center
            text-sm font-semibold text-accent cursor-default">
            JD
          </div>
        </div>
      </div>
    </header>
  );
}
```

- [ ] **Step 4: Create EmptyState** (`src/components/EmptyState.tsx`)

```typescript
export default function EmptyState() {
  return (
    <div className="flex flex-col items-center justify-center h-[60vh]">
      <div className="flex items-center gap-2 text-text-tertiary">
        <span className="relative flex h-2.5 w-2.5">
          <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-accent opacity-75"></span>
          <span className="relative inline-flex rounded-full h-2.5 w-2.5 bg-accent"></span>
        </span>
        <span className="text-sm">All clear. I'm listening.</span>
      </div>
    </div>
  );
}
```

- [ ] **Step 5: Create Board** (`src/components/Board.tsx`)

```typescript
import { useStore } from '../store/use-store';
import { sortByPriority } from '../utils/priority';
import Card from './Card';
import EmptyState from './EmptyState';

export default function Board() {
  const reminders = useStore((s) => {
    const { reminders, filter } = s;
    return reminders.filter((r) => {
      if (filter.priority && r.priority !== filter.priority) return false;
      if (filter.label && !r.labels.includes(filter.label)) return false;
      if (filter.search) {
        const q = filter.search.toLowerCase();
        if (!r.title.toLowerCase().includes(q) && !r.description.toLowerCase().includes(q)) return false;
      }
      return true;
    });
  });
  const sorted = [...reminders].sort(sortByPriority);

  if (sorted.length === 0) {
    return <EmptyState />;
  }

  return (
    <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4 max-w-6xl">
      {sorted.map((reminder) => (
        <Card key={reminder.id} reminder={reminder} />
      ))}
    </div>
  );
}
```

- [ ] **Step 6: Create base Card** (`src/components/Card.tsx`)

```typescript
import { useStore } from '../store/use-store';
import type { Reminder } from '../types';
import { formatTimeShort } from '../utils/time';
import { BUILTIN_LABELS } from '../utils/labels';

interface CardProps {
  reminder: Reminder;
}

export default function Card({ reminder }: CardProps) {
  const { setExpandedCard, expandedCardId } = useStore();
  const isExpanded = expandedCardId === reminder.id;

  const priorityColors: Record<string, { bg: string; text: string }> = {
    high: { bg: 'bg-priority-high-bg', text: 'text-priority-high-text' },
    medium: { bg: 'bg-priority-med-bg', text: 'text-priority-med-text' },
    low: { bg: 'bg-priority-low-bg', text: 'text-priority-low-text' },
  };

  const colors = priorityColors[reminder.priority];

  return (
    <div
      onClick={() => setExpandedCard(isExpanded ? null : reminder.id)}
      className="bg-surface border border-border rounded-card p-4 shadow-card-sm
        cursor-pointer transition-shadow hover:shadow-card-md hover:-translate-y-0.5"
    >
      <span className={`inline-block text-xs font-medium px-1.5 py-0.5 rounded mb-2.5 uppercase tracking-wider ${colors.bg} ${colors.text}`}>
        {reminder.priority}
      </span>
      <h3 className="text-[15px] font-semibold leading-tight mb-1.5 tracking-tight">
        {reminder.title}
      </h3>
      <p className="text-sm text-text-secondary leading-relaxed mb-3">
        {reminder.description}
      </p>
      {reminder.dueTime && (
        <p className="text-xs text-text-tertiary">
          {formatTimeShort(reminder.dueTime)}
        </p>
      )}
      {reminder.labels.length > 0 && (
        <div className="flex flex-wrap gap-2 mt-3">
          {reminder.labels.map((labelId) => {
            const label = BUILTIN_LABELS.find((l) => l.id === labelId);
            if (!label) return null;
            return (
              <button
                key={labelId}
                className="text-xs font-medium px-2 py-0.5 rounded cursor-pointer hover:ring-1 hover:ring-accent/30"
                style={{ background: label.bgColor, color: label.color }}
                onClick={(e) => {
                  e.stopPropagation();
                  const { setFilter } = useStore.getState();
                  setFilter({ label: labelId });
                }}
              >
                {label.name}
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}
```

- [ ] **Step 7: Verify the app builds**

```bash
bun run build
```

Expected: Build succeeds, no type errors

- [ ] **Step 8: Commit**

```bash
git add src/main.tsx src/App.tsx src/components/Header.tsx src/components/Board.tsx src/components/Card.tsx src/components/EmptyState.tsx
git commit -m "ui: header, board grid, base card, and empty state"
```

---

### Task 6: Keyboard Shortcut Hook

**Files:**
- Create: `src/hooks/use-keyboard.ts`

- [ ] **Step 1: Create the hook** (`src/hooks/use-keyboard.ts`)

```typescript
import { useEffect, useState } from 'react';
import { useStore } from '../store/use-store';
import { shortcutsEnabled } from '../utils/keyboard';

export function useBoardKeyboardShortcuts() {
  const { setNewReminderBar, setExpandedCard, expandedCardId } = useStore();
  const [focusIndex, setFocusIndex] = useState(0);

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (!shortcutsEnabled(e)) return;

      // Cmd+K: focus search
      if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
        e.preventDefault();
        document.querySelector<HTMLInputElement>('input[type="text"]')?.focus();
        return;
      }

      // Cmd+N: new reminder
      if ((e.metaKey || e.ctrlKey) && e.key === 'n') {
        e.preventDefault();
        setNewReminderBar(true);
        return;
      }

      // Esc: close anything open
      if (e.key === 'Escape') {
        const { ui, setExpandedCard, toggleLabelsPanel, setNewReminderBar } = useStore.getState();
        if (ui.showNewReminderBar) {
          setNewReminderBar(false);
        } else if (ui.showLabelsPanel) {
          toggleLabelsPanel();
        } else if (ui.expandedCardId) {
          setExpandedCard(null);
        }
        return;
      }

      // Arrow keys: navigate between cards
      if (e.key === 'ArrowRight' || e.key === 'ArrowDown') {
        e.preventDefault();
        const reminders = useStore.getState().getFilteredReminders();
        const next = Math.min(focusIndex + 1, reminders.length - 1);
        setFocusIndex(next);
        return;
      }
      if (e.key === 'ArrowLeft' || e.key === 'ArrowUp') {
        e.preventDefault();
        const prev = Math.max(focusIndex - 1, 0);
        setFocusIndex(prev);
        return;
      }

      // Enter: toggle card detail for focused card
      if (e.key === 'Enter') {
        const reminders = useStore.getState().getFilteredReminders();
        const target = reminders[focusIndex];
        if (target) {
          setExpandedCard(expandedCardId === target.id ? null : target.id);
        }
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [expandedCardId, focusIndex, setNewReminderBar, setExpandedCard]);
}
```

- [ ] **Step 2: Commit**

```bash
git add src/hooks/use-keyboard.ts
git commit -m "hooks: keyboard shortcuts (Cmd+K, Cmd+N, Esc, Enter)"
```

---

### Task 7: Onboarding Card

**Files:**
- Create: `src/components/OnboardingCard.tsx`

- [ ] **Step 1: Create OnboardingCard** (`src/components/OnboardingCard.tsx`)

```typescript
import { useEffect, useState } from 'react';
import { useStore } from '../store/use-store';

export default function OnboardingCard() {
  const { ui, setHasSeenOnboarding } = useStore();
  const [visible, setVisible] = useState(!ui.hasSeenOnboarding);

  useEffect(() => {
    if (!visible) {
      setHasSeenOnboarding(true);
      return;
    }
    const timer = setTimeout(() => setVisible(false), 5000);
    return () => clearTimeout(timer);
  }, [visible, setHasSeenOnboarding]);

  if (!visible) return null;

  return (
    <div
      className="fixed inset-0 flex items-center justify-center z-40 pointer-events-none"
      style={{ animation: 'fadeInOut 5s ease-in-out' }}
    >
      <div className="bg-surface/90 backdrop-blur border border-border rounded-card p-5 shadow-card-md max-w-xs text-center">
        <p className="text-sm text-text-secondary">
          Start talking. I'll catch what matters.
        </p>
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Commit**

```bash
git add src/components/OnboardingCard.tsx
git commit -m "ui: first-launch onboarding card"
```

---

### Task 8: Labels Panel

**Files:**
- Create: `src/components/LabelsPanel.tsx`

- [ ] **Step 1: Create the panel** (`src/components/LabelsPanel.tsx`)

```typescript
import { useStore } from '../store/use-store';
import { BUILTIN_LABELS } from '../utils/labels';

export default function LabelsPanel() {
  const { ui, toggleLabelsPanel, setFilter, filter, clearFilter } = useStore();

  if (!ui.showLabelsPanel) return null;

  return (
    <>
      {/* Backdrop */}
      <div
        className="fixed inset-0 bg-black/20 z-40"
        onClick={toggleLabelsPanel}
      />
      {/* Panel */}
      <div
        className="fixed right-0 top-0 bottom-0 w-96 bg-surface border-l border-border z-50 p-6 pt-16
          shadow-card-lg"
        style={{ animation: 'slideInRight 0.3s ease-out' }}
      >
        <h2 className="text-base font-semibold tracking-tight mb-5">Labels</h2>
        <div className="space-y-0">
          {BUILTIN_LABELS.map((label) => {
            const isActive = filter.label === label.id;
            return (
              <button
                key={label.id}
                onClick={() => {
                  if (isActive) {
                    clearFilter();
                  } else {
                    setFilter({ label: label.id });
                  }
                }}
                className={`flex items-center justify-between w-full py-3 border-b border-border
                  transition-colors ${isActive ? 'bg-accent/5' : 'hover:bg-bg'}`}
              >
                <div className="flex items-center gap-2">
                  <span
                    className="w-2 h-2 rounded-full"
                    style={{ background: label.color }}
                  />
                  <span className="text-sm font-medium">{label.name}</span>
                </div>
                <span className="text-xs text-text-tertiary">{label.count}</span>
              </button>
            );
          })}
        </div>
        <button
          className="mt-5 w-full py-2 border border-dashed border-border rounded-md
            text-text-tertiary text-sm hover:border-accent hover:text-accent transition-colors"
        >
          + New label
        </button>
      </div>
    </>
  );
}
```

- [ ] **Step 2: Commit**

```bash
git add src/components/LabelsPanel.tsx
git commit -m "ui: labels panel with filter support"
```

---

### Task 9: Card Detail Drawer

**Files:**
- Modify: `src/components/Card.tsx` — add detail section
- Create: `src/components/CardDetail.tsx`

- [ ] **Step 1: Create CardDetail** (`src/components/CardDetail.tsx`)

```typescript
import { useStore } from '../store/use-store';
import type { Reminder, Priority } from '../types';
import { BUILTIN_LABELS } from '../utils/labels';

interface CardDetailProps {
  reminder: Reminder;
}

export default function CardDetail({ reminder }: CardDetailProps) {
  const { markDone, setExpandedCard } = useStore();

  return (
    <div className="mt-3 pt-3 border-t border-border">
      {reminder.transcript && (
        <div className="mb-3">
          <p className="text-xs font-medium text-text-tertiary mb-1.5">Transcript</p>
          <p className="text-sm text-text-secondary leading-relaxed">
            {reminder.transcript}
          </p>
        </div>
      )}
      {reminder.context && (
        <p className="mb-3 text-xs text-text-tertiary italic">
          {reminder.context}
        </p>
      )}

      {/* Priority override */}
      <div className="mb-3">
        <p className="text-xs font-medium text-text-tertiary mb-1.5">Priority</p>
        <div className="flex gap-2">
          {(['high', 'medium', 'low'] as Priority[]).map((p) => (
            <button key={p} className={`px-2 py-1 text-xs font-medium rounded ${
              p === reminder.priority
                ? p === 'high' ? 'bg-priority-high-text text-white'
                : p === 'medium' ? 'bg-priority-med-text text-white'
                : 'bg-priority-low-text text-white'
                : 'bg-bg text-text-secondary hover:bg-border'
            }`}>
              {p}
            </button>
          ))}
        </div>
      </div>

      {/* Label editor */}
      <div>
        <p className="text-xs font-medium text-text-tertiary mb-1.5">Labels</p>
        <div className="flex flex-wrap gap-1.5">
          {BUILTIN_LABELS.map((label) => {
            const isActive = reminder.labels.includes(label.id);
            return (
              <button
                key={label.id}
                className={`px-2 py-0.5 text-xs font-medium rounded ${
                  isActive ? '' : 'opacity-50'
                }`}
                style={{ background: label.bgColor, color: label.color }}
              >
                {label.name}
              </button>
            );
          })}
        </div>
      </div>

      <div className="flex gap-2 mt-3">
        <button
          onClick={(e) => {
            e.stopPropagation();
            markDone(reminder.id);
            setExpandedCard(null);
          }}
          className="px-3 py-1.5 text-xs font-medium bg-accent text-white rounded-md
            hover:opacity-90 transition-opacity"
        >
          Done
        </button>
        <button
          onClick={(e) => {
            e.stopPropagation();
            setExpandedCard(null);
          }}
          className="px-3 py-1.5 text-xs font-medium text-text-secondary border border-border
            rounded-md hover:border-accent hover:text-accent transition-colors"
        >
          Close
        </button>
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Update Card to include detail** — modify `Card.tsx` to render CardDetail when expanded:

```typescript
// After the card body, before closing div:
{isExpanded && <CardDetail reminder={reminder} />}
```

- [ ] **Step 3: Commit**

```bash
git add src/components/CardDetail.tsx src/components/Card.tsx
git commit -m "ui: card detail drawer with transcript and done action"
```

---

### Task 10: The Wow Moment — Card Appearance Animation

**Files:**
- Create: `src/components/CardAppearance.tsx`
- Modify: `src/components/Board.tsx` — wrap cards with animation

- [ ] **Step 1: Create the animation wrapper** (`src/components/CardAppearance.tsx`)

```typescript
import { motion } from 'framer-motion';

interface CardAppearanceProps {
  children: React.ReactNode;
  isNew?: boolean;
}

export const paperFoldVariants = {
  hidden: {
    opacity: 0,
    rotateX: -15,
    x: 24,           // slide from the right
    y: 24,           // slide from the bottom
    scale: 0.95,
    transformOrigin: 'bottom right',
  },
  visible: {
    opacity: 1,
    rotateX: 0,
    x: 0,
    y: 0,
    scale: 1,
    transition: {
      duration: 0.4,
      ease: [0.22, 1, 0.36, 1] as const, // custom easing
    },
  },
};

export const glowVariants = {
  glow: {
    boxShadow: [
      '0 0 0 0px rgba(99, 102, 241, 0)',
      '0 0 0 3px rgba(99, 102, 241, 0.3)',
      '0 0 0 0px rgba(99, 102, 241, 0)',
    ],
    transition: {
      duration: 2,
      times: [0, 0.3, 1],
    },
  },
};

export default function CardAppearance({ children, isNew }: CardAppearanceProps) {
  return (
    <motion.div
      variants={paperFoldVariants}
      initial={isNew ? 'hidden' : 'visible'}
      animate="visible"
      {...(isNew ? { whileInView: { boxShadow: glowVariants.glow } } : {})}
    >
      {children}
    </motion.div>
  );
}
```

- [ ] **Step 2: Update Board to use the wrapper**

In `Board.tsx`, wrap each card:

```typescript
import CardAppearance from './CardAppearance';

// Inside the map:
<CardAppearance key={reminder.id} reminderId={reminder.id} isNew={reminder.isNew}>
  <Card reminder={reminder} />
</CardAppearance>
```

- [ ] **Step 3: Replace CardAppearance with final version** — Replace the entire `src/components/CardAppearance.tsx` with this version that includes `clearNewFlag` and `layout={false}`:

```typescript
import { useEffect } from 'react';
import { motion } from 'framer-motion';
import { useStore } from '../store/use-store';
import { paperFoldVariants } from './CardAppearance';

interface CardAppearanceProps {
  reminderId: string;
  children: React.ReactNode;
  isNew?: boolean;
}

export default function CardAppearance({ reminderId, isNew, children }: CardAppearanceProps) {
  const clearNewFlag = useStore((s) => s.clearNewFlag);

  useEffect(() => {
    if (!isNew) return;
    const timer = setTimeout(() => clearNewFlag(reminderId), 2000);
    return () => clearTimeout(timer);
  }, [isNew, reminderId, clearNewFlag]);

  return (
    <motion.div
      variants={paperFoldVariants}
      initial={isNew ? 'hidden' : 'visible'}
      animate="visible"
    >
      {children}
    </motion.div>
  );
}
```

Add `clearNewFlag` action to the store (`use-store.ts`):

```typescript
clearNewFlag: (id: string) =>
  set((state) => ({
    reminders: state.reminders.map((r) =>
      r.id === id ? { ...r, isNew: false } : r
    ),
  })),
```

**IMPORTANT:** The Board uses `SwipeDismiss` as the outermost wrapper. Since both `SwipeDismiss` and `CardAppearance` use `motion.div`, add `layout={false}` to the inner CardAppearance motion.div to prevent Framer Motion from trying to reconcile transforms across the two nested motion components:

```typescript
export default function CardAppearance({ reminderId, isNew, children }: CardAppearanceProps) {
  // ... same as above, but:
  return (
    <motion.div
      variants={paperFoldVariants}
      initial={isNew ? 'hidden' : 'visible'}
      animate="visible"
      layout={false}  /* Prevents conflict with parent SwipeDismiss drag */
    >
      {children}
    </motion.div>
  );
}
```

- [ ] **Step 4: Add Framer Motion to App** — ensure `framer-motion` is properly integrated. Add `MotionConfig` at the App level if needed.

- [ ] **Step 5: Test visually** — Run `bun run dev` and simulate a new reminder being added. The card should fold in from the bottom with an indigo glow.

- [ ] **Step 6: Commit**

```bash
git add src/components/CardAppearance.tsx src/components/Board.tsx
git commit -m "animation: card-appear paper-fold with indigo glow (wow moment)"
```

---

### Task 11: Swipe to Dismiss

**Files:**
- Create: `src/components/SwipeDismiss.tsx`
- Modify: `src/components/Card.tsx` — wrap with swipe handler

- [ ] **Step 1: Create swipe wrapper** (`src/components/SwipeDismiss.tsx`)

```typescript
import { useState } from 'react';
import { motion, useMotionValue, useTransform } from 'framer-motion';
import { useStore } from '../store/use-store';

interface SwipeDismissProps {
  children: React.ReactNode;
  reminderId: string;
}

const SWIPE_THRESHOLD = 60;

export default function SwipeDismiss({ children, reminderId }: SwipeDismissProps) {
  const x = useMotionValue(0);
  const rotateZ = useTransform(x, [-100, 0, 100], [-8, 0, 8]);
  const opacity = useTransform(x, [-100, -60, 0, 60, 100], [0.3, 0.6, 1, 0.6, 0.3]);
  const [isDragging, setIsDragging] = useState(false);
  const { markDone } = useStore();

  return (
    <motion.div
      style={{ x, rotateZ, opacity: isDragging ? opacity : 1 }}
      drag="x"
      dragConstraints={{ left: 0, right: 0 }}
      dragElastic={0.7}
      onDragStart={() => setIsDragging(true)}
      onDragEnd={(_, info) => {
        setIsDragging(false);
        if (Math.abs(info.offset.x) > SWIPE_THRESHOLD) {
          markDone(reminderId);
        }
      }}
      className="relative cursor-grab active:cursor-grabbing"
    >
      {!isDragging && (
        <div className="absolute inset-0 flex items-center justify-between px-4 pointer-events-none">
          <span className="text-xs font-medium text-priority-high-text/50">Delete</span>
          <span className="text-xs font-medium text-priority-low-text/50">Done</span>
        </div>
      )}
      {children}
    </motion.div>
  );
}
```

- [ ] **Step 2: Wrap Card with SwipeDismiss in Board.tsx**

```typescript
import SwipeDismiss from './SwipeDismiss';

// In the map:
<SwipeDismiss key={reminder.id} reminderId={reminder.id}>
  <CardAppearance isNew={reminder.isNew}>
    <Card reminder={reminder} />
  </CardAppearance>
</SwipeDismiss>
```

- [ ] **Step 3: Test** — Run dev server, drag cards left/right. Past 60px should trigger dismiss.

- [ ] **Step 4: Commit**

```bash
git add src/components/SwipeDismiss.tsx src/components/Board.tsx
git commit -m "animation: swipe-to-dismiss with tilt and collapse"
```

---

### Task 12: New Reminder Input Bar

**Files:**
- Create: `src/components/NewReminderBar.tsx`
- Modify: `src/App.tsx` — include the bar

- [ ] **Step 1: Create the bar** (`src/components/NewReminderBar.tsx`)

```typescript
import { useState, useRef, useEffect } from 'react';
import { useStore } from '../store/use-store';
import { motion } from 'framer-motion';

export default function NewReminderBar() {
  const { ui, setNewReminderBar, addReminder } = useStore();
  const [text, setText] = useState('');
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (ui.showNewReminderBar) {
      inputRef.current?.focus();
    }
  }, [ui.showNewReminderBar]);

  if (!ui.showNewReminderBar) return null;

  const handleSubmit = () => {
    if (!text.trim()) return;
    addReminder({
      title: text.trim(),
      description: '',
      priority: 'medium',
      labels: [],
      createdAt: new Date().toISOString(),
    });
    setText('');
    setNewReminderBar(false);
  };

  return (
    <motion.div
      className="fixed bottom-0 left-0 right-0 z-50 bg-surface border-t border-border p-4"
      initial={{ y: '100%' }}
      animate={{ y: 0 }}
      exit={{ y: '100%' }}
      transition={{ duration: 0.25, ease: 'easeOut' }}
    >
      <div className="max-w-2xl mx-auto flex gap-3">
        <input
          ref={inputRef}
          type="text"
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && handleSubmit()}
          placeholder="Add a reminder..."
          className="flex-1 border border-border rounded-md px-3 py-2 text-sm
            focus:border-accent focus:outline-none focus:ring-2 focus:ring-accent/10"
        />
        <button
          onClick={handleSubmit}
          className="px-4 py-2 bg-accent text-white text-sm font-medium rounded-md
            hover:opacity-90 transition-opacity"
        >
          Add
        </button>
        <button
          onClick={() => {
            setNewReminderBar(false);
            setText('');
          }}
          className="px-3 py-2 text-sm text-text-tertiary hover:text-text-secondary"
        >
          Cancel
        </button>
      </div>
    </motion.div>
  );
}
```

- [ ] **Step 2: Add to App.tsx**

```typescript
import NewReminderBar from './components/NewReminderBar';
import { AnimatePresence } from 'framer-motion';

// Inside App return, after Board:
<AnimatePresence>
  <NewReminderBar />
</AnimatePresence>
```

- [ ] **Step 3: Commit**

```bash
git add src/components/NewReminderBar.tsx src/App.tsx
git commit -m "ui: new reminder input bar with slide-up animation"
```

---

### Task 13: Standby Tray Widget

**Files:**
- Create: `src/components/StandbyTray.tsx`

- [ ] **Step 1: Create the tray widget** (`src/components/StandbyTray.tsx`)

```typescript
import { useState } from 'react';
import { useStore } from '../store/use-store';
import { sortByPriority } from '../utils/priority';
import { motion, AnimatePresence } from 'framer-motion';

type TrayState = 'collapsed' | 'hover' | 'expanded';

export default function StandbyTray() {
  const [state, setState] = useState<TrayState>('collapsed');
  const reminders = useStore((s) => s.getFilteredReminders());
  const sorted = [...reminders].sort(sortByPriority);
  const top3 = sorted.slice(0, 3);

  const dotSpeed = state === 'collapsed' ? 'animate-pulse' : 'animate-ping';

  return (
    <div
      className="fixed bottom-4 right-4 z-50"
      onMouseEnter={() => setState(state === 'collapsed' ? 'hover' : state)}
      onMouseLeave={() => setState('collapsed')}
      onClick={() => setState(state === 'expanded' ? 'collapsed' : 'expanded')}
      style={{ width: state === 'expanded' ? '340px' : state === 'hover' ? '260px' : '50px' }}
    >
      <div className="bg-surface/95 backdrop-blur rounded-card shadow-card-md overflow-hidden
        transition-all duration-300">
        {/* Header - always visible */}
        <div className="flex items-center justify-between px-3.5 py-2.5 cursor-pointer">
          <div className="flex items-center gap-1.5">
            <span className={`relative flex h-2 w-2 ${dotSpeed}`}>
              <span className="absolute inset-0 rounded-full bg-accent opacity-75"></span>
              <span className="relative rounded-full h-2 w-2 bg-accent"></span>
            </span>
            <span className="text-sm font-bold text-accent tracking-tight">actio</span>
          </div>
          {reminders.length > 0 && (
            <span className="text-xs text-text-tertiary">
              {state === 'collapsed' ? reminders.length : ''}
            </span>
          )}
        </div>

        {/* Content */}
        <AnimatePresence>
          {(state === 'hover' || state === 'expanded') && (
            <motion.div
              initial={{ height: 0, opacity: 0 }}
              animate={{ height: 'auto', opacity: 1 }}
              exit={{ height: 0, opacity: 0 }}
              transition={{ duration: 0.2 }}
              className="overflow-hidden"
            >
              {top3.map((r) => (
                <div key={r.id} className="px-3.5 py-2.5 border-t border-border
                  hover:bg-bg/50 cursor-pointer text-sm">
                  <div className="flex items-center gap-2">
                    <span className={`w-2 h-2 rounded-full ${
                      r.priority === 'high' ? 'bg-priority-high-text' :
                      r.priority === 'medium' ? 'bg-priority-med-text' :
                      'bg-priority-low-text'
                    }`} />
                    <span className="font-medium text-sm flex-1 truncate">{r.title}</span>
                    <span className="text-xs text-text-tertiary">{r.dueTime ? 'due' : ''}</span>
                  </div>
                </div>
              ))}
              {state === 'expanded' && (
                <div className="px-3.5 py-3 border-t border-border">
                  <button className="w-full py-1.5 bg-accent text-white text-xs font-medium
                    rounded-md hover:opacity-90 transition-opacity">
                    View full board
                  </button>
                </div>
              )}
            </motion.div>
          )}
        </AnimatePresence>
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Add to App.tsx** — Include `<StandbyTray />` in the App shell

- [ ] **Step 3: Commit**

```bash
git add src/components/StandbyTray.tsx src/App.tsx
git commit -m "ui: standby tray widget with 3 states (collapsed/hover/expanded)"
```

---

### Task 14: Tauri Event Bridge + Mock Data

**Files:**
- Create: `src/tauri/commands.ts`
- Create: `src/tauri/events.ts`
- Create: `src-tauri/src/mock_data.rs`
- Modify: `src-tauri/src/main.rs` — add mock data commands

- [ ] **Step 1: Create Tauri commands** (`src/tauri/commands.ts`)

```typescript
import { invoke } from '@tauri-apps/api/core';
import type { Reminder } from '../types';

export async function fetchReminders(): Promise<Reminder[]> {
  return invoke<Reminder[]>('get_reminders');
}

export async function markDoneCommand(id: string): Promise<void> {
  return invoke('mark_done', { id });
}

export async function addReminderCommand(reminder: Reminder): Promise<string> {
  return invoke<string>('add_reminder', { reminder });
}
```

- [ ] **Step 2: Create Tauri events listener** (`src/tauri/events.ts`)

```typescript
import { listen } from '@tauri-apps/api/event';
import { useStore } from '../store/use-store';
import type { Reminder } from '../types';

export async function setupTauriEvents() {
  await listen<Reminder>('new-reminder', (event) => {
    const reminder = event.payload;
    useStore.getState().setReminders([
      ...useStore.getState().reminders,
      { ...reminder, isNew: true },
    ]);

    // Clear isNew after animation
    setTimeout(() => {
      const { reminders, setReminders } = useStore.getState();
      setReminders(
        reminders.map((r) => (r.id === reminder.id ? { ...r, isNew: false } : r))
      );
    }, 2000);
  });
}
```

- [ ] **Step 3: Create mock data** (`src-tauri/src/mock_data.rs`)

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MockReminder {
    pub id: String,
    pub title: String,
    pub description: String,
    pub priority: String,
    pub labels: Vec<String>,
    pub due_time: Option<String>,
    pub transcript: Option<String>,
    pub context: Option<String>,
    pub source_time: Option<String>,
    pub is_new: bool,
    pub created_at: String,
}

pub fn seed_data() -> Vec<MockReminder> {
    vec![
        MockReminder {
            id: "1".to_string(),
            title: "Prepare Q3 report".to_string(),
            description: "Board meeting next week. Finance numbers needed by Friday.".to_string(),
            priority: "high".to_string(),
            labels: vec!["work".to_string(), "urgent".to_string()],
            due_time: Some("2026-04-04T18:00:00Z".to_string()),
            transcript: Some("You discussed this in yesterday's standup. Sarah needs marketing numbers. Tom has engineering KPIs.".to_string()),
            context: Some("Extracted from voice notes at 9:42 AM and 2:15 PM yesterday".to_string()),
            source_time: Some("2026-04-03T14:15:00Z".to_string()),
            is_new: false,
            created_at: "2026-04-03T09:42:00Z".to_string(),
        },
        MockReminder {
            id: "2".to_string(),
            title: "Call dentist".to_string(),
            description: "Follow-up scheduled last week. Office hours 9 AM - 5 PM.".to_string(),
            priority: "medium".to_string(),
            labels: vec!["health".to_string(), "personal".to_string()],
            due_time: Some("2026-04-04T17:00:00Z".to_string()),
            transcript: None,
            context: None,
            source_time: None,
            is_new: false,
            created_at: "2026-04-02T10:00:00Z".to_string(),
        },
        MockReminder {
            id: "3".to_string(),
            title: "Design review with UX team".to_string(),
            description: "30 min sync. New onboarding flow mockups.".to_string(),
            priority: "medium".to_string(),
            labels: vec!["meeting".to_string(), "work".to_string()],
            due_time: Some("2026-04-05T14:00:00Z".to_string()),
            transcript: None,
            context: None,
            source_time: None,
            is_new: false,
            created_at: "2026-04-03T11:30:00Z".to_string(),
        },
        MockReminder {
            id: "4".to_string(),
            title: "Research standing desk".to_string(),
            description: "Herman Miller recommended.".to_string(),
            priority: "low".to_string(),
            labels: vec!["personal".to_string()],
            due_time: None,
            transcript: None,
            context: None,
            source_time: None,
            is_new: false,
            created_at: "2026-04-01T15:00:00Z".to_string(),
        },
    ]
}
```

- [ ] **Step 4: Update main.rs to wire up commands**

```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod mock_data;

use mock_data::{MockReminder, seed_data};
use std::sync::Mutex;
use tauri::State;

struct AppState {
    reminders: Mutex<Vec<MockReminder>>,
}

#[tauri::command]
fn get_reminders(state: State<AppState>) -> Vec<MockReminder> {
    state.reminders.lock().unwrap().clone()
}

#[tauri::command]
fn mark_done(id: String, state: State<AppState>) {
    let mut reminders = state.reminders.lock().unwrap();
    reminders.retain(|r| r.id != id);
}

#[tauri::command]
fn add_reminder(reminder: MockReminder, state: State<AppState>) -> String {
    let id = reminder.id.clone();
    state.reminders.lock().unwrap().push(reminder);
    id
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .manage(AppState {
            reminders: Mutex::new(seed_data()),
        })
        .invoke_handler(tauri::generate_handler![
            get_reminders,
            mark_done,
            add_reminder,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

Note: The `add_reminder` command accepts a full `MockReminder` struct matching the TypeScript `Reminder` interface. Tauri auto-deserializes the JSON payload.

- [ ] **Step 5: Update App.tsx to call Tauri on mount**

```typescript
import { useEffect } from 'react';
import { fetchReminders } from './tauri/commands';
import { setupTauriEvents } from './tauri/events';
import { useStore } from './store/use-store';

// In App component:
useEffect(() => {
  // Try to fetch from Tauri; fall back to empty state if not in Tauri
  fetchReminders().then((reminders) => {
    useStore.getState().setReminders(reminders);
  }).catch(() => {
    // Not in Tauri env (web dev) - use mock data or leave empty
  });
  setupTauriEvents().catch(() => {});
}, []);
```

- [ ] **Step 6: Commit**

```bash
git add src/tauri/commands.ts src/tauri/events.ts src-tauri/src/mock_data.rs src-tauri/src/main.rs src/App.tsx
git commit -m "bridge: Tauri commands, event listeners, and mock data seed"
```

---

### Task 15: System Notifications

**Files:**
- Create: `src/components/SystemNotification.tsx`
- Modify: `src/tauri/events.ts` — add notification trigger

- [ ] **Step 1: Create notification bridge** (`src/components/SystemNotification.tsx`)

```typescript
import { isPermissionGranted, requestPermission, sendNotification } from '@tauri-apps/plugin-notification';
import type { Reminder } from '../types';

export async function showNotification(reminder: Reminder) {
  let permission = await isPermissionGranted();
  if (!permission) {
    const result = await requestPermission();
    permission = result === 'granted';
  }
  if (!permission) return;

  const isHigh = reminder.priority === 'high';
  sendNotification({
    title: isHigh ? 'New high-priority item' : 'Actio Reminder',
    body: reminder.description,
  });
}

export async function showReminderNotification(reminder: Reminder) {
  let permission = await isPermissionGranted();
  if (!permission) return;

  sendNotification({
    title: 'Reminder',
    body: `${reminder.title} — ${reminder.description}`,
  });
}
```

- [ ] **Step 2: Wire up in events.ts** — When a `new-reminder` event fires, call `showNotification` if the app window is not focused.

- [ ] **Step 3: Add notification plugin to Cargo.toml and tauri.conf.json**

- [ ] **Step 4: Commit**

```bash
git add src/components/SystemNotification.tsx src/tauri/events.ts
git commit -m "desktop: system notifications for new items and reminders"
```

---

### Task 16: Polish and Integration

- [ ] **Step 1: Add custom CSS animations** to `src/styles/globals.css`:

```css
@keyframes slideInRight {
  from { transform: translateX(100%); }
  to { transform: translateX(0); }
}

@keyframes fadeInOut {
  0% { opacity: 0; }
  20% { opacity: 1; }
  80% { opacity: 1; }
  100% { opacity: 0; }
}
```

- [ ] **Step 2: Add the FAB button** — A floating `+` button in the bottom right, above the tray widget. Triggers `setNewReminderBar(true)`.

- [ ] **Step 3: Run full build and test**

```bash
bun run build
bun test -run
```

- [ ] **Step 4: Commit**

```bash
git add src/styles/globals.css src/components/Fab.tsx
git commit -m "polish: animations, FAB button, full build check"
```

---

### Task 17: Settings Panel

**Files:**
- Create: `src/components/SettingsPanel.tsx`
- Modify: `src/hooks/use-keyboard.ts` — add `Cmd+,` shortcut
- Modify: `src-tauri/tauri.conf.json` — add system tray config

- [ ] **Step 1: Add `Cmd+,` to keyboard hook** (`src/hooks/use-keyboard.ts`)

Add this case to the `handleKeyDown` function, before the arrow key section:

```typescript
// Cmd+, (comma): toggle settings
if ((e.metaKey || e.ctrlKey) && e.key === ',') {
  e.preventDefault();
  const { setSettingsOpen, ui } = useStore.getState();
  setSettingsOpen(!ui.showSettings);
  return;
}
```

- [ ] **Step 2: Add settings state to store** (`src/store/use-store.ts`)

Add `showSettings: boolean` to `UIState` (in `src/types/index.ts`):

```typescript
export type UIState = {
  showLabelsPanel: boolean;
  expandedCardId: string | null;
  showNewReminderBar: boolean;
  hasSeenOnboarding: boolean;
  showSettings: boolean;
};
```

Add toggler action and include in the Esc handler. In the Esc handler in `use-keyboard.ts`, add this *before* the `showNewReminderBar` check:

```typescript
// Before: if (ui.showNewReminderBar)
if (ui.showSettings) {
  setSettingsOpen(false);
  return;
}
```

Add:
```typescript
setSettingsOpen: (show: boolean) => set((s) => ({
  ui: { ...s.ui, showSettings: show },
})),
```

- [ ] **Step 3: Create Settings component** (`src/components/SettingsPanel.tsx`)

```typescript
import { useEffect } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { useStore } from '../store/use-store';

export default function SettingsPanel() {
  const { ui, setSettingsOpen } = useStore();

  useEffect(() => {
    if (!ui.showSettings) return;
    const handleEsc = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setSettingsOpen(false);
    };
    window.addEventListener('keydown', handleEsc);
    return () => window.removeEventListener('keydown', handleEsc);
  }, [ui.showSettings, setSettingsOpen]);

  return (
    <>
      {/* Backdrop */}
      <AnimatePresence>
        {ui.showSettings && (
          <>
            <motion.div
              className="fixed inset-0 bg-black/20 z-40"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
            />
            {/* Settings Modal */}
            <motion.div
              className="fixed top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2
                w-80 bg-surface rounded-card shadow-card-lg z-50 p-6 border border-border"
              initial={{ opacity: 0, scale: 0.95 }}
              animate={{ opacity: 1, scale: 1 }}
              exit={{ opacity: 0, scale: 0.95 }}
            >
              <h2 className="text-base font-semibold tracking-tight mb-6">Settings</h2>
              <div className="space-y-4">
                {/* Notifications toggle */}
                <div className="flex items-center justify-between">
                  <span className="text-sm font-medium">Notifications</span>
                  <button className="w-10 h-6 bg-accent rounded-full relative transition-colors">
                    <span className="absolute right-0.5 top-0.5 w-5 h-5 bg-white rounded-full shadow-sm" />
                  </button>

