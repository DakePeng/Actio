import { useState } from 'react';
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
      <div className="archive-view__section-tabs" role="tablist" aria-label="Archive sections">
        <button
          type="button"
          role="tab"
          aria-selected={section === 'tasks'}
          className={`archive-section-btn${section === 'tasks' ? ' is-active' : ''}`}
          onClick={() => setSection('tasks')}
        >
          Tasks
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={section === 'clips'}
          className={`archive-section-btn${section === 'clips' ? ' is-active' : ''}`}
          onClick={() => setSection('clips')}
        >
          Clips
        </button>
      </div>

      {section === 'tasks' && (
        <>
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
          )}
        </>
      )}

      {section === 'clips' && (
        <>
          <div className="clips-tab__filters" role="group" aria-label="Filter clips">
            <button
              type="button"
              className={`clips-filter-btn${clipFilter === 'all' ? ' is-active' : ''}`}
              onClick={() => setClipFilter('all')}
            >
              All
            </button>
            <button
              type="button"
              className={`clips-filter-btn${clipFilter === 'starred' ? ' is-active' : ''}`}
              onClick={() => setClipFilter('starred')}
            >
              Starred
            </button>
          </div>

          {visibleClips.length === 0 ? (
            <p className="clips-tab__empty">
              {clipFilter === 'starred'
                ? 'No starred clips yet. Star a clip to save it permanently.'
                : 'No clips yet. Start recording to generate clips.'}
            </p>
          ) : (
            <div className="clips-tab__list">
              {visibleClips.map((segment) => {
                const isExpanded = expandedClipId === segment.id;
                const isLong = segment.text.length > 150;
                return (
                  <div key={segment.id} className="clip-card">
                    <div className="clip-card__header">
                      <span className="clip-card__timestamp">{formatTimestamp(segment.createdAt)}</span>
                      <div className="clip-card__actions">
                        <button
                          type="button"
                          className={`clip-star-btn${segment.starred ? ' is-starred' : ''}`}
                          onClick={() =>
                            segment.starred ? unstarSegment(segment.id) : starSegment(segment.id)
                          }
                          aria-label={segment.starred ? 'Unstar clip' : 'Star clip'}
                        >
                          {segment.starred ? '★' : '☆'}
                        </button>
                        <button
                          type="button"
                          className="clip-delete-btn"
                          onClick={() => deleteSegment(segment.id)}
                          aria-label="Delete clip"
                        >
                          🗑
                        </button>
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
                  </div>
                );
              })}
            </div>
          )}
        </>
      )}
    </div>
  );
}
