import type {
  BackendLabelDto,
  BackendReminderDto,
  Label,
  LabelDraft,
  LabelPatch,
  Reminder,
  ReminderDraft,
  ReminderPatch,
} from '../types';
import { getApiUrl } from './backend-url';

export const DEV_TENANT_ID = '00000000-0000-0000-0000-000000000001';

function normalizeDueTime(value?: string) {
  if (!value) {
    return undefined;
  }

  const parsed = Date.parse(value);
  return Number.isNaN(parsed) ? undefined : new Date(parsed).toISOString();
}

async function request<T>(path: string, init: RequestInit = {}) {
  const response = await fetch(await getApiUrl(path), {
    ...init,
    headers: {
      'Content-Type': 'application/json',
      'x-tenant-id': DEV_TENANT_ID,
      ...(init.headers ?? {}),
    },
  });

  if (!response.ok) {
    throw new Error(`Actio API ${response.status}: ${response.statusText}`);
  }

  if (response.status === 204) {
    return undefined as T;
  }

  return (await response.json()) as T;
}

export function mapBackendReminder(dto: BackendReminderDto): Reminder {
  return {
    id: dto.id,
    title: dto.title ?? dto.description,
    description: dto.description,
    priority: dto.priority ?? 'medium',
    dueTime: dto.due_time ?? undefined,
    labels: dto.labels,
    transcript: dto.transcript_excerpt ?? undefined,
    context: dto.context ?? undefined,
    sourceTime: dto.source_time ?? undefined,
    sourceWindowId: dto.source_window_id ?? undefined,
    speakerId: dto.speaker_id ?? undefined,
    status: dto.status,
    createdAt: dto.created_at,
    archivedAt: dto.status === 'archived' ? dto.archived_at ?? dto.updated_at : null,
  };
}

export function mapBackendLabel(dto: BackendLabelDto): Label {
  return {
    id: dto.id,
    name: dto.name,
    color: dto.color,
    bgColor: dto.bg_color,
  };
}

export function createActioApiClient() {
  return {
    async listReminders(filter?: { status?: string }) {
      const qs = filter?.status ? `?status=${encodeURIComponent(filter.status)}` : '';
      const reminders = await request<BackendReminderDto[]>(`/reminders${qs}`);
      return reminders.map(mapBackendReminder);
    },
    async getReminderTrace(id: string) {
      // Provenance for the "Show context" inspector. Returns window bounds
      // and in-window transcripts with speaker attribution. Safe to call on
      // any reminder — non-windowed items come back with empty `lines`.
      return request<import('../types').ReminderTrace>(`/reminders/${id}/trace`);
    },
    async createReminder(reminder: ReminderDraft) {
      const created = await request<BackendReminderDto>('/reminders', {
        method: 'POST',
        body: JSON.stringify({
          title: reminder.title,
          description: reminder.description,
          priority: reminder.priority,
          due_time: normalizeDueTime(reminder.dueTime),
          labels: reminder.labels,
          context: reminder.context,
        }),
      });
      return mapBackendReminder(created);
    },
    async updateReminder(id: string, patch: ReminderPatch) {
      const updated = await request<BackendReminderDto>(`/reminders/${id}`, {
        method: 'PATCH',
        body: JSON.stringify({
          title: patch.title,
          description: patch.description,
          priority: patch.priority,
          due_time: normalizeDueTime(patch.dueTime),
          status: patch.status,
          labels: patch.labels,
        }),
      });
      return mapBackendReminder(updated);
    },
    async deleteReminder(id: string) {
      await request<void>(`/reminders/${id}`, { method: 'DELETE' });
    },
    async listLabels() {
      const labels = await request<BackendLabelDto[]>('/labels');
      return labels.map(mapBackendLabel);
    },
    async createLabel(label: LabelDraft) {
      const created = await request<BackendLabelDto>('/labels', {
        method: 'POST',
        body: JSON.stringify({
          name: label.name,
          color: label.color,
          bg_color: label.bgColor,
        }),
      });
      return mapBackendLabel(created);
    },
    async updateLabel(id: string, patch: LabelPatch) {
      const updated = await request<BackendLabelDto>(`/labels/${id}`, {
        method: 'PATCH',
        body: JSON.stringify({
          name: patch.name,
          color: patch.color,
          bg_color: patch.bgColor,
        }),
      });
      return mapBackendLabel(updated);
    },
    async deleteLabel(id: string) {
      await request<void>(`/labels/${id}`, { method: 'DELETE' });
    },
    async extractReminders(text: string, imageDataUrls: string[] = []) {
      const reminders = await request<BackendReminderDto[]>('/reminders/extract', {
        method: 'POST',
        body: JSON.stringify({
          text,
          images: imageDataUrls.map((url) => ({ data_url: url })),
        }),
      });
      return reminders.map(mapBackendReminder);
    },
  };
}
