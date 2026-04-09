import { useState, useRef, useEffect } from 'react';
import { AnimatePresence, motion } from 'framer-motion';
import { useStore } from '../../store/use-store';

const PALETTE = [
  { c: '#6366F1', b: '#EEF2FF' },
  { c: '#DC2626', b: '#FEF2F2' },
  { c: '#D97706', b: '#FFFBEB' },
  { c: '#16A34A', b: '#F0FDF4' },
  { c: '#0284C7', b: '#F0F9FF' },
  { c: '#8B5CF6', b: '#EDE9FE' },
  { c: '#EC4899', b: '#FCE7F3' },
  { c: '#F43F5E', b: '#FFE4E6' },
  { c: '#EAB308', b: '#FEF9C3' },
  { c: '#84CC16', b: '#ECFCCB' },
  { c: '#14B8A6', b: '#CCFBF1' },
  { c: '#64748B', b: '#F1F5F9' },
];

export function LabelManager() {
  const labels = useStore((s) => s.labels);
  const addLabel = useStore((s) => s.addLabel);
  const deleteLabel = useStore((s) => s.deleteLabel);

  const [newLabelText, setNewLabelText] = useState('');
  const [newLabelColor, setNewLabelColor] = useState<{ c: string; b: string } | null>(null);
  const [colorError, setColorError] = useState(false);
  const [showColorWheel, setShowColorWheel] = useState(false);
  const colorWheelRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!showColorWheel) return;
    const handler = (e: MouseEvent) => {
      if (colorWheelRef.current && !colorWheelRef.current.contains(e.target as Node)) {
        setShowColorWheel(false);
      }
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [showColorWheel]);

  const usedColors = new Set(labels.map((l) => l.color));
  const availableColors = PALETTE.filter((p) => !usedColors.has(p.c));

  const handleAddLabel = (e: React.FormEvent) => {
    e.preventDefault();
    if (!newLabelText.trim()) return;
    if (!newLabelColor) { setColorError(true); return; }
    addLabel({ name: newLabelText.trim(), color: newLabelColor.c, bgColor: newLabelColor.b });
    setNewLabelText('');
    setNewLabelColor(null);
    setColorError(false);
    setShowColorWheel(false);
  };

  return (
    <section className="settings-section">
      <div className="settings-section__title">Labels</div>

      <div style={{ display: 'flex', flexWrap: 'wrap', gap: '8px', alignItems: 'center' }}>
        {labels.map((label) => (
          <div
            key={label.id}
            className="filter-chip"
            style={{
              display: 'inline-flex',
              alignItems: 'center',
              gap: '4px',
              color: label.color,
              background: label.bgColor,
              borderColor: `${label.color}33`,
              cursor: 'default',
              userSelect: 'none',
            }}
          >
            <span style={{ display: 'inline-block', width: '7px', height: '7px', borderRadius: '50%', background: label.color, flexShrink: 0 }} />
            {label.name}
            <button
              type="button"
              aria-label={`Delete ${label.name}`}
              onClick={() => deleteLabel(label.id)}
              style={{ background: 'none', border: 'none', cursor: 'pointer', padding: '0 2px', marginLeft: '2px', lineHeight: 1, fontSize: '14px', color: 'inherit', opacity: 0.7, display: 'inline-flex', alignItems: 'center' }}
            >
              ×
            </button>
          </div>
        ))}
      </div>

      <form onSubmit={handleAddLabel} style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
        <div ref={colorWheelRef} style={{ position: 'relative', marginTop: '5px' }}>
          <button
            type="button"
            onClick={() => { setShowColorWheel((v) => !v); setColorError(false); }}
            aria-label="Choose color"
            style={{
              width: '28px', height: '28px', borderRadius: '50%',
              background: newLabelColor ? newLabelColor.c : '#fff',
              border: colorError ? '2px solid #dc2626' : newLabelColor ? '2px solid rgba(0,0,0,0.12)' : '2px dashed rgba(0,0,0,0.25)',
              cursor: 'pointer', padding: 0, flexShrink: 0,
              boxShadow: showColorWheel ? '0 0 0 3px rgba(0,0,0,0.12)' : 'none',
              transition: 'box-shadow 0.15s, border-color 0.15s',
            }}
          />
          <AnimatePresence>
            {showColorWheel && (() => {
              const RADIUS = 42;
              const DOT = 22;
              const n = availableColors.length;
              const offset = RADIUS + DOT / 2 + 4;
              return (
                <motion.div
                  key="colorwheel"
                  initial={{ scale: 0, opacity: 0 }}
                  animate={{ scale: 1, opacity: 1 }}
                  exit={{ scale: 0, opacity: 0 }}
                  transition={{ type: 'spring', stiffness: 380, damping: 28, mass: 0.8 }}
                  style={{ position: 'absolute', left: `${14 - offset}px`, top: `${14 - offset}px`, width: `${offset * 2}px`, height: `${offset * 2}px`, zIndex: 200, pointerEvents: 'none', transformOrigin: `${offset}px ${offset}px` }}
                >
                  <div style={{ position: 'absolute', inset: 0, borderRadius: '50%', background: 'var(--color-surface, #fff)', boxShadow: '0 8px 32px rgba(0,0,0,0.16)', border: '1px solid rgba(0,0,0,0.07)', pointerEvents: 'auto' }} />
                  {availableColors.map((p, i) => {
                    const angle = (2 * Math.PI * i) / n - Math.PI / 2;
                    const cx = offset + RADIUS * Math.cos(angle);
                    const cy = offset + RADIUS * Math.sin(angle);
                    const isChosen = newLabelColor?.c === p.c;
                    return (
                      <button
                        key={p.c}
                        type="button"
                        aria-label={`Pick color ${p.c}`}
                        onClick={() => { setNewLabelColor(p); setShowColorWheel(false); setColorError(false); }}
                        style={{
                          position: 'absolute', left: `${cx - DOT / 2}px`, top: `${cy - DOT / 2}px`,
                          width: `${DOT}px`, height: `${DOT}px`, borderRadius: '50%', background: p.c,
                          border: isChosen ? '3px solid var(--color-text-primary)' : '2px solid rgba(255,255,255,0.7)',
                          cursor: 'pointer', padding: 0, pointerEvents: 'auto',
                          boxShadow: isChosen ? '0 0 0 1px rgba(0,0,0,0.25)' : '0 1px 4px rgba(0,0,0,0.18)',
                          transition: 'transform 0.1s, box-shadow 0.1s',
                          transform: isChosen ? 'scale(1.25)' : 'scale(1)',
                        }}
                      />
                    );
                  })}
                </motion.div>
              );
            })()}
          </AnimatePresence>
        </div>

        <input
          type="text"
          value={newLabelText}
          onChange={(e) => setNewLabelText(e.target.value)}
          placeholder="Label name…"
          className="filter-chip"
          style={{ maxWidth: '160px', padding: '0 12px', outline: 'none', cursor: 'text' }}
        />
        <button
          type="submit"
          disabled={!newLabelText.trim()}
          className="filter-chip"
          style={{
            background: newLabelColor ? newLabelColor.b : 'var(--color-surface)',
            color: newLabelColor ? newLabelColor.c : 'var(--color-text-secondary)',
            borderColor: newLabelColor ? `${newLabelColor.c}33` : undefined,
            opacity: newLabelText.trim() ? 1 : 0.4,
            cursor: newLabelText.trim() ? 'pointer' : 'default',
          }}
        >
          Add label
        </button>
        {colorError && (
          <span style={{ fontSize: '0.78rem', color: '#dc2626', whiteSpace: 'nowrap' }}>Pick a color first</span>
        )}
      </form>
    </section>
  );
}
