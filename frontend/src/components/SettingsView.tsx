import { useStore } from '../store/use-store';

export function SettingsView() {
  const hasSeenOnboarding = useStore((s) => s.ui.hasSeenOnboarding);
  const setHasSeenOnboarding = useStore((s) => s.setHasSeenOnboarding);
  const archivedReminders = useStore((s) => s.archivedReminders);
  const reminders = useStore((s) => s.reminders);
  const setFeedback = useStore((s) => s.setFeedback);

  return (
    <main className="board-shell board-shell--section">
      <section className="workspace-controls workspace-controls--quiet">
        <div className="workspace-controls__main">
          <span className="active-pill">Board preferences</span>
        </div>
        <div className="workspace-controls__actions">
          <span className="active-pill">{reminders.length} live</span>
          <span className="active-pill">{archivedReminders.length} archived</span>
        </div>
      </section>

      <section className="section-hero">
        <div>
          <div className="section-hero__eyebrow">Settings</div>
          <h1 className="section-hero__title">Tune the board without leaving the workspace.</h1>
          <p className="section-hero__copy">
            Keep these controls light. This section should feel like board preferences, not a separate admin area.
          </p>
        </div>
      </section>

      <section className="settings-grid">
        <article className="settings-card">
          <div className="settings-card__eyebrow">Onboarding</div>
          <h2 className="settings-card__title">Replay the first-run guidance</h2>
          <p className="settings-card__copy">
            Useful when you want to recheck the board behavior or demo the product flow again.
          </p>
          <div className="settings-card__actions">
            <span className="active-pill">{hasSeenOnboarding ? 'Dismissed' : 'Visible'}</span>
            <button
              type="button"
              className="secondary-button"
              onClick={() => {
                localStorage.removeItem('actio-onboarded');
                setHasSeenOnboarding(false);
                setFeedback('Onboarding will show again next time you open the board');
              }}
            >
              Show onboarding again
            </button>
          </div>
        </article>

        <article className="settings-card">
          <div className="settings-card__eyebrow">Workflow</div>
          <h2 className="settings-card__title">Archive is now your completion lane</h2>
          <p className="settings-card__copy">
            Marking a reminder done moves it into Archive instead of deleting it. Restore from Archive whenever you need to reopen something.
          </p>
          <div className="settings-card__actions">
            <span className="active-pill">Top tabs enabled</span>
            <span className="active-pill">No side rail</span>
          </div>
        </article>
      </section>
    </main>
  );
}
