import { useFilteredReminders, useStore } from '../store/use-store';
import { sortByPriority } from '../utils/priority';
import { getLabelById } from '../utils/labels';
import { Card } from './Card';
import { EmptyState } from './EmptyState';
import { AnimatePresence } from 'framer-motion';

export function Board() {
  const priorityOptions: Array<{ id: 'high' | 'medium' | 'low' | null; label: string }> = [
    { id: null, label: 'All priorities' },
    { id: 'high', label: 'High' },
    { id: 'medium', label: 'Medium' },
    { id: 'low', label: 'Low' },
  ];

  const filtered = useFilteredReminders();
  const reminders = useStore((s) => s.reminders);
  const filter = useStore((s) => s.filter);
  const setFilter = useStore((s) => s.setFilter);
  const clearFilter = useStore((s) => s.clearFilter);
  const setExpandedCard = useStore((s) => s.setExpandedCard);
  const clearNewFlag = useStore((s) => s.clearNewFlag);
  const setFeedback = useStore((s) => s.setFeedback);
  const expandedCardId = useStore((s) => s.ui.expandedCardId);

  const sorted = [...filtered].sort(sortByPriority);
  const now = Date.now();
  const dueToday = reminders.filter((reminder) => {
    if (!reminder.dueTime) return false;
    const due = new Date(reminder.dueTime).getTime();
    return due >= now && due - now < 24 * 60 * 60 * 1000;
  }).length;
  const highPriority = reminders.filter((reminder) => reminder.priority === 'high').length;
  const hasActiveFilters = Boolean(filter.priority || filter.label || filter.search);
  const activeLabel = filter.label ? getLabelById(filter.label) : null;
  const boardStatus =
    sorted.length === 0
      ? 'No matching notes'
      : `${sorted.length} visible`;

  return (
    <main className="board-shell">
      <section className="board-hero">
        <div className="board-hero__main">
          <div className="board-hero__eyebrow">Board overview</div>
          <div className="board-hero__title-row">
            <h1 className="board-hero__title">Today&apos;s notes</h1>
            <span className="active-pill">{boardStatus}</span>
          </div>
          <p className="board-hero__copy">
            Review what needs action first, then narrow the board only when you need to.
          </p>
        </div>

        <div className="board-hero__stats" aria-label="Board stats">
          <div className="board-stat">
            <span className="board-stat__label">High priority</span>
            <strong className="board-stat__value">{highPriority}</strong>
          </div>
          <div className="board-stat">
            <span className="board-stat__label">Due soon</span>
            <strong className="board-stat__value">{dueToday}</strong>
          </div>
          <div className="board-stat">
            <span className="board-stat__label">Total notes</span>
            <strong className="board-stat__value">{reminders.length}</strong>
          </div>
        </div>
      </section>

      <section className="board-summary">
        <div className="board-summary__cluster">
          <div className="board-summary__label">Priority</div>
          <div className="filter-group">
            {priorityOptions.map((priority) => (
              <button
                key={priority.label}
                type="button"
                className={`filter-chip${filter.priority === priority.id ? ' is-selected' : ''}`}
                onClick={() => {
                  setFilter({ priority: priority.id });
                  setFeedback(
                    priority.id ? `Showing ${priority.label.toLowerCase()} priority notes` : 'Showing all priorities',
                  );
                }}
              >
                {priority.label}
              </button>
            ))}
          </div>
        </div>

        <div className="board-summary__cluster board-summary__cluster--right">
          <div className="board-summary__scope">
            <span className="board-summary__label">Scope</span>
            <div className="active-filter-row">
              {activeLabel && <span className="active-pill">Label: {activeLabel.name}</span>}
              {filter.search && <span className="active-pill">Search: {filter.search}</span>}
              {!hasActiveFilters && <span className="board-summary__hint">Showing the full board</span>}
            </div>
          </div>
          {hasActiveFilters && (
            <button
              type="button"
              className="ghost-button board-summary__reset"
              onClick={clearFilter}
            >
              Reset filters
            </button>
          )}
        </div>
      </section>

      {sorted.length === 0 ? (
        <EmptyState />
      ) : (
        <div className="board-grid">
          <AnimatePresence mode="popLayout">
            {sorted.map((reminder) => (
              <Card
                key={reminder.id}
                reminder={reminder}
                isExpanded={reminder.id === expandedCardId}
                onToggle={() => {
                  const nextExpanded = reminder.id === expandedCardId ? null : reminder.id;
                  setExpandedCard(nextExpanded);
                  if (nextExpanded && reminder.isNew) {
                    clearNewFlag(reminder.id);
                  }
                }}
              />
            ))}
          </AnimatePresence>
        </div>
      )}
    </main>
  );
}
