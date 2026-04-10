import { useState } from 'react';
import { useVoiceStore } from '../store/use-voice-store';

type FilterMode = 'all' | 'starred';

function formatTimestamp(iso: string): string {
  const date = new Date(iso);
  const now = new Date();
  const isToday = date.toDateString() === now.toDateString();
  const time = date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
  if (isToday) return `Today ${time}`;
  return `${date.toLocaleDateString([], { month: 'short', day: 'numeric' })} ${time}`;
}

export function ClipsTab() {
  const segments = useVoiceStore((s) => s.segments);
  const starSegment = useVoiceStore((s) => s.starSegment);
  const unstarSegment = useVoiceStore((s) => s.unstarSegment);
  const deleteSegment = useVoiceStore((s) => s.deleteSegment);

  const [filter, setFilter] = useState<FilterMode>('all');
  const [expandedId, setExpandedId] = useState<string | null>(null);

  const visible = filter === 'starred' ? segments.filter((s) => s.starred) : segments;

  return (
    <div className="clips-tab">
      <div className="clips-tab__filters" role="group" aria-label="Filter clips">
        <button
          type="button"
          className={`clips-filter-btn${filter === 'all' ? ' is-active' : ''}`}
          onClick={() => setFilter('all')}
        >
          All
        </button>
        <button
          type="button"
          className={`clips-filter-btn${filter === 'starred' ? ' is-active' : ''}`}
          onClick={() => setFilter('starred')}
        >
          Starred
        </button>
      </div>

      {visible.length === 0 ? (
        <p className="clips-tab__empty">
          {filter === 'starred'
            ? 'No starred clips yet. Star a clip to save it permanently.'
            : 'No clips yet. Start recording to generate clips.'}
        </p>
      ) : (
        <div className="clips-tab__list">
          {visible.map((segment) => {
            const isExpanded = expandedId === segment.id;
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
                    onClick={() => setExpandedId(isExpanded ? null : segment.id)}
                  >
                    {isExpanded ? 'Show less' : 'Show more'}
                  </button>
                )}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
