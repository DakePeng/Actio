import { useStore } from '../store/use-store';
import { EmptyState } from './EmptyState';

function formatDate(iso: string) {
  return new Date(iso).toLocaleDateString('en-US', {
    month: 'short',
    day: 'numeric',
    year: 'numeric',
  });
}

const PRIORITY_COLORS = {
  high: { bg: '#fef2f2', text: '#b91c1c', label: 'High' },
  medium: { bg: '#fff7df', text: '#a16207', label: 'Medium' },
  low: { bg: '#edf9f1', text: '#166534', label: 'Low' },
};

export function ArchiveView() {
  const reminders = useStore((s) => s.reminders);
  const restoreReminder = useStore((s) => s.restoreReminder);
  const deleteReminder = useStore((s) => s.deleteReminder);

  const archived = [...reminders]
    .filter((r) => r.archivedAt !== null)
    .sort((a, b) => new Date(b.archivedAt!).getTime() - new Date(a.archivedAt!).getTime());

  if (archived.length === 0) {
    return (
      <EmptyState
        title="Archive is empty"
        description="Deleted or archived notes will appear here."
        eyebrow="Clean Slate"
        icon={
          <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" style={{ opacity: 0.5 }}>
            <path d="M21 8v13H3V8M1 3h22v5H1zM10 12h4" />
          </svg>
        }
      />
    );
  }

  return (
    <div className="archive-list">
      {archived.map((reminder) => {
        const colors = PRIORITY_COLORS[reminder.priority ?? 'medium'];
        return (
          <div key={reminder.id} className="archive-row">
            <span
              className="card-badge"
              style={{ background: colors.bg, color: colors.text, flexShrink: 0 }}
            >
              {colors.label}
            </span>
            <span className="archive-row__title">{reminder.title}</span>
            <span className="archive-row__date">{formatDate(reminder.archivedAt!)}</span>
            <div className="archive-row__actions">
              <button
                type="button"
                className="ghost-button"
                onClick={() => void restoreReminder(reminder.id)}
              >
                Restore
              </button>
              <button
                type="button"
                className="ghost-button archive-row__delete"
                onClick={() => void deleteReminder(reminder.id)}
              >
                Delete
              </button>
            </div>
          </div>
        );
      })}
    </div>
  );
}
