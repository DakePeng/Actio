import { useState } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
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

function PencilIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M17 3a2.85 2.83 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5Z" />
      <path d="m15 5 4 4" />
    </svg>
  );
}

function TrashIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <polyline points="3 6 5 6 21 6" />
      <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />
    </svg>
  );
}

function PlusIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round">
      <line x1="12" y1="5" x2="12" y2="19" />
      <line x1="5" y1="12" x2="19" y2="12" />
    </svg>
  );
}

const personVariants = {
  hidden: { opacity: 0, y: 16 },
  visible: (i: number) => ({
    opacity: 1,
    y: 0,
    transition: { delay: i * 0.05, type: 'spring' as const, stiffness: 300, damping: 24 },
  }),
  exit: { opacity: 0, x: -20, transition: { duration: 0.15 } },
};

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
      <AnimatePresence mode="wait">
        {!isFormOpen ? (
          <motion.button
            key="add-btn"
            type="button"
            className="primary-button people-tab__add-btn"
            onClick={openAdd}
            initial={{ opacity: 0, scale: 0.9 }}
            animate={{ opacity: 1, scale: 1 }}
            exit={{ opacity: 0, scale: 0.9 }}
            transition={{ duration: 0.15 }}
            whileHover={{ scale: 1.02 }}
            whileTap={{ scale: 0.97 }}
          >
            <PlusIcon />
            Add person
          </motion.button>
        ) : (
          <motion.div
            key="form"
            className="person-form"
            initial={{ opacity: 0, y: -12, scale: 0.97 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: -12, scale: 0.97 }}
            transition={{ type: 'spring', stiffness: 400, damping: 28 }}
          >
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
                <motion.button
                  key={c}
                  type="button"
                  className={`person-form__swatch${color === c ? ' is-selected' : ''}`}
                  style={{ backgroundColor: c }}
                  onClick={() => setColor(c)}
                  aria-label={`Select color ${c}`}
                  aria-pressed={color === c}
                  whileHover={{ scale: 1.18 }}
                  whileTap={{ scale: 0.92 }}
                />
              ))}
            </div>
            <div className="person-form__actions">
              <motion.button
                type="button"
                className="primary-button"
                onClick={handleSave}
                disabled={!name.trim()}
                whileHover={{ scale: 1.02 }}
                whileTap={{ scale: 0.97 }}
              >
                Save
              </motion.button>
              <motion.button
                type="button"
                className="secondary-button"
                onClick={handleCancel}
                whileHover={{ scale: 1.02 }}
                whileTap={{ scale: 0.97 }}
              >
                Cancel
              </motion.button>
            </div>
          </motion.div>
        )}
      </AnimatePresence>

      <div className="people-tab__list">
        <AnimatePresence>
          {people.length === 0 && !isFormOpen && (
            <motion.p
              key="empty"
              className="people-tab__empty"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
            >
              No people added yet.
            </motion.p>
          )}
          {people.map((person, i) => (
            <motion.div
              key={person.id}
              className="person-row"
              variants={personVariants}
              initial="hidden"
              animate="visible"
              exit="exit"
              custom={i}
              layout
            >
              <motion.div
                className="person-row__avatar"
                style={{ backgroundColor: person.color }}
                aria-hidden="true"
                layoutId={`avatar-${person.id}`}
              >
                {person.name.charAt(0).toUpperCase()}
              </motion.div>
              <span className="person-row__name">{person.name}</span>
              <div className="person-row__actions">
                <motion.button
                  type="button"
                  className="person-edit-btn"
                  onClick={() => openEdit(person)}
                  aria-label={`Edit ${person.name}`}
                  whileHover={{ scale: 1.15 }}
                  whileTap={{ scale: 0.9 }}
                >
                  <PencilIcon />
                </motion.button>
                <motion.button
                  type="button"
                  className="person-delete-btn"
                  onClick={() => deletePerson(person.id)}
                  aria-label={`Delete ${person.name}`}
                  whileHover={{ scale: 1.15 }}
                  whileTap={{ scale: 0.9 }}
                >
                  <TrashIcon />
                </motion.button>
              </div>
            </motion.div>
          ))}
        </AnimatePresence>
      </div>
    </div>
  );
}
