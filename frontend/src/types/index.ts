export type Priority = 'high' | 'medium' | 'low';

export interface Reminder {
  id: string;
  title: string;
  description: string;
  priority: Priority;
  dueTime?: string;
  labels: string[];
  transcript?: string;
  context?: string;
  sourceTime?: string;
  isNew?: boolean;
  createdAt: string;
  archivedAt: string | null;
}

export interface Label {
  id: string;
  name: string;
  color: string;
  bgColor: string;
}

export interface FilterState {
  priority: Priority | null;
  label: string | null;
  search: string;
}

export interface UIState {
  showBoardWindow: boolean;
  trayExpanded: boolean;
  expandedCardId: string | null;
  highlightedCardId: string | null;
  showNewReminderBar: boolean;
  hasSeenOnboarding: boolean;
  feedback: {
    message: string;
    tone: 'neutral' | 'success';
  } | null;
}
