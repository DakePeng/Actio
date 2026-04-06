import { create } from 'zustand';
import type { Reminder, FilterState, UIState, Label } from '../types';

interface AppState {
  reminders: Reminder[];
  customLabels: Label[];
  filter: FilterState;
  ui: UIState;

  setReminders: (reminders: Reminder[]) => void;
  addReminder: (reminder: Omit<Reminder, 'id' | 'isNew'>) => void;
  addCustomLabel: (label: Omit<Label, 'id'>) => void;
  markDone: (id: string) => void;
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

export const useStore = create<AppState>((set) => ({
  reminders: [],
  customLabels: [],
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

  addCustomLabel: (label) => {
    set((state) => ({
      customLabels: [
        ...state.customLabels,
        { ...label, id: crypto.randomUUID() },
      ],
    }));
    pushFeedback(set, 'Label created', 'success');
  },

  markDone: (id) => {
    set((state) => ({
      reminders: state.reminders.filter((r) => r.id !== id),
    }));
    pushFeedback(set, 'Reminder marked done', 'success');
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
        showLabelsPanel: show ? state.ui.showLabelsPanel : false,
        showNewReminderBar: show ? state.ui.showNewReminderBar : false,
      },
    })),

  setTrayExpanded: (expanded) =>
    set((state) => ({
      ui: { ...state.ui, trayExpanded: expanded },
    })),

  toggleLabelsPanel: () =>
    set((state) => ({
      ui: { ...state.ui, showLabelsPanel: !state.ui.showLabelsPanel },
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

  reset: () => set({ reminders: [], customLabels: [], filter: initialFilter, ui: initialUI }),
}));

// Convenience selector — call inside component, not in selector callback
export function useFilteredReminders() {
  const reminders = useStore((s) => s.reminders);
  const filter = useStore((s) => s.filter);
  return filterReminders(reminders, filter);
}
