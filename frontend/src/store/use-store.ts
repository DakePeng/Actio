import { create } from 'zustand';
import { createActioApiClient } from '../api/actio-api';
import type {
  FilterState,
  Label,
  LabelDraft,
  Preferences,
  Priority,
  Profile,
  Reminder,
  ReminderDraft,
  ReminderPatch,
  Tab,
  UIState,
} from '../types';

interface AppState {
  reminders: Reminder[];
  labels: Label[];
  filter: FilterState;
  ui: UIState;
  profile: Profile;
  preferences: Preferences;

  loadBoard: () => Promise<void>;
  setReminders: (reminders: Reminder[]) => void;
  addReminder: (reminder: ReminderDraft) => Promise<void>;
  updateReminderInline: (id: string, patch: Partial<Pick<Reminder, 'title' | 'description' | 'dueTime'>>) => Promise<void>;
  addLabel: (label: LabelDraft) => Promise<void>;
  deleteLabel: (id: string) => Promise<void>;
  updateLabelInline: (id: string, patch: Partial<Pick<Label, 'name' | 'color' | 'bgColor'>>) => Promise<void>;
  archiveReminder: (id: string) => Promise<void>;
  restoreReminder: (id: string) => Promise<void>;
  deleteReminder: (id: string) => Promise<void>;
  setPriority: (id: string, priority: Priority) => Promise<void>;
  setLabels: (id: string, labels: string[]) => Promise<void>;
  setFilter: (filter: Partial<FilterState>) => void;
  clearFilter: () => void;
  setBoardWindow: (show: boolean) => void;
  setTrayExpanded: (expanded: boolean) => void;
  setActiveTab: (tab: Tab) => void;
  setExpandedCard: (id: string | null) => void;
  highlightCard: (id: string | null) => void;
  setNewReminderBar: (show: boolean) => void;
  setHasSeenOnboarding: (seen: boolean) => void;
  setFocusedCard: (index: number | null) => void;
  setDictating: (active: boolean) => void;
  setDictationTranscript: (text: string) => void;
  setFeedback: (
    message: string,
    tone?: 'neutral' | 'success',
    vars?: Record<string, string | number>,
  ) => void;
  clearFeedback: () => void;
  clearNewFlag: (id: string) => void;
  extractReminders: (text: string, imageDataUrls?: string[]) => Promise<void>;
  clearAiGenerated: (id: string) => void;
  setProfile: (patch: Partial<Profile>) => void;
  setPreferences: (patch: Partial<Preferences>) => void;
  reset: () => void;
}

const api = createActioApiClient();
const initialFilter: FilterState = { priority: null, label: null, search: '' };
const defaultProfile: Profile = { name: '' };
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
  focusedCardIndex: null,
  isDictating: false,
  isDictationTranscribing: false,
  dictationTranscript: '',
  feedback: null,
};

function filterReminders(reminders: Reminder[], filter: FilterState) {
  return reminders.filter((r) => {
    if (r.archivedAt !== null) return false;
    // Medium-confidence auto-extracted items live in the Needs-review queue,
    // not the main Board. The Needs-review UI filters on `r.status === 'pending'`
    // directly; here we exclude them so the Board stays clean.
    if (r.status === 'pending') return false;
    if (filter.priority && r.priority !== filter.priority) return false;
    if (filter.label && !r.labels.includes(filter.label)) return false;
    if (filter.search) {
      const q = filter.search.toLowerCase();
      if (!r.title.toLowerCase().includes(q) && !r.description.toLowerCase().includes(q)) {
        return false;
      }
    }
    return true;
  });
}

/// Selector used by the Needs-review UI. Kept here (not inlined at call sites)
/// so the exclusion rules stay in one place.
export function pendingReminders(reminders: Reminder[]) {
  return reminders.filter((r) => r.status === 'pending' && r.archivedAt === null);
}

let feedbackTimer: number | null = null;
let highlightTimer: number | null = null;

function pushFeedback(
  set: (partial: Partial<AppState> | ((state: AppState) => Partial<AppState>)) => void,
  message: string,
  tone: 'neutral' | 'success' = 'neutral',
  vars?: Record<string, string | number>,
) {
  if (feedbackTimer) window.clearTimeout(feedbackTimer);
  set((state) => ({ ui: { ...state.ui, feedback: { message, vars, tone } } }));
  feedbackTimer = window.setTimeout(() => {
    set((state) => ({ ui: { ...state.ui, feedback: null } }));
    feedbackTimer = null;
  }, 2200);
}

function upsertReminder(reminders: Reminder[], next: Reminder) {
  const existingIndex = reminders.findIndex((item) => item.id === next.id);
  if (existingIndex === -1) {
    return [next, ...reminders];
  }

  return reminders.map((item) => (item.id === next.id ? next : item));
}

function replaceLabel(labels: Label[], next: Label) {
  const existingIndex = labels.findIndex((item) => item.id === next.id);
  if (existingIndex === -1) {
    return [...labels, next].sort((a, b) => a.name.localeCompare(b.name));
  }

  return labels
    .map((item) => (item.id === next.id ? next : item))
    .sort((a, b) => a.name.localeCompare(b.name));
}

function asReminderPatch(patch: Partial<Pick<Reminder, 'title' | 'description' | 'dueTime'>>): ReminderPatch {
  return {
    title: patch.title,
    description: patch.description,
    dueTime: patch.dueTime,
  };
}

export const useStore = create<AppState>((set) => ({
  reminders: [],
  labels: [],
  filter: initialFilter,
  ui: initialUI,
  profile: loadProfile(),
  preferences: loadPreferences(),

  loadBoard: async () => {
    try {
      const [labels, reminders] = await Promise.all([api.listLabels(), api.listReminders()]);
      set({ labels, reminders });
    } catch {
      set({ labels: [], reminders: [] });
      pushFeedback(set, 'feedback.loadRemindersFailed');
    }
  },

  setReminders: (reminders) => set({ reminders }),

  addReminder: async (reminder) => {
    try {
      const created = await api.createReminder(reminder);
      set((state) => ({ reminders: upsertReminder(state.reminders, { ...created, isNew: true }) }));
      pushFeedback(set, 'feedback.reminderAdded', 'success');
    } catch {
      pushFeedback(set, 'feedback.saveReminderFailed');
    }
  },

  updateReminderInline: async (id, patch) => {
    try {
      const updated = await api.updateReminder(id, asReminderPatch(patch));
      set((state) => ({ reminders: upsertReminder(state.reminders, updated) }));
    } catch {
      pushFeedback(set, 'feedback.updateReminderFailed');
    }
  },

  addLabel: async (label) => {
    try {
      const created = await api.createLabel(label);
      set((state) => ({ labels: replaceLabel(state.labels, created) }));
      pushFeedback(set, 'feedback.labelCreated', 'success');
    } catch {
      pushFeedback(set, 'feedback.createLabelFailed');
    }
  },

  deleteLabel: async (id) => {
    try {
      await api.deleteLabel(id);
      set((state) => ({
        labels: state.labels.filter((label) => label.id !== id),
        reminders: state.reminders.map((reminder) => ({
          ...reminder,
          labels: reminder.labels.filter((labelId) => labelId !== id),
        })),
        filter: state.filter.label === id ? { ...state.filter, label: null } : state.filter,
      }));
      pushFeedback(set, 'feedback.labelDeleted', 'neutral');
    } catch {
      pushFeedback(set, 'feedback.deleteLabelFailed');
    }
  },

  updateLabelInline: async (id, patch) => {
    try {
      const updated = await api.updateLabel(id, {
        name: patch.name,
        color: patch.color,
        bgColor: patch.bgColor,
      });
      set((state) => ({ labels: replaceLabel(state.labels, updated) }));
    } catch {
      pushFeedback(set, 'feedback.updateLabelFailed');
    }
  },

  archiveReminder: async (id) => {
    try {
      const updated = await api.updateReminder(id, { status: 'archived' });
      set((state) => ({ reminders: upsertReminder(state.reminders, updated) }));
      pushFeedback(set, 'feedback.reminderArchived', 'neutral');
    } catch {
      pushFeedback(set, 'feedback.archiveReminderFailed');
    }
  },

  restoreReminder: async (id) => {
    try {
      const updated = await api.updateReminder(id, { status: 'open' });
      set((state) => ({ reminders: upsertReminder(state.reminders, updated) }));
      pushFeedback(set, 'feedback.restoredToBoard', 'success');
    } catch {
      pushFeedback(set, 'feedback.restoreReminderFailed');
    }
  },

  deleteReminder: async (id) => {
    try {
      await api.deleteReminder(id);
      set((state) => ({ reminders: state.reminders.filter((reminder) => reminder.id !== id) }));
      pushFeedback(set, 'feedback.deletedPermanently', 'neutral');
    } catch {
      pushFeedback(set, 'feedback.deleteReminderFailed');
    }
  },

  setPriority: async (id, priority) => {
    try {
      const updated = await api.updateReminder(id, { priority });
      set((state) => ({ reminders: upsertReminder(state.reminders, updated) }));
      pushFeedback(set, 'feedback.prioritySet', 'success', {
        priority, // interpolation token; rendered via i18n
      });
    } catch {
      pushFeedback(set, 'feedback.updatePriorityFailed');
    }
  },

  setLabels: async (id, labels) => {
    try {
      const updated = await api.updateReminder(id, { labels });
      set((state) => ({ reminders: upsertReminder(state.reminders, updated) }));
      pushFeedback(set, 'feedback.labelsUpdated', 'success');
    } catch {
      pushFeedback(set, 'feedback.updateLabelsFailed');
    }
  },

  setFilter: (filter) => set((state) => ({ filter: { ...state.filter, ...filter } })),

  clearFilter: () => {
    set({ filter: initialFilter });
    pushFeedback(set, 'feedback.filtersCleared', 'neutral');
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
        focusedCardIndex: null,
      },
    })),

  setExpandedCard: (id) => set((state) => ({ ui: { ...state.ui, expandedCardId: id } })),

  highlightCard: (id) => {
    if (highlightTimer) {
      window.clearTimeout(highlightTimer);
      highlightTimer = null;
    }
    set((state) => ({ ui: { ...state.ui, highlightedCardId: id } }));
    if (id) {
      highlightTimer = window.setTimeout(() => {
        set((state) => ({ ui: { ...state.ui, highlightedCardId: null } }));
        highlightTimer = null;
      }, 1600);
    }
  },

  setNewReminderBar: (show) => set((state) => ({ ui: { ...state.ui, showNewReminderBar: show } })),

  setFocusedCard: (index) => set((state) => ({
    ui: { ...state.ui, focusedCardIndex: index },
  })),

  setDictating: (active) => set((state) => ({
    ui: { ...state.ui, isDictating: active, dictationTranscript: active ? '' : state.ui.dictationTranscript },
  })),

  setDictationTranscript: (text) => set((state) => ({
    ui: { ...state.ui, dictationTranscript: text },
  })),

  setHasSeenOnboarding: (seen) => {
    localStorage.setItem('actio-onboarded', 'true');
    set((state) => ({ ui: { ...state.ui, hasSeenOnboarding: seen } }));
  },

  setFeedback: (message, tone = 'neutral', vars) => {
    pushFeedback(set, message, tone, vars);
  },

  clearFeedback: () => {
    if (feedbackTimer) {
      window.clearTimeout(feedbackTimer);
      feedbackTimer = null;
    }
    set((state) => ({ ui: { ...state.ui, feedback: null } }));
  },

  clearNewFlag: (id) =>
    set((state) => ({
      reminders: state.reminders.map((reminder) =>
        reminder.id === id ? { ...reminder, isNew: false } : reminder,
      ),
    })),

  extractReminders: async (text, imageDataUrls = []) => {
    // Insert skeleton placeholders
    const placeholderIds: string[] = [crypto.randomUUID()];
    const placeholders: Reminder[] = placeholderIds.map((id) => ({
      id,
      title: '',
      description: '',
      priority: 'medium' as Priority,
      labels: [],
      isExtracting: true,
      createdAt: new Date().toISOString(),
      archivedAt: null,
    }));
    set((state) => ({ reminders: [...placeholders, ...state.reminders] }));

    try {
      const extracted = await api.extractReminders(text, imageDataUrls);
      set((state) => ({
        reminders: [
          ...extracted.map((r) => ({ ...r, isNew: true, isAiGenerated: true })),
          ...state.reminders.filter((r) => !placeholderIds.includes(r.id)),
        ],
      }));
      if (extracted.length === 0) {
        pushFeedback(set, 'feedback.noActionItems');
      } else if (extracted.length === 1) {
        pushFeedback(set, 'feedback.extractedSingle', 'success');
      } else {
        pushFeedback(set, 'feedback.extractedMany', 'success', {
          count: extracted.length,
        });
      }
    } catch {
      // Remove placeholders on failure
      set((state) => ({
        reminders: state.reminders.filter((r) => !placeholderIds.includes(r.id)),
      }));
      pushFeedback(set, 'feedback.extractFailed');
    }
  },

  clearAiGenerated: (id) =>
    set((state) => ({
      reminders: state.reminders.map((reminder) =>
        reminder.id === id ? { ...reminder, isAiGenerated: false } : reminder,
      ),
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

  reset: () => set({ reminders: [], labels: [], filter: initialFilter, ui: initialUI, profile: loadProfile(), preferences: loadPreferences() }),
}));

export function useFilteredReminders() {
  const reminders = useStore((state) => state.reminders);
  const filter = useStore((state) => state.filter);
  return filterReminders(reminders, filter);
}
