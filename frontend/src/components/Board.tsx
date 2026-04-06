import { useFilteredReminders, useStore } from '../store/use-store';
import { sortByPriority } from '../utils/priority';
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
  const filter = useStore((s) => s.filter);
  const setFilter = useStore((s) => s.setFilter);
  const clearFilter = useStore((s) => s.clearFilter);
  const setExpandedCard = useStore((s) => s.setExpandedCard);
  const clearNewFlag = useStore((s) => s.clearNewFlag);
  const setFeedback = useStore((s) => s.setFeedback);
  const expandedCardId = useStore((s) => s.ui.expandedCardId);

  const sorted = [...filtered].sort(sortByPriority);
  const hasActiveFilters = Boolean(filter.priority || filter.label || filter.search);

  return (
    <main className="board-shell">
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
          {hasActiveFilters && (
            <button
              type="button"
              className="ghost-button board-summary__reset"
              onClick={clearFilter}
            >
              Clear filters
            </button>
          )}
        </div>
      </section>

      {sorted.length === 0 ? (
        <EmptyState hasFilters={hasActiveFilters} />
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
