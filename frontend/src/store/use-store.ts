import { create } from 'zustand';
import type { Reminder, FilterState, UIState, Label, Priority } from '../types';

interface AppState {
  reminders: Reminder[];
  labels: Label[];
  filter: FilterState;
  ui: UIState;

  setReminders: (reminders: Reminder[]) => void;
  addReminder: (reminder: Omit<Reminder, 'id' | 'isNew'>) => void;
  updateReminder: (id: string, patch: Partial<Pick<Reminder, 'title' | 'description' | 'dueTime'>>) => void;
  addLabel: (label: Omit<Label, 'id'>) => void;
  deleteLabel: (id: string) => void;
  markDone: (id: string) => void;
  setPriority: (id: string, priority: Priority) => void;
  setLabels: (id: string, labels: string[]) => void;
  setFilter: (filter: Partial<FilterState>) => void;
  clearFilter: () => void;
  setBoardWindow: (show: boolean) => void;
  setTrayExpanded: (expanded: boolean) => void;
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
  trayExpanded: false,
  expandedCardId: null,
  highlightedCardId: null,
  showNewReminderBar: false,
  hasSeenOnboarding: localStorage.getItem('actio-onboarded') === 'true',
  feedback: null,
};

function filterReminders(reminders: Reminder[], filter: FilterState) {
  return reminders.filter((r) => {
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
  if (feedbackTimer) {
    window.clearTimeout(feedbackTimer);
  }
  set((state) => ({
    ui: { ...state.ui, feedback: { message, tone } },
  }));
  feedbackTimer = window.setTimeout(() => {
    set((state) => ({
      ui: { ...state.ui, feedback: null },
    }));
    feedbackTimer = null;
  }, 2200);
}

import { BUILTIN_LABELS } from '../utils/labels';

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
        { ...reminder, id: crypto.randomUUID(), isNew: true },
      ],
    }));
    pushFeedback(set, 'Reminder added to the board', 'success');
  },

  updateReminder: (id, patch) => {
    set((state) => ({
      reminders: state.reminders.map((r) => (r.id === id ? { ...r, ...patch } : r)),
    }));
  },

  addLabel: (label) => {
    set((state) => ({
      labels: [
        ...state.labels,
        { ...label, id: crypto.randomUUID() },
      ],
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
      filter: state.filter.label === id ? { ...state.filter, label: null } : state.filter,
    }));
    pushFeedback(set, 'Label deleted', 'neutral');
  },

  markDone: (id) => {
    set((state) => ({
      reminders: state.reminders.filter((r) => r.id !== id),
    }));
    pushFeedback(set, 'Reminder marked done', 'success');
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

  setFilter: (filter) =>
    set((state) => ({
      filter: { ...state.filter, ...filter },
    })),

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

  setTrayExpanded: (expanded) =>
    set((state) => ({
      ui: { ...state.ui, trayExpanded: expanded },
    })),

  setExpandedCard: (id) =>
    set((state) => ({
      ui: { ...state.ui, expandedCardId: id },
    })),

  highlightCard: (id) => {
    if (highlightTimer) {
      window.clearTimeout(highlightTimer);
      highlightTimer = null;
    }
    set((state) => ({
      ui: { ...state.ui, highlightedCardId: id },
    }));
    if (id) {
      highlightTimer = window.setTimeout(() => {
        set((state) => ({
          ui: { ...state.ui, highlightedCardId: null },
        }));
        highlightTimer = null;
      }, 1600);
    }
  },

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

  setFeedback: (message, tone = 'neutral') => {
    pushFeedback(set, message, tone);
  },

  clearFeedback: () => {
    if (feedbackTimer) {
      window.clearTimeout(feedbackTimer);
      feedbackTimer = null;
    }
    set((state) => ({
      ui: { ...state.ui, feedback: null },
    }));
  },

  clearNewFlag: (id) =>
    set((state) => ({
      reminders: state.reminders.map((r) =>
        r.id === id ? { ...r, isNew: false } : r,
      ),
    })),

  reset: () => set({ reminders: [], labels: [...BUILTIN_LABELS], filter: initialFilter, ui: initialUI }),
}));

// Convenience selector — call inside component, not in selector callback
export function useFilteredReminders() {
  const reminders = useStore((s) => s.reminders);
  const filter = useStore((s) => s.filter);
  return filterReminders(reminders, filter);
}
