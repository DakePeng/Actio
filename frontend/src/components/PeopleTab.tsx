import { useState } from 'react';
import { useVoiceStore } from '../store/use-voice-store';
import type { Person } from '../types';

const PRESET_COLORS = [
  '#E57373',
  '#F06292',
  '#BA68C8',
  '#64B5F6',
  '#4DB6AC',
  '#81C784',
  '#FFD54F',
  '#FF8A65',
];

type FormMode = 'idle' | 'adding' | { editing: string };

export function PeopleTab() {
  const people = useVoiceStore((s) => s.people);
  const addPerson = useVoiceStore((s) => s.addPerson);
  const updatePerson = useVoiceStore((s) => s.updatePerson);
  const deletePerson = useVoiceStore((s) => s.deletePerson);

  const [formMode, setFormMode] = useState<FormMode>('idle');
  const [name, setName] = useState('');
  const [color, setColor] = useState(PRESET_COLORS[0]);

  function openAdd() {
    setFormMode('adding');
    setName('');
    setColor(PRESET_COLORS[0]);
  }

  function openEdit(person: Person) {
    setFormMode({ editing: person.id });
    setName(person.name);
    setColor(person.color);
  }

  function handleSave() {
    const trimmed = name.trim();
    if (!trimmed) return;
    if (formMode === 'adding') {
      addPerson(trimmed, color);
    } else if (typeof formMode === 'object') {
      updatePerson(formMode.editing, { name: trimmed, color });
    }
    setFormMode('idle');
  }

  function handleCancel() {
    setFormMode('idle');
  }

  const isFormOpen = formMode !== 'idle';

  return (
    <div className="people-tab">
      {!isFormOpen && (
        <button type="button" className="primary-button people-tab__add-btn" onClick={openAdd}>
          Add person
        </button>
      )}

      {isFormOpen && (
        <div className="person-form">
          <input
            type="text"
            className="person-form__name-input"
            placeholder="Name"
            value={name}
            onChange={(e) => setName(e.target.value)}
            autoFocus
            onKeyDown={(e) => {
              if (e.key === 'Enter') handleSave();
              if (e.key === 'Escape') handleCancel();
            }}
          />
          <div className="person-form__swatches" role="group" aria-label="Color">
            {PRESET_COLORS.map((c) => (
              <button
                key={c}
                type="button"
                className={`person-form__swatch${color === c ? ' is-selected' : ''}`}
                style={{ backgroundColor: c }}
                onClick={() => setColor(c)}
                aria-label={`Select color ${c}`}
                aria-pressed={color === c}
              />
            ))}
          </div>
          <div className="person-form__actions">
            <button
              type="button"
              className="primary-button"
              onClick={handleSave}
              disabled={!name.trim()}
            >
              Save
            </button>
            <button type="button" className="secondary-button" onClick={handleCancel}>
              Cancel
            </button>
          </div>
        </div>
      )}

      <div className="people-tab__list">
        {people.length === 0 && !isFormOpen && (
          <p className="people-tab__empty">No people added yet.</p>
        )}
        {people.map((person) => (
          <div key={person.id} className="person-row">
            <div
              className="person-row__avatar"
              style={{ backgroundColor: person.color }}
              aria-hidden="true"
            >
              {person.name.charAt(0).toUpperCase()}
            </div>
            <span className="person-row__name">{person.name}</span>
            <div className="person-row__actions">
              <button
                type="button"
                className="person-edit-btn"
                onClick={() => openEdit(person)}
                aria-label={`Edit ${person.name}`}
              >
                ✏️
              </button>
              <button
                type="button"
                className="person-delete-btn"
                onClick={() => deletePerson(person.id)}
                aria-label={`Delete ${person.name}`}
              >
                🗑
              </button>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
