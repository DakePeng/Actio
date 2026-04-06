export function EmptyState() {
  return (
    <div className="empty-shell">
      <div className="empty-shell__inner">
        <div className="empty-shell__mark" aria-hidden="true">
          <div className="empty-pulse" />
        </div>
        <div className="empty-shell__eyebrow">No active matches</div>
        <h2 className="empty-shell__title">The board is clear for now.</h2>
        <p className="empty-shell__copy">
          No reminders match the current view. Clear your filters or capture a new task to refill the board.
        </p>
      </div>
    </div>
  );
}
