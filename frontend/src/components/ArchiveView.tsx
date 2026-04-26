import { useEffect, useState } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { useStore } from '../store/use-store';
import { useVoiceStore } from '../store/use-voice-store';
import { EmptyState } from './EmptyState';
import { useLanguage, useT, type TKey } from '../i18n';

type ArchiveSection = 'tasks' | 'clips';
type ClipFilter = 'all' | 'starred';

function formatDate(iso: string, locale: string) {
  return new Date(iso).toLocaleDateString(locale, {
    month: 'short',
    day: 'numeric',
    year: 'numeric',
  });
}

function formatTimestamp(iso: string, locale: string, todayFormat: (time: string) => string): string {
  const date = new Date(iso);
  const now = new Date();
  const isToday = date.toDateString() === now.toDateString();
  const time = date.toLocaleTimeString(locale, { hour: '2-digit', minute: '2-digit' });
  if (isToday) return todayFormat(time);
  return `${date.toLocaleDateString(locale, { month: 'short', day: 'numeric' })} ${time}`;
}

const PRIORITY_COLORS: Record<'high' | 'medium' | 'low', { bg: string; text: string; labelKey: TKey }> = {
  high: { bg: '#fef2f2', text: '#b91c1c', labelKey: 'board.priority.high' },
  medium: { bg: '#fff7df', text: '#a16207', labelKey: 'board.priority.medium' },
  low: { bg: '#edf9f1', text: '#166534', labelKey: 'board.priority.low' },
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

const SECTION_TABS: { id: ArchiveSection; labelKey: TKey }[] = [
  { id: 'tasks', labelKey: 'archive.section.tasks' },
  { id: 'clips', labelKey: 'archive.section.clips' },
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
  const loadBackendClips = useVoiceStore((s) => s.loadBackendClips);

  // Pull processed clips from the always-listening batch pipeline. Re-runs
  // whenever the user opens Archive so newly-finished clips show up without
  // a page refresh. Cheap call (single SQL query) — no need to debounce.
  useEffect(() => {
    void loadBackendClips();
  }, [loadBackendClips]);
  const t = useT();
  const { lang } = useLanguage();
  const dateLocale = lang === 'zh-CN' ? 'zh-CN' : 'en-US';

  const [section, setSection] = useState<ArchiveSection>('tasks');
  const [clipFilter, setClipFilter] = useState<ClipFilter>('all');
  const [expandedClipId, setExpandedClipId] = useState<string | null>(null);

  const [selectedTaskIds, setSelectedTaskIds] = useState<Set<string>>(new Set());
  const [selectedClipIds, setSelectedClipIds] = useState<Set<string>>(new Set());

  const archived = [...reminders]
    .filter((r) => r.archivedAt !== null)
    .sort((a, b) => new Date(b.archivedAt!).getTime() - new Date(a.archivedAt!).getTime());

  const visibleClips = clipFilter === 'starred' ? segments.filter((s) => s.starred) : segments;

  // ── Tab switching ────────────────────────────────────────────────────
  const handleSectionChange = (s: ArchiveSection) => {
    setSection(s);
    setSelectedTaskIds(new Set());
    setSelectedClipIds(new Set());
  };

  // ── Task multi-select ────────────────────────────────────────────────
  const toggleTask = (id: string) =>
    setSelectedTaskIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id); else next.add(id);
      return next;
    });

  const allTasksSelected = archived.length > 0 && selectedTaskIds.size === archived.length;
  const toggleAllTasks = () =>
    setSelectedTaskIds(allTasksSelected ? new Set() : new Set(archived.map((r) => r.id)));

  const bulkRestoreTasks = () => {
    selectedTaskIds.forEach((id) => void restoreReminder(id));
    setSelectedTaskIds(new Set());
  };
  const bulkDeleteTasks = () => {
    selectedTaskIds.forEach((id) => void deleteReminder(id));
    setSelectedTaskIds(new Set());
  };

  // ── Clip multi-select ────────────────────────────────────────────────
  const toggleClip = (id: string) =>
    setSelectedClipIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id); else next.add(id);
      return next;
    });

  const allClipsSelected = visibleClips.length > 0 && selectedClipIds.size === visibleClips.length;
  const toggleAllClips = () =>
    setSelectedClipIds(allClipsSelected ? new Set() : new Set(visibleClips.map((s) => s.id)));

  const bulkStarClips = () => {
    selectedClipIds.forEach((id) => {
      const clip = segments.find((s) => s.id === id);
      if (clip && !clip.starred) starSegment(id);
    });
    setSelectedClipIds(new Set());
  };
  const bulkUnstarClips = () => {
    selectedClipIds.forEach((id) => {
      const clip = segments.find((s) => s.id === id);
      if (clip && clip.starred) unstarSegment(id);
    });
    setSelectedClipIds(new Set());
  };
  const bulkDeleteClips = () => {
    selectedClipIds.forEach((id) => deleteSegment(id));
    setSelectedClipIds(new Set());
  };

  const handleClipFilterChange = (f: ClipFilter) => {
    setClipFilter(f);
    setSelectedClipIds(new Set());
  };

  const selectedClipsAllStarred =
    selectedClipIds.size > 0 &&
    [...selectedClipIds].every((id) => segments.find((s) => s.id === id)?.starred);

  return (
    <div className="archive-view">
      {/* Section tabs with animated indicator */}
      <div
        className="archive-view__section-tabs"
        role="tablist"
        aria-label={t('archive.aria.sections')}
      >
        {SECTION_TABS.map(({ id, labelKey }) => {
          const isActive = section === id;
          return (
            <button
              key={id}
              type="button"
              role="tab"
              aria-selected={isActive}
              className={`archive-section-btn${isActive ? ' is-active' : ''}`}
              onClick={() => handleSectionChange(id)}
            >
              {t(labelKey)}
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
                title={t('archive.empty.title')}
                description={t('archive.empty.desc')}
                eyebrow={t('archive.empty.eyebrow')}
                icon={
                  <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" style={{ opacity: 0.5 }}>
                    <path d="M21 8v13H3V8M1 3h22v5H1zM10 12h4" />
                  </svg>
                }
              />
            ) : (
              <>
                {/* Floating bulk action pill (appears when anything selected) */}
                <div className="archive-bulk-bar-anchor">
                  <AnimatePresence>
                    {selectedTaskIds.size > 0 && (
                      <motion.div
                        key="tasks-bulk-bar"
                        className="archive-bulk-bar"
                        initial={{ opacity: 0, y: 24 }}
                        animate={{ opacity: 1, y: 0 }}
                        exit={{ opacity: 0, y: 24 }}
                        transition={{ duration: 0.18, ease: 'easeOut' }}
                      >
                        <span className="archive-bulk-bar__count">
                          {t('archive.selectedCount', { count: selectedTaskIds.size })}
                        </span>
                        <button type="button" className="archive-bulk-bar__select-all" onClick={toggleAllTasks}>
                          {allTasksSelected ? t('archive.deselectAll') : t('archive.selectAll')}
                        </button>
                        <div className="archive-bulk-bar__actions">
                          <button type="button" className="ghost-button" onClick={bulkRestoreTasks}>
                            {t('archive.action.restore')}
                          </button>
                          <button type="button" className="ghost-button archive-row__delete" onClick={bulkDeleteTasks}>
                            {t('archive.action.delete')}
                          </button>
                        </div>
                      </motion.div>
                    )}
                  </AnimatePresence>
                </div>

                <div className="archive-list">
                  <AnimatePresence>
                    {archived.map((reminder, i) => {
                      const colors = PRIORITY_COLORS[reminder.priority ?? 'medium'];
                      const isSelected = selectedTaskIds.has(reminder.id);
                      return (
                        <motion.div
                          key={reminder.id}
                          className={`archive-row${isSelected ? ' is-selected' : ''}`}
                          variants={listItemVariants}
                          initial="hidden"
                          animate="visible"
                          exit="exit"
                          custom={i}
                          layout
                          onClick={() => toggleTask(reminder.id)}
                        >
                          <span
                            className="card-badge"
                            style={{ background: colors.bg, color: colors.text, flexShrink: 0 }}
                          >
                            {t(colors.labelKey)}
                          </span>
                          <span className="archive-row__title">{reminder.title}</span>
                          <span className="archive-row__date">
                            {formatDate(reminder.archivedAt!, dateLocale)}
                          </span>
                          <div className="archive-row__actions">
                            <button
                              type="button"
                              className="ghost-button"
                              onClick={(e) => {
                                e.stopPropagation();
                                void restoreReminder(reminder.id);
                              }}
                            >
                              {t('archive.action.restore')}
                            </button>
                            <button
                              type="button"
                              className="ghost-button archive-row__delete"
                              onClick={(e) => {
                                e.stopPropagation();
                                void deleteReminder(reminder.id);
                              }}
                            >
                              {t('archive.action.delete')}
                            </button>
                          </div>
                        </motion.div>
                      );
                    })}
                  </AnimatePresence>
                </div>
              </>
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
            <div
              className="clips-tab__filters"
              role="group"
              aria-label={t('archive.clips.aria.filter')}
            >
              {(['all', 'starred'] as const).map((f) => (
                <button
                  key={f}
                  type="button"
                  className={`clips-filter-btn${clipFilter === f ? ' is-active' : ''}`}
                  onClick={() => handleClipFilterChange(f)}
                >
                  {f === 'all'
                    ? t('archive.clips.filter.all')
                    : t('archive.clips.filter.starred')}
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
                  ? t('archive.clips.empty.starred')
                  : t('archive.clips.empty.all')}
              </motion.p>
            ) : (
              <>
                {/* Floating bulk action pill (appears when anything selected) */}
                <div className="archive-bulk-bar-anchor">
                  <AnimatePresence>
                    {selectedClipIds.size > 0 && (
                      <motion.div
                        key="clips-bulk-bar"
                        className="archive-bulk-bar"
                        initial={{ opacity: 0, y: 24 }}
                        animate={{ opacity: 1, y: 0 }}
                        exit={{ opacity: 0, y: 24 }}
                        transition={{ duration: 0.18, ease: 'easeOut' }}
                      >
                        <span className="archive-bulk-bar__count">
                          {t('archive.selectedCount', { count: selectedClipIds.size })}
                        </span>
                        <button type="button" className="archive-bulk-bar__select-all" onClick={toggleAllClips}>
                          {allClipsSelected ? t('archive.deselectAll') : t('archive.selectAll')}
                        </button>
                        <div className="archive-bulk-bar__actions">
                          <button
                            type="button"
                            className="ghost-button"
                            onClick={selectedClipsAllStarred ? bulkUnstarClips : bulkStarClips}
                          >
                            {selectedClipsAllStarred
                              ? t('archive.action.unstar')
                              : t('archive.action.star')}
                          </button>
                          <button type="button" className="ghost-button archive-row__delete" onClick={bulkDeleteClips}>
                            {t('archive.action.delete')}
                          </button>
                        </div>
                      </motion.div>
                    )}
                  </AnimatePresence>
                </div>

                <div className="clips-tab__list">
                  <AnimatePresence>
                    {visibleClips.map((segment, i) => {
                      const isExpanded = expandedClipId === segment.id;
                      const isLong = segment.text.length > 150;
                      const isSelected = selectedClipIds.has(segment.id);
                      return (
                        <motion.div
                          key={segment.id}
                          className={`clip-card${isSelected ? ' is-selected' : ''}`}
                          variants={listItemVariants}
                          initial="hidden"
                          animate="visible"
                          exit="exit"
                          custom={i}
                          layout
                          onClick={() => toggleClip(segment.id)}
                        >
                          <div className="clip-card__header">
                            <span className="clip-card__timestamp">
                              {formatTimestamp(segment.createdAt, dateLocale, (time) =>
                                t('archive.clip.today', { time }),
                              )}
                            </span>
                            <div className="clip-card__actions">
                              <motion.button
                                type="button"
                                className={`clip-star-btn${segment.starred ? ' is-starred' : ''}`}
                                onClick={(e) => {
                                  e.stopPropagation();
                                  segment.starred ? unstarSegment(segment.id) : starSegment(segment.id);
                                }}
                                aria-label={
                                  segment.starred
                                    ? t('archive.clip.aria.unstar')
                                    : t('archive.clip.aria.star')
                                }
                                whileHover={{ scale: 1.15 }}
                                whileTap={{ scale: 0.9 }}
                              >
                                <StarIcon filled={segment.starred} />
                              </motion.button>
                              <motion.button
                                type="button"
                                className="clip-delete-btn"
                                onClick={(e) => {
                                  e.stopPropagation();
                                  deleteSegment(segment.id);
                                }}
                                aria-label={t('archive.clip.aria.delete')}
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
                              onClick={(e) => {
                                e.stopPropagation();
                                setExpandedClipId(isExpanded ? null : segment.id);
                              }}
                            >
                              {isExpanded
                                ? t('archive.clip.showLess')
                                : t('archive.clip.showMore')}
                            </button>
                          )}
                        </motion.div>
                      );
                    })}
                  </AnimatePresence>
                </div>
              </>
            )}
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
