export type Priority = 'high' | 'medium' | 'low';
// 'pending' is the "Needs review" state for medium-confidence items produced
// by the windowed extractor. User confirms → 'open', dismisses → 'archived'.
export type ReminderStatus = 'open' | 'pending' | 'completed' | 'archived';

export type Tab = 'board' | 'needs-review' | 'archive' | 'settings' | 'live' | 'people';

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
  /** ID of the `extraction_windows` row that produced this reminder. Only
   *  set for items from the windowed background extractor — manual POSTs
   *  and legacy session-end generation leave this null. Drives the "Show
   *  context" trace inspector. */
  sourceWindowId?: string;
  /** Speaker the backend inferred from the evidence quote. UI renders a
   *  speaker chip on the card when present. */
  speakerId?: string;
  /** Review-queue state for medium-confidence auto-extracted items. */
  status?: ReminderStatus;
  isNew?: boolean;
  isExtracting?: boolean;
  isAiGenerated?: boolean;
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
  source_window_id: string | null;
  labels: string[];
  created_at: string;
  updated_at: string;
}

export interface ReminderTraceLine {
  start_ms: number;
  end_ms: number;
  text: string;
  speaker_id: string | null;
  speaker_name: string | null;
}

export interface ReminderTrace {
  reminder_id: string;
  session_id: string | null;
  window_id: string | null;
  window_start_ms: number | null;
  window_end_ms: number | null;
  session_started_at: string | null;
  transcript_excerpt: string | null;
  source_time: string | null;
  lines: ReminderTraceLine[];
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
  focusedCardIndex: number | null;
  isDictating: boolean;
  isDictationTranscribing: boolean;
  dictationTranscript: string;
  feedback: {
    /** Translation key for the toast message. Raw strings are also accepted
     *  (they fall through untranslated). */
    message: string;
    /** Optional interpolation vars for the translation. */
    vars?: Record<string, string | number>;
    tone: 'neutral' | 'success';
  } | null;
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
}
