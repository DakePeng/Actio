import { useEffect, useState } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { useVoiceStore } from '../store/use-voice-store';
import { PendingVoicesPanel } from './PendingVoicesPanel';
import { VoiceprintRecorder } from './VoiceprintRecorder';
import type { Speaker } from '../types/speaker';
import { useT } from '../i18n';

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

type FormMode =
  | { kind: 'idle' }
  | { kind: 'adding' }
  | { kind: 'editing'; speaker: Speaker }
  | { kind: 'enrolling'; speaker: Speaker };

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

function MicIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M12 1a3 3 0 0 0-3 3v8a3 3 0 0 0 6 0V4a3 3 0 0 0-3-3Z" />
      <path d="M19 10v2a7 7 0 0 1-14 0v-2" />
      <line x1="12" y1="19" x2="12" y2="23" />
      <line x1="8" y1="23" x2="16" y2="23" />
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
  const speakers = useVoiceStore((s) => s.speakers);
  const speakersStatus = useVoiceStore((s) => s.speakersStatus);
  const speakersError = useVoiceStore((s) => s.speakersError);
  const fetchSpeakers = useVoiceStore((s) => s.fetchSpeakers);
  const createSpeaker = useVoiceStore((s) => s.createSpeaker);
  const updateSpeaker = useVoiceStore((s) => s.updateSpeaker);
  const deleteSpeaker = useVoiceStore((s) => s.deleteSpeaker);

  const [mode, setMode] = useState<FormMode>({ kind: 'idle' });
  const [name, setName] = useState('');
  const [color, setColor] = useState(PRESET_COLORS[0]);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [pendingDeleteId, setPendingDeleteId] = useState<string | null>(null);
  const t = useT();

  useEffect(() => {
    if (speakersStatus === 'idle') void fetchSpeakers();
  }, [speakersStatus, fetchSpeakers]);

  function startAdd() {
    setMode({ kind: 'adding' });
    setName('');
    setColor(PRESET_COLORS[0]);
    setSaveError(null);
  }

  function startEdit(s: Speaker) {
    setMode({ kind: 'editing', speaker: s });
    setName(s.display_name);
    setColor(s.color);
    setSaveError(null);
  }

  async function save() {
    const trimmed = name.trim();
    if (!trimmed) return;
    setSaving(true);
    setSaveError(null);
    try {
      if (mode.kind === 'adding') {
        const created = await createSpeaker({ display_name: trimmed, color });
        // After creating a speaker, drop straight into the voiceprint recorder
        // so the user can read the prompts while the mic is still warm.
        setMode({ kind: 'enrolling', speaker: created });
      } else if (mode.kind === 'editing') {
        await updateSpeaker(mode.speaker.id, { display_name: trimmed, color });
        setMode({ kind: 'idle' });
      }
    } catch (e) {
      setSaveError((e as Error).message);
    } finally {
      setSaving(false);
    }
  }

  async function confirmDelete(id: string) {
    try {
      await deleteSpeaker(id);
      setPendingDeleteId(null);
    } catch (e) {
      console.warn('[Actio] delete speaker failed', e);
      setPendingDeleteId(null);
    }
  }

  const isFormOpen = mode.kind !== 'idle';

  return (
    <div className="people-tab">
      <AnimatePresence mode="wait">
        {mode.kind === 'idle' && (
          <motion.button
            key="add-btn"
            type="button"
            className="primary-button people-tab__add-btn"
            onClick={startAdd}
            initial={{ opacity: 0, scale: 0.9 }}
            animate={{ opacity: 1, scale: 1 }}
            exit={{ opacity: 0, scale: 0.9 }}
            transition={{ duration: 0.15 }}
            whileHover={{ scale: 1.02 }}
            whileTap={{ scale: 0.97 }}
          >
            <PlusIcon />
            {t('people.addPerson')}
          </motion.button>
        )}

        {(mode.kind === 'adding' || mode.kind === 'editing') && (
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
              placeholder={t('people.namePlaceholder')}
              value={name}
              onChange={(e) => setName(e.target.value)}
              autoFocus
              onKeyDown={(e) => {
                if (e.key === 'Enter') void save();
                if (e.key === 'Escape') setMode({ kind: 'idle' });
              }}
            />
            <div
              className="person-form__swatches"
              role="group"
              aria-label={t('people.aria.colorGroup')}
            >
              {PRESET_COLORS.map((c) => (
                <motion.button
                  key={c}
                  type="button"
                  className={`person-form__swatch${color === c ? ' is-selected' : ''}`}
                  style={{ backgroundColor: c }}
                  onClick={() => setColor(c)}
                  aria-label={t('people.aria.swatch', { color: c })}
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
                onClick={() => void save()}
                disabled={!name.trim() || saving}
                whileHover={{ scale: 1.02 }}
                whileTap={{ scale: 0.97 }}
              >
                {saving ? t('people.saving') : t('people.save')}
              </motion.button>
              <motion.button
                type="button"
                className="secondary-button"
                onClick={() => setMode({ kind: 'idle' })}
                whileHover={{ scale: 1.02 }}
                whileTap={{ scale: 0.97 }}
              >
                {t('people.cancel')}
              </motion.button>
            </div>
            {mode.kind === 'adding' && (
              <p className="person-form__hint">{t('people.formHint')}</p>
            )}
            {saveError && <p className="person-form__error">{saveError}</p>}
          </motion.div>
        )}

        {mode.kind === 'enrolling' && (
          <motion.div
            key="enrolling"
            initial={{ opacity: 0, y: -8 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -8 }}
            transition={{ duration: 0.15 }}
          >
            <VoiceprintRecorder
              speakerId={mode.speaker.id}
              speakerName={mode.speaker.display_name}
              onDone={() => setMode({ kind: 'idle' })}
              onCancel={() => setMode({ kind: 'idle' })}
            />
          </motion.div>
        )}
      </AnimatePresence>

      {speakersStatus === 'error' && (
        <div className="people-tab__error" role="alert">
          {t('people.backendRequired')}{' '}
          {speakersError && <span className="people-tab__error-detail">{speakersError}</span>}
          <button type="button" className="secondary-button" onClick={() => void fetchSpeakers()}>
            {t('people.retry')}
          </button>
        </div>
      )}

      <div className="people-tab__list">
        <AnimatePresence>
          {speakersStatus === 'loading' && (
            <motion.p
              key="loading"
              className="people-tab__empty"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
            >
              {t('people.loading')}
            </motion.p>
          )}
          {speakersStatus === 'ready' && speakers.length === 0 && !isFormOpen && (
            <motion.p
              key="empty"
              className="people-tab__empty"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
            >
              {t('people.empty')}
            </motion.p>
          )}
          {speakers.map((speaker, i) => (
            <motion.div
              key={speaker.id}
              className={`person-row${pendingDeleteId === speaker.id ? ' is-confirming-delete' : ''}`}
              variants={personVariants}
              initial="hidden"
              animate="visible"
              exit="exit"
              custom={i}
              layout
            >
              <motion.div
                className="person-row__avatar"
                style={{ backgroundColor: speaker.color }}
                aria-hidden="true"
                layoutId={`avatar-${speaker.id}`}
              >
                {speaker.display_name.charAt(0).toUpperCase()}
              </motion.div>
              {pendingDeleteId === speaker.id ? (
                <>
                  <span className="person-row__confirm-text">
                    {t('people.confirmDelete', { name: speaker.display_name })}
                  </span>
                  <div className="person-row__actions">
                    <motion.button
                      type="button"
                      className="person-row__confirm-delete"
                      onClick={() => void confirmDelete(speaker.id)}
                      whileHover={{ scale: 1.04 }}
                      whileTap={{ scale: 0.96 }}
                    >
                      {t('people.delete')}
                    </motion.button>
                    <motion.button
                      type="button"
                      className="person-row__confirm-cancel"
                      onClick={() => setPendingDeleteId(null)}
                      whileHover={{ scale: 1.04 }}
                      whileTap={{ scale: 0.96 }}
                    >
                      {t('people.cancel')}
                    </motion.button>
                  </div>
                </>
              ) : (
                <>
                  <span className="person-row__name">{speaker.display_name}</span>
                  <div className="person-row__actions">
                    <motion.button
                      type="button"
                      className="person-edit-btn"
                      onClick={() => setMode({ kind: 'enrolling', speaker })}
                      aria-label={t('people.aria.record', { name: speaker.display_name })}
                      title={t('people.tooltip.record')}
                      whileHover={{ scale: 1.15 }}
                      whileTap={{ scale: 0.9 }}
                    >
                      <MicIcon />
                    </motion.button>
                    <motion.button
                      type="button"
                      className="person-edit-btn"
                      onClick={() => startEdit(speaker)}
                      aria-label={t('people.aria.edit', { name: speaker.display_name })}
                      whileHover={{ scale: 1.15 }}
                      whileTap={{ scale: 0.9 }}
                    >
                      <PencilIcon />
                    </motion.button>
                    <motion.button
                      type="button"
                      className="person-delete-btn"
                      onClick={() => setPendingDeleteId(speaker.id)}
                      aria-label={t('people.aria.delete', { name: speaker.display_name })}
                      whileHover={{ scale: 1.15 }}
                      whileTap={{ scale: 0.9 }}
                    >
                      <TrashIcon />
                    </motion.button>
                  </div>
                </>
              )}
            </motion.div>
          ))}
        </AnimatePresence>
      </div>

      <PendingVoicesPanel />
    </div>
  );
}
