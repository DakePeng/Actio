import { useState } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { useStore } from '../store/use-store';
import { useVoiceStore } from '../store/use-voice-store';
import { EmptyState } from './EmptyState';

type ArchiveSection = 'tasks' | 'clips';
type ClipFilter = 'all' | 'starred';

function formatDate(iso: string) {
  return new Date(iso).toLocaleDateString('en-US', {
    month: 'short',
    day: 'numeric',
    year: 'numeric',
  });
}

function formatTimestamp(iso: string): string {
  const date = new Date(iso);
  const now = new Date();
  const isToday = date.toDateString() === now.toDateString();
  const time = date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
  if (isToday) return `Today ${time}`;
  return `${date.toLocaleDateString([], { month: 'short', day: 'numeric' })} ${time}`;
}

const PRIORITY_COLORS = {
  high: { bg: '#fef2f2', text: '#b91c1c', label: 'High' },
  medium: { bg: '#fff7df', text: '#a16207', label: 'Medium' },
  low: { bg: '#edf9f1', text: '#166534', label: 'Low' },
};

function StarIcon({ filled }: { filled: boolean }) {
  return filled ? (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor" stroke="currentColor" strokeWidth="1">
      <polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2" />
    </svg>
  ) : (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2" />
    </svg>
  );
}

function TrashIcon() {
  return (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <polyline points="3 6 5 6 21 6" />
      <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />
    </svg>
  );
}

const SECTION_TABS: { id: ArchiveSection; label: string }[] = [
  { id: 'tasks', label: 'Tasks' },
  { id: 'clips', label: 'Clips' },
];

const listItemVariants = {
  hidden: { opacity: 0, y: 12 },
  visible: (i: number) => ({
    opacity: 1,
    y: 0,
    transition: { delay: i * 0.04, type: 'spring' as const, stiffness: 300, damping: 24 },
  }),
  exit: { opacity: 0, scale: 0.95, transition: { duration: 0.12 } },
};

export function ArchiveView() {
  const reminders = useStore((s) => s.reminders);
  const restoreReminder = useStore((s) => s.restoreReminder);
  const deleteReminder = useStore((s) => s.deleteReminder);

  const segments = useVoiceStore((s) => s.segments);
  const starSegment = useVoiceStore((s) => s.starSegment);
  const unstarSegment = useVoiceStore((s) => s.unstarSegment);
  const deleteSegment = useVoiceStore((s) => s.deleteSegment);

  const [section, setSection] = useState<ArchiveSection>('tasks');
  const [clipFilter, setClipFilter] = useState<ClipFilter>('all');
  const [expandedClipId, setExpandedClipId] = useState<string | null>(null);

  const archived = [...reminders]
    .filter((r) => r.archivedAt !== null)
    .sort((a, b) => new Date(b.archivedAt!).getTime() - new Date(a.archivedAt!).getTime());

  const visibleClips = clipFilter === 'starred' ? segments.filter((s) => s.starred) : segments;

  return (
    <div className="archive-view">
      {/* Section tabs with animated indicator */}
      <div className="archive-view__section-tabs" role="tablist" aria-label="Archive sections">
        {SECTION_TABS.map(({ id, label }) => {
          const isActive = section === id;
          return (
            <button
              key={id}
              type="button"
              role="tab"
              aria-selected={isActive}
              className={`archive-section-btn${isActive ? ' is-active' : ''}`}
              onClick={() => setSection(id)}
            >
              {label}
              {isActive && (
                <motion.div
                  layoutId="archiveSectionIndicator"
                  className="archive-section-btn__indicator"
                  initial={false}
                  transition={{ type: 'spring', stiffness: 500, damping: 32 }}
                />
              )}
            </button>
          );
        })}
      </div>

      <AnimatePresence mode="wait">
        {section === 'tasks' && (
          <motion.div
            key="tasks"
            initial={{ opacity: 0, x: -12 }}
            animate={{ opacity: 1, x: 0 }}
            exit={{ opacity: 0, x: 12 }}
            transition={{ duration: 0.18 }}
            style={{ flex: 1, overflow: 'hidden', display: 'flex', flexDirection: 'column' }}
          >
            {archived.length === 0 ? (
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
            ) : (
              <div className="archive-list">
                <AnimatePresence>
                  {archived.map((reminder, i) => {
                    const colors = PRIORITY_COLORS[reminder.priority ?? 'medium'];
                    return (
                      <motion.div
                        key={reminder.id}
                        className="archive-row"
                        variants={listItemVariants}
                        initial="hidden"
                        animate="visible"
                        exit="exit"
                        custom={i}
                        layout
                      >
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
                      </motion.div>
                    );
                  })}
                </AnimatePresence>
              </div>
            )}
          </motion.div>
        )}

        {section === 'clips' && (
          <motion.div
            key="clips"
            initial={{ opacity: 0, x: 12 }}
            animate={{ opacity: 1, x: 0 }}
            exit={{ opacity: 0, x: -12 }}
            transition={{ duration: 0.18 }}
            style={{ flex: 1, overflow: 'hidden', display: 'flex', flexDirection: 'column' }}
          >
            <div className="clips-tab__filters" role="group" aria-label="Filter clips">
              {(['all', 'starred'] as const).map((f) => (
                <button
                  key={f}
                  type="button"
                  className={`clips-filter-btn${clipFilter === f ? ' is-active' : ''}`}
                  onClick={() => setClipFilter(f)}
                >
                  {f === 'all' ? 'All' : 'Starred'}
                </button>
              ))}
            </div>

            {visibleClips.length === 0 ? (
              <motion.p
                className="clips-tab__empty"
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                transition={{ delay: 0.1 }}
              >
                {clipFilter === 'starred'
                  ? 'No starred clips yet. Star a clip to save it permanently.'
                  : 'No clips yet. Start recording to generate clips.'}
              </motion.p>
            ) : (
              <div className="clips-tab__list">
                <AnimatePresence>
                  {visibleClips.map((segment, i) => {
                    const isExpanded = expandedClipId === segment.id;
                    const isLong = segment.text.length > 150;
                    return (
                      <motion.div
                        key={segment.id}
                        className="clip-card"
                        variants={listItemVariants}
                        initial="hidden"
                        animate="visible"
                        exit="exit"
                        custom={i}
                        layout
                      >
                        <div className="clip-card__header">
                          <span className="clip-card__timestamp">{formatTimestamp(segment.createdAt)}</span>
                          <div className="clip-card__actions">
                            <motion.button
                              type="button"
                              className={`clip-star-btn${segment.starred ? ' is-starred' : ''}`}
                              onClick={() =>
                                segment.starred ? unstarSegment(segment.id) : starSegment(segment.id)
                              }
                              aria-label={segment.starred ? 'Unstar clip' : 'Star clip'}
                              whileHover={{ scale: 1.15 }}
                              whileTap={{ scale: 0.9 }}
                            >
                              <StarIcon filled={segment.starred} />
                            </motion.button>
                            <motion.button
                              type="button"
                              className="clip-delete-btn"
                              onClick={() => deleteSegment(segment.id)}
                              aria-label="Delete clip"
                              whileHover={{ scale: 1.15 }}
                              whileTap={{ scale: 0.9 }}
                            >
                              <TrashIcon />
                            </motion.button>
                          </div>
                        </div>
                        <p className={`clip-card__text${isExpanded ? ' is-expanded' : ''}`}>
                          {segment.text}
                        </p>
                        {isLong && (
                          <button
                            type="button"
                            className="clip-expand-btn"
                            onClick={() => setExpandedClipId(isExpanded ? null : segment.id)}
                          >
                            {isExpanded ? 'Show less' : 'Show more'}
                          </button>
                        )}
                      </motion.div>
                    );
                  })}
                </AnimatePresence>
              </div>
            )}
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
