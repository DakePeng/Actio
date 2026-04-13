export function Header() {
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

        <div className="topbar-actions">
          <div className="topbar-avatar" aria-label="Current user">
            JD
          </div>
        </div>
      </div>
    </header>
  );
}
