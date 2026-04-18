import { useState, useEffect, useRef } from 'react';
import type { Reminder, Priority } from '../types';
import { useStore } from '../store/use-store';
import { getLabelById } from '../utils/labels';
import { formatTimeShort } from '../utils/time';
import { AnimatePresence, motion, useMotionValue, useTransform } from 'framer-motion';

/** Convert an ISO 8601 timestamp to the `YYYY-MM-DDTHH:MM` format that
 *  `<input type="datetime-local">` expects, in the browser's local tz. */
function toDatetimeLocalValue(iso: string | undefined): string {
  if (!iso) return '';
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return '';
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** Parse a `datetime-local` input string back into an ISO 8601 UTC string.
 *  Empty string → undefined (clears the due time). */
function fromDatetimeLocalValue(local: string): string | undefined {
  if (!local) return undefined;
  const d = new Date(local);
  return Number.isNaN(d.getTime()) ? undefined : d.toISOString();
}

interface CardProps {
  reminder: Reminder;
  isExpanded: boolean;
  onToggle: () => void;
  isFocused?: boolean;
  focusedRef?: React.RefObject<HTMLDivElement | null>;
}

export function Card({ reminder, isExpanded, onToggle, isFocused, focusedRef }: CardProps) {
  // Skeleton variant — mirrors the collapsed-card structure so the layout
  // doesn't jump when the real content arrives. Every piece shimmers in unison.
  if (reminder.isExtracting) {
    return (
      <motion.div
        layout
        initial={{ opacity: 0, y: 30 }}
        animate={{ opacity: 1, y: 0 }}
        exit={{ opacity: 0, scale: 0.8, transition: { duration: 0.15 } }}
      >
        <article
          className="reminder-card card--skeleton"
          aria-busy="true"
          aria-label="Extracting reminder…"
        >
          <div className="reminder-accent" />
          <div className="card-shell">
            <div className="card-head">
              <span className="skeleton-badge" />
              <span className="mini-badge mini-badge--ai skeleton-ai-badge">AI</span>
            </div>
            <div className="skeleton-line skeleton-line--title" />
            <div className="skeleton-line skeleton-line--desc" />
            <div className="skeleton-line skeleton-line--desc-short" />
            <div className="card-meta">
              <div className="card-meta__item">
                <span className="skeleton-dot" />
                <span className="skeleton-line skeleton-line--meta" />
              </div>
              <span className="skeleton-line skeleton-line--meta-short" />
            </div>
            <div className="label-row">
              <span className="skeleton-chip" style={{ width: 48 }} />
              <span className="skeleton-chip" style={{ width: 64 }} />
              <span className="skeleton-chip" style={{ width: 40 }} />
            </div>
          </div>
        </article>
      </motion.div>
    );
  }

  const setFilter = useStore((s) => s.setFilter);
  const archiveReminder = useStore((s) => s.archiveReminder);
  const setPriority = useStore((s) => s.setPriority);
  const setLabels = useStore((s) => s.setLabels);
  const updateReminderInline = useStore((s) => s.updateReminderInline);
  const allLabels = useStore((s) => s.labels);
  const setFeedback = useStore((s) => s.setFeedback);
  const highlightedCardId = useStore((s) => s.ui.highlightedCardId);
  const clearAiGenerated = useStore((s) => s.clearAiGenerated);

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

  // Inline editing state
  const [editTitle, setEditTitle] = useState(title);
  const [editDescription, setEditDescription] = useState(description);
  const [editDueTime, setEditDueTime] = useState(toDatetimeLocalValue(dueTime));

  // Sync when reminder changes externally
  useEffect(() => { setEditTitle(title); }, [title]);
  useEffect(() => { setEditDescription(description); }, [description]);
  useEffect(() => { setEditDueTime(toDatetimeLocalValue(dueTime)); }, [dueTime]);

  // Commit edits on blur / on collapse
  const commitEdits = async () => {
    const t = editTitle.trim();
    const d = editDescription.trim();
    const currentDueLocal = toDatetimeLocalValue(dueTime);
    const nextDueLocal = editDueTime.trim();

    const patch: Partial<Pick<Reminder, 'title' | 'description' | 'dueTime'>> = {};
    if (t !== title) patch.title = t || title;
    if (d !== description) patch.description = d;
    if (nextDueLocal !== currentDueLocal) {
      patch.dueTime = fromDatetimeLocalValue(nextDueLocal);
    }

    if (Object.keys(patch).length > 0) {
      await updateReminderInline(reminder.id, patch);
    }
  };

  useEffect(() => {
    if (!isExpanded) void commitEdits();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isExpanded]);

  // Drag-to-done
  const x = useMotionValue(0);
  const rot = useTransform(x, [-200, 200], [-10, 10]);
  const opac = useTransform(x, [-200, -100, 0, 100, 200], [0, 1, 1, 1, 0]);
  const dragFeedbackOpacity = useTransform(x, [-120, -80, 0, 80, 120], [1, 0, 0, 0, 1]);
  const dragFeedbackScale = useTransform(x, [-120, -80, 0, 80, 120], [1, 0.8, 0.8, 0.8, 1]);

  // Label dropdown
  const [labelDropdownOpen, setLabelDropdownOpen] = useState(false);
  const dropdownRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!labelDropdownOpen) return;
    const handler = (e: MouseEvent) => {
      if (dropdownRef.current && !dropdownRef.current.contains(e.target as Node)) {
        setLabelDropdownOpen(false);
      }
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [labelDropdownOpen]);

  useEffect(() => {
    if (!labelDropdownOpen) return;
    const handler = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setLabelDropdownOpen(false);
    };
    document.addEventListener('keydown', handler);
    return () => document.removeEventListener('keydown', handler);
  }, [labelDropdownOpen]);

  const unassignedLabels = allLabels.filter((l) => !labels.includes(l.id));

  const priorityOptions: Array<{ value: Priority; label: string }> = [
    { value: 'high', label: 'High' },
    { value: 'medium', label: 'Medium' },
    { value: 'low', label: 'Low' },
  ];

  // Stop drag from eating interactive-element clicks
  const stopDrag = (e: React.PointerEvent) => e.stopPropagation();

  return (
    <motion.div
      ref={focusedRef}
      layout
      initial={{ opacity: 0, y: 30 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, scale: 0.8, transition: { duration: 0.15 } }}
      className={isFocused ? 'card-kb-focused' : undefined}
      style={{ x, rotate: rot, opacity: opac, cursor: 'grab' }}
      whileTap={{ cursor: 'grabbing' }}
      drag="x"
      dragConstraints={{ left: 0, right: 0 }}
      onDragEnd={(_e, { offset, velocity }) => {
        if (Math.abs(offset.x) > 120 || Math.abs(velocity.x) > 400) {
          void archiveReminder(reminder.id);
          setFeedback(`Archived: ${title}`);
        }
      }}
    >
      <article
        className={`reminder-card${isExpanded ? ' is-expanded' : ''}${isHighlighted ? ' is-highlighted' : ''}`}
        onClick={(e) => {
          e.stopPropagation();
          if (reminder.isAiGenerated) clearAiGenerated(reminder.id);
          onToggle();
        }}
      >
        {/* Swipe-to-done overlay */}
        <motion.div
          style={{
            position: 'absolute', inset: 0,
            background: '#e4f9f4',
            display: 'flex', alignItems: 'center', justifyContent: 'center',
            opacity: dragFeedbackOpacity, zIndex: 20, borderRadius: 'inherit',
            pointerEvents: 'none',
          }}
        >
          <motion.div style={{ scale: dragFeedbackScale, color: '#0f766e', fontWeight: 800, fontSize: '1rem', display: 'flex', gap: '8px', alignItems: 'center', letterSpacing: '-0.03em' }}>
            <svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round"><polyline points="20 6 9 17 4 12"></polyline></svg>
            Mark done
          </motion.div>
        </motion.div>

        <div className="reminder-accent" style={{ background: priorityColors.accent }} aria-hidden="true" />
        <div className="card-shell">
          {/* Head: badge only, no arrow button */}
          <div className="card-head">
            <span
              className="card-badge"
              style={{ background: priorityColors.bg, color: priorityColors.text }}
            >
              {priorityColors.label}
            </span>
            <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
              {reminder.isAiGenerated && <span className="mini-badge mini-badge--ai">AI</span>}
              {reminder.isNew && <span className="mini-badge">New</span>}
            </div>
          </div>

          {/* Title — editable when expanded */}
          {isExpanded ? (
            <input
              className="card-title card-editable"
              value={editTitle}
              onChange={(e) => setEditTitle(e.target.value)}
              onBlur={() => void commitEdits()}
              onPointerDown={stopDrag}
              onClick={(e) => e.stopPropagation()}
              placeholder="Reminder title"
              aria-label="Edit title"
            />
          ) : (
            <div className="card-title">{title}</div>
          )}

          {/* Description — editable textarea when expanded */}
          {isExpanded ? (
            <textarea
              className="card-description card-editable"
              value={editDescription}
              onChange={(e) => setEditDescription(e.target.value)}
              onBlur={() => void commitEdits()}
              onPointerDown={stopDrag}
              onClick={(e) => e.stopPropagation()}
              rows={3}
              placeholder="Add a description…"
              aria-label="Edit description"
              style={{ lineHeight: '1.6' }}
            />
          ) : description ? (
            <div
              className="card-description"
              style={{
                display: '-webkit-box',
                WebkitLineClamp: 2,
                WebkitBoxOrient: 'vertical',
                overflow: 'hidden',
              }}
            >
              {description}
            </div>
          ) : null}

          {/* Subtle edit hint shown briefly when expanded */}
          {isExpanded && (
            <div style={{ fontSize: '0.72rem', color: 'var(--color-text-tertiary)', marginTop: '2px', letterSpacing: '0.01em' }}>
              Tap title or description to edit
            </div>
          )}

          <div className="card-meta">
            <div className="card-meta__item">
              <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" style={{ width: '15px', height: '15px' }}>
                <circle cx="12" cy="12" r="10" />
                <path d="M12 6v6l4 2" />
              </svg>
              {isExpanded ? (
                <input
                  type="datetime-local"
                  className="card-editable card-due-input"
                  value={editDueTime}
                  onChange={(e) => setEditDueTime(e.target.value)}
                  onBlur={() => void commitEdits()}
                  onPointerDown={stopDrag}
                  onClick={(e) => e.stopPropagation()}
                  aria-label="Edit due time"
                />
              ) : (
                <span>{timeDisplay}</span>
              )}
            </div>
            <span className="card-meta__count">{labels.length} labels</span>
          </div>

          {/* Label chips */}
          <div className="label-row" onPointerDown={stopDrag} onClick={(e) => e.stopPropagation()}>
            {displayLabels.map((labelId) => {
              const label = getLabelById(allLabels, labelId);
              if (!label) return null;
              return (
                <button
                  key={labelId}
                  type="button"
                  onClick={(e) => { e.stopPropagation(); setFilter({ label: labelId }); }}
                  className="label-chip"
                  style={{ background: label.bgColor, color: label.color, borderColor: `${label.color}22` }}
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

          <AnimatePresence>
            {isExpanded && (
              <motion.div
                initial={{ opacity: 0, height: 0 }}
                animate={{ opacity: 1, height: 'auto' }}
                exit={{ opacity: 0, height: 0 }}
                transition={{ duration: 0.2 }}
                className="card-edit"
                onPointerDown={stopDrag}
                onClick={(e) => e.stopPropagation()}
              >
                {/* Priority row */}
                <div className="card-edit__row">
                  <span className="card-edit__label">Priority</span>
                  <div style={{ display: 'flex', gap: '6px' }}>
                    {priorityOptions.map((opt) => {
                      const isActive = priority === opt.value;
                      const colors = {
                        high: { bg: '#fef2f2', text: '#b91c1c' },
                        medium: { bg: '#fff7df', text: '#a16207' },
                        low: { bg: '#edf9f1', text: '#166534' },
                      }[opt.value];
                      return (
                        <button
                          key={opt.value}
                          type="button"
                          className={`priority-btn${isActive ? ' is-active' : ''}`}
                          style={isActive ? { background: colors.bg, color: colors.text } : undefined}
                          onClick={() => void setPriority(reminder.id, opt.value)}
                        >
                          {opt.label}
                        </button>
                      );
                    })}
                  </div>
                </div>

                {/* Labels row */}
                <div className="card-edit__row">
                  <span className="card-edit__label">Labels</span>
                  <div style={{ display: 'flex', flexWrap: 'wrap', gap: '6px', flex: 1, position: 'relative' }} ref={dropdownRef}>
                    {labels.map((labelId) => {
                      const label = getLabelById(allLabels, labelId);
                      if (!label) return null;
                      return (
                        <span
                          key={labelId}
                          className="label-chip"
                          style={{ background: label.bgColor, color: label.color, borderColor: `${label.color}22`, display: 'inline-flex', alignItems: 'center', gap: '4px' }}
                        >
                          {label.name}
                          <button
                            type="button"
                            aria-label={`Remove ${label.name}`}
                            onClick={() => void setLabels(reminder.id, labels.filter((id) => id !== labelId))}
                            style={{ background: 'none', border: 'none', cursor: 'pointer', padding: '0 2px', lineHeight: 1, color: 'inherit', opacity: 0.7 }}
                          >
                            ×
                          </button>
                        </span>
                      );
                    })}

                    {unassignedLabels.length > 0 && (
                      <button
                        type="button"
                        className="priority-btn"
                        onClick={() => setLabelDropdownOpen((v) => !v)}
                        aria-haspopup="listbox"
                        aria-expanded={labelDropdownOpen}
                      >
                        + add
                      </button>
                    )}

                    {labelDropdownOpen && (
                      <div className="label-add-dropdown" role="listbox">
                        {unassignedLabels.map((label) => (
                          <button
                            key={label.id}
                            type="button"
                            role="option"
                            className="label-add-dropdown__item"
                            onClick={() => {
                              void setLabels(reminder.id, [...labels, label.id]);
                              setLabelDropdownOpen(false);
                            }}
                          >
                            <span
                              style={{
                                display: 'inline-block',
                                width: '8px',
                                height: '8px',
                                borderRadius: '50%',
                                background: label.color,
                                flexShrink: 0,
                              }}
                            />
                            {label.name}
                          </button>
                        ))}
                      </div>
                    )}
                  </div>
                </div>
              </motion.div>
            )}
          </AnimatePresence>
        </div>
      </article>
    </motion.div>
  );
}
