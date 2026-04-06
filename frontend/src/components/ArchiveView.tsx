import { useStore } from '../store/use-store';
import { formatTimeShort } from '../utils/time';

export function ArchiveView() {
  const archivedReminders = useStore((s) => s.archivedReminders);
  const search = useStore((s) => s.filter.search);
  const setFilter = useStore((s) => s.setFilter);
  const restoreArchived = useStore((s) => s.restoreArchived);
  const clearArchive = useStore((s) => s.clearArchive);
  const filteredArchived = archivedReminders.filter((reminder) => {
    if (!search) return true;
    const query = search.toLowerCase();
    return (
      reminder.title.toLowerCase().includes(query) ||
      reminder.description.toLowerCase().includes(query)
    );
  });

  return (
    <main className="board-shell board-shell--section">
      <section className="workspace-controls">
        <div className="workspace-controls__main">
          <div className="search-shell workspace-controls__search">
            <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <circle cx="11" cy="11" r="7" />
              <path d="m21 21-4.3-4.3" />
            </svg>
            <input
              className="search-input"
              type="text"
              placeholder="Search archive"
              value={search}
              onChange={(e) => setFilter({ search: e.target.value })}
            />
          </div>
        </div>
        <div className="workspace-controls__actions">
          {search && (
            <button type="button" className="pill-button" onClick={() => setFilter({ search: '' })}>
              Clear search
            </button>
          )}
          {archivedReminders.length > 0 && (
            <button type="button" className="secondary-button" onClick={clearArchive}>
              Clear archive
            </button>
          )}
        </div>
      </section>

      <section className="section-hero">
        <div>
          <div className="section-hero__eyebrow">Archive</div>
          <h1 className="section-hero__title">Completed items stay nearby, not in the way.</h1>
          <p className="section-hero__copy">
            Review what has already been handled, restore anything that was archived too early, or clear the log when it is no longer useful.
          </p>
        </div>
        <div className="section-hero__meta">
          <span className="active-pill">{filteredArchived.length} archived</span>
        </div>
      </section>

      {filteredArchived.length === 0 ? (
        <section className="empty-shell">
          <div className="empty-shell__inner">
            <div className="empty-shell__eyebrow">{archivedReminders.length === 0 ? 'Nothing archived' : 'No archive matches'}</div>
            <h2 className="empty-shell__title">
              {archivedReminders.length === 0 ? 'The archive is still empty.' : 'Nothing in the archive matches this search.'}
            </h2>
            <p className="empty-shell__copy">
              {archivedReminders.length === 0
                ? 'Finished reminders will land here after you mark them done from the board.'
                : 'Try a broader search or clear the query to review the full archive.'}
            </p>
          </div>
        </section>
      ) : (
        <section className="archive-list" aria-label="Archived reminders">
          {filteredArchived.map((reminder) => (
            <article key={reminder.id} className="archive-item">
              <div className="archive-item__header">
                <div>
                  <div className="archive-item__title">{reminder.title}</div>
                  <div className="archive-item__meta">
                    <span>{reminder.priority} priority</span>
                    <span>{reminder.dueTime ? formatTimeShort(reminder.dueTime) : 'No deadline'}</span>
                  </div>
                </div>
                <button type="button" className="secondary-button" onClick={() => restoreArchived(reminder.id)}>
                  Restore
                </button>
              </div>
              {reminder.description && <p className="archive-item__copy">{reminder.description}</p>}
            </article>
          ))}
        </section>
      )}
    </main>
  );
}
