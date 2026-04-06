export function EmptyState({ hasFilters }: { hasFilters?: boolean }) {
  if (hasFilters) {
    return (
      <div className="empty-shell">
        <div className="empty-shell__inner">
          <h2 className="empty-shell__title">No results found</h2>
          <p className="empty-shell__copy">
            No reminders match your search or filters.
          </p>
        </div>
      </div>
    );
  }

  return (
    <div className="empty-shell">
      <div className="empty-shell__inner">
        <div className="empty-shell__mark" aria-hidden="true">
          <div className="empty-pulse" />
        </div>
        <div className="empty-shell__eyebrow">All caught up</div>
        <h2 className="empty-shell__title">The board is clear for now.</h2>
        <p className="empty-shell__copy">
          Capture a new task to refill the board.
        </p>
      </div>
    </div>
  );
}
