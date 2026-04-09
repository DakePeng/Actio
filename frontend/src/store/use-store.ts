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

// Convenience selector — call inside component, not in selector callback
export function useFilteredReminders() {
  const reminders = useStore((s) => s.reminders);
  const filter = useStore((s) => s.filter);
  return filterReminders(reminders, filter);
}
