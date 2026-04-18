import { useEffect, useRef, useState } from 'react';
import { useVoiceStore } from '../store/use-voice-store';
import type { AssignTarget } from '../types/speaker';

const DEFAULT_COLORS = [
  '#E57373',
  '#F06292',
  '#BA68C8',
  '#64B5F6',
  '#4DB6AC',
  '#81C784',
  '#FFD54F',
  '#FF8A65',
];

export function AssignSpeakerPicker({
  onPick,
  onCancel,
}: {
  onPick: (target: AssignTarget) => void;
  onCancel: () => void;
}) {
  const speakers = useVoiceStore((s) => s.speakers);
  const [query, setQuery] = useState('');
  const [creating, setCreating] = useState(false);
  const [newName, setNewName] = useState('');
  const [newColor, setNewColor] = useState(DEFAULT_COLORS[0]);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
  }, [creating]);

  const filtered = speakers.filter((s) =>
    s.display_name.toLowerCase().includes(query.toLowerCase()),
  );

  if (creating) {
    return (
      <div className="assign-picker">
        <input
          ref={inputRef}
          type="text"
          value={newName}
          onChange={(e) => setNewName(e.target.value)}
          placeholder="Name"
          onKeyDown={(e) => {
            if (e.key === 'Enter' && newName.trim()) {
              onPick({
                new_speaker: { display_name: newName.trim(), color: newColor },
              });
            } else if (e.key === 'Escape') {
              setCreating(false);
            }
          }}
        />
        <div className="assign-picker__swatches" role="group" aria-label="Color">
          {DEFAULT_COLORS.map((c) => (
            <button
              key={c}
              type="button"
              className={`assign-picker__swatch${newColor === c ? ' is-selected' : ''}`}
              style={{ backgroundColor: c }}
              onClick={() => setNewColor(c)}
              aria-label={`Select color ${c}`}
              aria-pressed={newColor === c}
            />
          ))}
        </div>
        <div className="assign-picker__actions">
          <button
            type="button"
            className="primary-button"
            disabled={!newName.trim()}
            onClick={() =>
              onPick({
                new_speaker: { display_name: newName.trim(), color: newColor },
              })
            }
          >
            Create and assign
          </button>
          <button
            type="button"
            className="secondary-button"
            onClick={() => setCreating(false)}
          >
            Back
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="assign-picker">
      <input
        ref={inputRef}
        type="text"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        placeholder="Search people…"
        onKeyDown={(e) => {
          if (e.key === 'Escape') onCancel();
        }}
      />
      <ul className="assign-picker__list">
        <li>
          <button
            type="button"
            className="assign-picker__create"
            onClick={() => setCreating(true)}
          >
            + Create new person
          </button>
        </li>
        {filtered.map((s) => (
          <li key={s.id}>
            <button
              type="button"
              className="assign-picker__row"
              onClick={() => onPick({ speaker_id: s.id })}
            >
              <span
                className="assign-picker__avatar"
                style={{ backgroundColor: s.color }}
              >
                {s.display_name.charAt(0).toUpperCase()}
              </span>
              {s.display_name}
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}
