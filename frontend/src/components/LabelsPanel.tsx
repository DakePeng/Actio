import { useStore } from '../store/use-store';
import { BUILTIN_LABELS, computeLabelCounts } from '../utils/labels';

const labelDotColors: Record<string, string> = {
  work: '#6366F1',
  urgent: '#DC2626',
  meeting: '#D97706',
  personal: '#16A34A',
  health: '#CA8A04',
  finance: '#0284C7',
};

export function LabelsPanel() {
  const showLabelsPanel = useStore((s) => s.ui.showLabelsPanel);
  const toggleLabelsPanel = useStore((s) => s.toggleLabelsPanel);
  const activeLabel = useStore((s) => s.filter.label);
  const setFilter = useStore((s) => s.setFilter);
  const setFeedback = useStore((s) => s.setFeedback);
  const reminders = useStore((s) => s.reminders);
  const labelCounts = computeLabelCounts(reminders);

  return (
    <>
      {showLabelsPanel && <div className="sheet-overlay" onClick={toggleLabelsPanel} />}

      <aside
        className="labels-panel"
        style={{
          transform: showLabelsPanel ? 'translateX(0)' : 'translateX(100%)',
          transition: 'transform 0.28s ease',
        }}
        aria-hidden={!showLabelsPanel}
      >
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: '12px' }}>
          <div>
            <h2 style={{ fontSize: '1.1rem', fontWeight: 700, letterSpacing: '-0.04em' }}>Labels</h2>
            <p style={{ marginTop: '4px', fontSize: '0.9rem', color: 'var(--color-text-secondary)' }}>
              Keep one category in focus and tighten the board.
            </p>
          </div>
          <button type="button" className="ghost-button" onClick={toggleLabelsPanel}>
            Close
          </button>
        </div>

        <div className="labels-list">
          {BUILTIN_LABELS.map((label) => {
            const count = labelCounts.get(label.id) ?? 0;
            return (
              <button
                key={label.id}
                type="button"
                className={`label-row-item${activeLabel === label.id ? ' is-active' : ''}`}
                onClick={() => {
                  const nextLabel = activeLabel === label.id ? null : label.id;
                  setFilter({ label: nextLabel });
                  setFeedback(nextLabel ? `${label.name} filter applied` : 'Label filter cleared');
                  toggleLabelsPanel();
                }}
              >
                <div
                  style={{ display: 'flex', alignItems: 'center', gap: '8px' }}
                >
                  <span
                    style={{
                      width: '8px',
                      height: '8px',
                      borderRadius: '4px',
                      backgroundColor: labelDotColors[label.id],
                    }}
                  />
                  {label.name}
                </div>
                <span style={{ fontSize: '0.82rem', color: 'var(--color-text-tertiary)' }}>
                  {count}
                </span>
              </button>
            );
          })}
        </div>

        <button
          type="button"
          className="secondary-button"
          onClick={() => {
            setFilter({ label: null });
            setFeedback('Label filter cleared');
          }}
        >
          Clear label focus
        </button>
      </aside>
    </>
  );
}
