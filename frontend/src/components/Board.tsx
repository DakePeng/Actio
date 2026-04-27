import { useRef, useEffect } from 'react';
import { useFilteredReminders, useStore } from '../store/use-store';
import { sortByPriority } from '../utils/priority';
import { Card } from './Card';
import { CardSkeleton } from './CardSkeleton';
import { EmptyState } from './EmptyState';
import { AnimatePresence } from 'framer-motion';
import { useT, type TKey } from '../i18n';
import { translateLabelName } from '../i18n/label-names';

type BoardPriority = 'high' | 'medium' | 'low';

export function Board() {
  const t = useT();
  const priorityOptions: Array<{
    id: BoardPriority;
    labelKey: TKey;
    showingKey: TKey;
    colors: { c: string; b: string };
  }> = [
    {
      id: 'high',
      labelKey: 'board.priority.high',
      showingKey: 'board.filter.showingHigh',
      colors: { c: '#b91c1c', b: '#fef2f2' },
    },
    {
      id: 'medium',
      labelKey: 'board.priority.medium',
      showingKey: 'board.filter.showingMedium',
      colors: { c: '#a16207', b: '#fff7df' },
    },
    {
      id: 'low',
      labelKey: 'board.priority.low',
      showingKey: 'board.filter.showingLow',
      colors: { c: '#166534', b: '#edf9f1' },
    },
  ];

  const filtered = useFilteredReminders();
  const filter = useStore((s) => s.filter);
  const setFilter = useStore((s) => s.setFilter);
  const clearFilter = useStore((s) => s.clearFilter);
  const labels = useStore((s) => s.labels);
  const setExpandedCard = useStore((s) => s.setExpandedCard);
  const clearNewFlag = useStore((s) => s.clearNewFlag);
  const setFeedback = useStore((s) => s.setFeedback);
  const expandedCardId = useStore((s) => s.ui.expandedCardId);
  const focusedCardIndex = useStore((s) => s.ui.focusedCardIndex);

  const focusedRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (focusedRef.current) {
      focusedRef.current.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
    }
  }, [focusedCardIndex]);

  const sorted = [...filtered].sort(sortByPriority);
  const hasActiveFilters = Boolean(filter.priority || filter.label || filter.search);

  return (
    <main
      className="board-shell"
      onClick={() => {
        if (expandedCardId) setExpandedCard(null);
      }}
    >
      <section className="board-summary">
        <div style={{ display: 'flex', flexDirection: 'column', gap: '12px', flex: 1 }}>
          {/* Priority filter row */}
          <div className="board-summary__cluster">
            <div className="board-summary__label">{t('board.filter.priority')}</div>
            <div className="filter-group">
              {priorityOptions.map((priority) => {
                const isSelected = filter.priority === priority.id;
                return (
                  <button
                    key={priority.id}
                    type="button"
                    className={`filter-chip${isSelected ? ' is-selected' : ''}`}
                    style={isSelected ? { color: priority.colors.c, background: priority.colors.b, borderColor: `${priority.colors.c}33` } : undefined}
                    onClick={() => {
                      const next = filter.priority === priority.id ? null : priority.id;
                      setFilter({ priority: next });
                      setFeedback(next ? priority.showingKey : 'board.filter.priorityCleared');
                    }}
                  >
                    {t(priority.labelKey)}
                  </button>
                );
              })}
            </div>
          </div>

          {/* Label filter row */}
          <div className="board-summary__cluster">
            <div className="board-summary__label">{t('board.filter.labels')}</div>
            <div className="filter-group" style={{ display: 'flex', alignItems: 'center', flexWrap: 'wrap', gap: '6px' }}>
              {labels.map((label) => {
                const isSelected = filter.label === label.id;
                const displayName = translateLabelName(t, label.name);
                return (
                  <button
                    key={label.id}
                    type="button"
                    className={`filter-chip${isSelected ? ' is-selected' : ''}`}
                    onClick={() => {
                      const next = filter.label === label.id ? null : label.id;
                      setFilter({ label: next });
                      if (next) {
                        setFeedback('board.filter.labelApplied', 'neutral', { name: displayName });
                      } else {
                        setFeedback('board.filter.labelCleared');
                      }
                    }}
                    style={isSelected ? { color: label.color, background: label.bgColor, borderColor: `${label.color}33` } : undefined}
                  >
                    <span style={{ display: 'inline-block', width: '7px', height: '7px', borderRadius: '50%', background: label.color, flexShrink: 0 }} />
                    {displayName}
                  </button>
                );
              })}
            </div>
          </div>
        </div>

        <div className="board-summary__cluster board-summary__cluster--right">
          {hasActiveFilters && (
            <button
              type="button"
              className="ghost-button board-summary__reset"
              onClick={clearFilter}
            >
              {t('board.filter.clear')}
            </button>
          )}
        </div>
      </section>

      {sorted.length === 0 ? (
        <EmptyState hasFilters={hasActiveFilters} />
      ) : (
        <div className="board-grid">
          <AnimatePresence mode="popLayout">
            {sorted.map((reminder, index) => {
              const isFocused = focusedCardIndex === index;
              if (reminder.isExtracting) {
                return <CardSkeleton key={reminder.id} />;
              }
              return (
                <Card
                  key={reminder.id}
                  reminder={reminder}
                  isExpanded={reminder.id === expandedCardId}
                  isFocused={isFocused}
                  focusedRef={isFocused ? focusedRef : undefined}
                  onToggle={() => {
                    const nextExpanded = reminder.id === expandedCardId ? null : reminder.id;
                    setExpandedCard(nextExpanded);
                    if (nextExpanded && reminder.isNew) {
                      clearNewFlag(reminder.id);
                    }
                  }}
                />
              );
            })}
          </AnimatePresence>
        </div>
      )}
    </main>
  );
}
