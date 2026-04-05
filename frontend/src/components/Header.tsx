import { useStore } from '../store/use-store';

export function Header() {
  const search = useStore((s) => s.filter.search);
  const activeLabel = useStore((s) => s.filter.label);
  const setFilter = useStore((s) => s.setFilter);
  const toggleLabelsPanel = useStore((s) => s.toggleLabelsPanel);

  return (
    <header className="topbar">
      <div className="topbar__inner">
        <div className="brand-lockup">
          <div className="brand-mark" aria-hidden="true">
            <svg width="18" height="18" viewBox="0 0 18 18" fill="none">
              <path d="M3 9.5L7.25 13.5L15 4.5" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
            </svg>
          </div>
          <div className="brand-copy">
            <div className="brand-title">actio</div>
            <div className="brand-subtitle">Voice-captured reminders, organized for action.</div>
          </div>
        </div>

        <div className="search-shell">
          <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <circle cx="11" cy="11" r="7" />
            <path d="m21 21-4.3-4.3" />
          </svg>
          <input
            className="search-input"
            type="text"
            placeholder="Search reminders, people, or context"
            value={search}
            onChange={(e) => setFilter({ search: e.target.value })}
            aria-label="Search reminders"
          />
        </div>

        <div className="topbar-actions">
          <button
            type="button"
            onClick={toggleLabelsPanel}
            className={`pill-button${activeLabel ? ' is-active' : ''}`}
          >
            <span>{activeLabel ? 'Label active' : 'Browse labels'}</span>
            <span aria-hidden="true">+</span>
          </button>
          <div className="topbar-avatar" aria-label="Current user">
            JD
          </div>
        </div>
      </div>
    </header>
  );
}
