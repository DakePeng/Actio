import { useState } from 'react';
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
  const customLabels = useStore((s) => s.customLabels || []);
  const addCustomLabel = useStore((s) => s.addCustomLabel);
  const labelCounts = computeLabelCounts(reminders);
  
  const [newLabelName, setNewLabelName] = useState('');

  const handleAddLabel = (e: React.FormEvent) => {
    e.preventDefault();
    if (!newLabelName.trim()) return;
    
    addCustomLabel({
      name: newLabelName.trim(),
      color: '#4f46e5', // var(--color-accent)
      bgColor: '#eef2ff' // var(--color-accent-wash)
    });
    setNewLabelName('');
  };

  const allLabels = [...BUILTIN_LABELS, ...customLabels];

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
        <div className="sheet-header">
          <div>
            <div className="sheet-eyebrow">Label focus</div>
            <h2 className="sheet-title">Tighten the board by context</h2>
            <p className="sheet-copy">Hold one category in view and suppress the rest of the queue.</p>
          </div>
          <button type="button" className="ghost-button" onClick={toggleLabelsPanel}>
            Close
          </button>
        </div>

        <div className="labels-list">
          {allLabels.map((label) => {
            const count = labelCounts.get(label.id) ?? 0;
            const dotColor = labelDotColors[label.id] || label.color;
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
                <div className="label-row-item__meta">
                  <span
                    className="label-row-item__dot"
                    style={{ backgroundColor: dotColor }}
                  />
                  {label.name}
                </div>
                <span className="label-row-item__count">{count}</span>
              </button>
            );
          })}
        </div>

        <form onSubmit={handleAddLabel} style={{ display: 'flex', gap: '8px', marginTop: '16px' }}>
          <input 
            type="text" 
            placeholder="New label..." 
            value={newLabelName}
            onChange={(e) => setNewLabelName(e.target.value)}
            className="field-input"
            style={{ flex: 1, minHeight: '40px', padding: '8px 12px' }}
          />
          <button type="submit" className="primary-button" style={{ height: '40px' }} disabled={!newLabelName.trim()}>
            Add
          </button>
        </form>

        <button
          type="button"
          className="secondary-button"
          style={{ marginTop: '16px', width: '100%' }}
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
