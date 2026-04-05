export function EmptyState() {
  return (
    <div className="empty-shell">
      <div>
        <div className="relative mb-6 flex justify-center">
          <div className="empty-pulse animate-pulse" />
        </div>
        <h2 className="mb-2 text-3xl font-semibold tracking-[-0.04em] text-text">All clear.</h2>
        <p className="mx-auto max-w-md text-sm leading-6 text-text-secondary">
          No reminders match the current view. Clear your filters or capture a new task to refill the board.
        </p>
      </div>
    </div>
  );
}
