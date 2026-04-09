import { useStore } from '../store/use-store';

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
    return <div className="archive-empty"><p>Nothing archived yet.</p></div>;
  }

  return (
    <div className="archive-list">
      {archived.map((r) => {
        const colors = PRIORITY_COLORS[r.priority ?? 'medium'];
        return (
          <div key={r.id} className="archive-row">
            <span
              className="card-badge"
              style={{ background: colors.bg, color: colors.text, flexShrink: 0 }}
            >
              {colors.label}
            </span>
            <span className="archive-row__title">{r.title}</span>
            <span className="archive-row__date">{formatDate(r.archivedAt!)}</span>
            <div className="archive-row__actions">
              <button
                type="button"
                className="ghost-button"
                onClick={() => restoreReminder(r.id)}
              >
                Restore
              </button>
              <button
                type="button"
                className="ghost-button archive-row__delete"
                onClick={() => deleteReminder(r.id)}
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
