import type { Label } from '../types';

export const BUILTIN_LABELS: Label[] = [
  { id: 'work', name: 'Work', color: '#6366F1', bgColor: '#EEF2FF' },
  { id: 'urgent', name: 'Urgent', color: '#DC2626', bgColor: '#FEF2F2' },
  { id: 'meeting', name: 'Meeting', color: '#D97706', bgColor: '#FFFBEB' },
  { id: 'personal', name: 'Personal', color: '#16A34A', bgColor: '#F0FDF4' },
  { id: 'health', name: 'Health', color: '#CA8A04', bgColor: '#FFFBEB' },
  { id: 'finance', name: 'Finance', color: '#0284C7', bgColor: '#F0F9FF' },
] as const;

export function getLabelById(labels: Label[], id: string) {
  return labels.find((label) => label.id === id) ?? null;
}

export function computeLabelCounts(reminders: { labels: string[] }[]): Map<string, number> {
  const counts = new Map<string, number>();
  for (const r of reminders) {
    for (const labelId of r.labels) {
      counts.set(labelId, (counts.get(labelId) ?? 0) + 1);
    }
  }
  return counts;
}
