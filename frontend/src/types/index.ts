export type Priority = 'high' | 'medium' | 'low';
export type ReminderStatus = 'open' | 'completed' | 'archived';

export type Tab = 'board' | 'archive' | 'settings' | 'recording' | 'clips' | 'people';

export interface Segment {
  id: string;
  sessionId: string;
  text: string;
  createdAt: string; // ISO 8601
  starred: boolean;
}

export interface Person {
  id: string;
  name: string;
  color: string; // hex, from preset swatches
  createdAt: string; // ISO 8601
}

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

export interface ReminderDraft {
  title: string;
  description: string;
  priority: Priority;
  dueTime?: string;
  labels: string[];
  transcript?: string;
  context?: string;
  sourceTime?: string;
  createdAt: string;
  archivedAt: string | null;
}

export interface ReminderPatch {
  title?: string;
  description?: string;
  priority?: Priority;
  dueTime?: string;
  labels?: string[];
  status?: ReminderStatus;
}

export interface Label {
  id: string;
  name: string;
  color: string;
  bgColor: string;
}

export interface LabelDraft {
  name: string;
  color: string;
  bgColor: string;
}

export interface LabelPatch {
  name?: string;
  color?: string;
  bgColor?: string;
}

export interface BackendReminderDto {
  id: string;
  session_id: string | null;
  tenant_id: string;
  speaker_id: string | null;
  assigned_to: string | null;
  title: string | null;
  description: string;
  status: ReminderStatus;
  priority: Priority | null;
  due_time: string | null;
  archived_at: string | null;
  transcript_excerpt: string | null;
  context: string | null;
  source_time: string | null;
  labels: string[];
  created_at: string;
  updated_at: string;
}

export interface BackendLabelDto {
  id: string;
  tenant_id: string;
  name: string;
  color: string;
  bg_color: string;
  created_at: string;
}

export interface FilterState {
  priority: Priority | null;
  label: string | null;
  search: string;
}

export interface Profile {
  name: string;
  initials: string;
}

export interface Preferences {
  theme: 'light' | 'system' | 'dark';
  launchAtLogin: boolean;
  notifications: boolean;
}

export interface UIState {
  showBoardWindow: boolean;
  trayExpanded: boolean;
  expandedCardId: string | null;
  highlightedCardId: string | null;
  showNewReminderBar: boolean;
  hasSeenOnboarding: boolean;
  activeTab: Tab;
  feedback: {
    message: string;
    tone: 'neutral' | 'success';
  } | null;
}
