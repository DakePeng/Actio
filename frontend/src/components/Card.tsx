import type { Reminder } from '../types';
import { useStore } from '../store/use-store';
import { getLabelById } from '../utils/labels';
import { formatTimeShort } from '../utils/time';
import { AnimatePresence, motion, useMotionValue, useTransform } from 'framer-motion';

interface CardProps {
  reminder: Reminder;
  isExpanded: boolean;
  onToggle: () => void;
}

// CSS for line-clamp since we're using inline styles
const lineClampStyle: React.CSSProperties = {
  display: '-webkit-box',
  WebkitLineClamp: 2,
  WebkitBoxOrient: 'vertical',
  overflow: 'hidden',
};

export function Card({ reminder, isExpanded, onToggle }: CardProps) {
  const setFilter = useStore((s) => s.setFilter);
  const markDone = useStore((s) => s.markDone);
  const setFeedback = useStore((s) => s.setFeedback);
  const highlightedCardId = useStore((s) => s.ui.highlightedCardId);
  const { title, description, priority: p, labels, dueTime, transcript, context } = reminder;
  const displayLabels = labels.slice(0, 3);
  const timeDisplay = dueTime ? formatTimeShort(dueTime) : 'No deadline';
  const isHighlighted = highlightedCardId === reminder.id;

  const priority = p || 'medium';
  const priorityColors = {
    high: { accent: '#dc2626', bg: '#fef2f2', text: '#b91c1c', label: 'High priority' },
    medium: { accent: '#d97706', bg: '#fff7df', text: '#a16207', label: 'Medium priority' },
    low: { accent: '#1e7a53', bg: '#edf9f1', text: '#166534', label: 'Low priority' },
  }[priority];

  const x = useMotionValue(0);
  const rot = useTransform(x, [-200, 200], [-10, 10]);
  const opac = useTransform(x, [-200, -100, 0, 100, 200], [0, 1, 1, 1, 0]);
  
  // Drag feedback properties
  const dragFeedbackOpacity = useTransform(x, [-120, -80, 0, 80, 120], [1, 0, 0, 0, 1]);
  const dragFeedbackScale = useTransform(x, [-120, -80, 0, 80, 120], [1, 0.8, 0.8, 0.8, 1]);

  return (
    <motion.div
      layout
      initial={{ opacity: 0, y: 30 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, scale: 0.8, transition: { duration: 0.15 } }}
      style={{ x, rotate: rot, opacity: opac, cursor: 'grab' }}
      whileTap={{ cursor: 'grabbing' }}
      drag="x"
      dragConstraints={{ left: 0, right: 0 }}
      onDragEnd={(_e, { offset, velocity }) => {
        if (Math.abs(offset.x) > 120 || Math.abs(velocity.x) > 400) {
          markDone(reminder.id);
          setFeedback(`Completed: ${title}`);
        }
      }}
    >
      <article className={`reminder-card${isExpanded ? ' is-expanded' : ''}${isHighlighted ? ' is-highlighted' : ''}`}>
        
        <motion.div 
          style={{ 
            position: 'absolute', inset: 0, 
            background: '#e4f9f4', 
            display: 'flex', alignItems: 'center', justifyContent: 'center',
            opacity: dragFeedbackOpacity, zIndex: 20, borderRadius: 'inherit'
          }}
        >
          <motion.div style={{ scale: dragFeedbackScale, color: '#0f766e', fontWeight: 800, fontSize: '1rem', display: 'flex', gap: '8px', alignItems: 'center', letterSpacing: '-0.03em' }}>
            <svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round"><polyline points="20 6 9 17 4 12"></polyline></svg>
            Mark done
          </motion.div>
        </motion.div>
        <div className="reminder-accent" style={{ background: priorityColors.accent }} aria-hidden="true" />
        <div className="card-shell">
          <div className="card-head">
            <span
              className="card-badge"
              style={{
                background: priorityColors.bg,
                color: priorityColors.text,
              }}
            >
              {priorityColors.label}
            </span>
            <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
              {reminder.isNew && <span className="mini-badge">New</span>}
              <button
                type="button"
                className="card-expand"
                onClick={onToggle}
                aria-expanded={isExpanded}
                aria-label={isExpanded ? `Collapse ${title}` : `Expand ${title}`}
              >
                <span
                  aria-hidden="true"
                  style={{
                    display: 'inline-block',
                    transition: 'transform 0.18s ease',
                    transform: isExpanded ? 'rotate(180deg)' : 'rotate(0deg)',
                  }}
                >
                  ↓
                </span>
              </button>
            </div>
          </div>

          <div className="card-title">{title}</div>

          {description && (
            <div className="card-description" style={!isExpanded ? lineClampStyle : undefined}>
              {description}
            </div>
          )}

          <div className="card-meta">
            <div className="card-meta__item">
              <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" style={{ width: '15px', height: '15px' }}>
                <circle cx="12" cy="12" r="10" />
                <path d="M12 6v6l4 2" />
              </svg>
              <span>{timeDisplay}</span>
            </div>
            <span className="card-meta__count">{labels.length} labels</span>
          </div>

          <div className="label-row">
            {displayLabels.map((labelId) => {
              const label = getLabelById(labelId);
              if (!label) return null;
              return (
                <button
                  key={labelId}
                  type="button"
                  onClick={() => setFilter({ label: labelId })}
                  className="label-chip"
                  style={{
                    background: label.bgColor,
                    color: label.color,
                    borderColor: `${label.color}22`,
                  }}
                >
                  {label.name}
                </button>
              );
            })}
          </div>

          <AnimatePresence>
            {isExpanded && (transcript || context) && (
              <motion.div
                initial={{ opacity: 0, height: 0 }}
                animate={{ opacity: 1, height: 'auto' }}
                exit={{ opacity: 0, height: 0 }}
                transition={{ duration: 0.2 }}
                className="card-detail"
              >
                {transcript && <div>{transcript}</div>}
                {context && <div className="card-context">{context}</div>}
              </motion.div>
            )}
          </AnimatePresence>
        </div>
      </article>
    </motion.div>
  );
}
