import { useEffect, useRef, useState } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import * as speakerApi from '../api/speakers';
import { useVoiceStore } from '../store/use-voice-store';
import type { LiveEnrollmentState } from '../types/speaker';
import { useLanguage, useT, type TKey } from '../i18n';

// Longer, more varied passages give the embedding model a better chance to
// capture a speaker's prosody. Five clips × ~5 seconds each lands around
// the 25 s total that the 3D-Speaker family typically recommends.
const PASSAGES_EN = [
  'The quick brown fox jumps over the lazy dog, and then sits down for a long rest.',
  'She sells seashells by the seashore under a clear blue sky on a warm summer afternoon.',
  'A journey of a thousand miles begins with a single step, though most journeys are rarely that simple.',
  'Peter Piper picked a peck of pickled peppers, and the whole kitchen smelled like vinegar for days.',
  'How much wood would a woodchuck chuck if a woodchuck could chuck wood all afternoon?',
];

// Each passage covers all four tones + neutral, mixed initials (zh/ch/sh
// vs z/c/s vs p/t/k), and varied prosody (declarative, conditional,
// descriptive). Length targets ~5s read-aloud so the 5-clip enrollment
// totals around 25s, matching the English set.
const PASSAGES_ZH_CN = [
  '春天的风轻轻吹过湖面，岸边的柳树摇晃着细长的枝条，仿佛在和水里的鱼儿打招呼。',
  '昨天下午三点，我在图书馆二楼找到了那本关于宋代绘画的书，内容比想象中还要精彩。',
  '他说话的声音不大不小，语速也很平稳，让人听着觉得特别舒服，不知不觉就记住了要点。',
  '如果明天早晨六点钟还没有下雨，我们就一起去山脚下的小路上跑步，大概四十分钟就能回来。',
  '小猫跳到窗台上，圆圆的眼睛盯着外面飞来飞去的麻雀，尾巴一下一下有节奏地轻轻摆动。',
];

// Bilingual set for users who code-switch. Each line mixes Chinese and
// English naturally so the embedding captures both phonetic inventories.
// The speaker model averages across all five clips — still one voiceprint
// per user, just with broader coverage for zh/en speakers.
const PASSAGES_MIXED = [
  "Let me quickly check the schedule — 我们下午三点在 meeting room 见面，好吗？",
  'I just finished reading that book — 里面提到的观点非常 interesting，值得再读一遍。',
  'Can you send me the report? 我明天早上 before work 需要把这份资料看一遍。',
  'She said the new app is surprisingly good — 用起来比想象中 smooth 很多。',
  'A friend asked about it yesterday — 我告诉他可以直接在 official website 下载。',
];

type PassageSet = 'en' | 'zh' | 'mixed';

const PASSAGE_SETS: Record<PassageSet, string[]> = {
  en: PASSAGES_EN,
  zh: PASSAGES_ZH_CN,
  mixed: PASSAGES_MIXED,
};

const TARGET = 5;
// Poll fast while active so the meter feels responsive; slow down otherwise.
const POLL_MS_ACTIVE = 200;
const POLL_MS_IDLE = 700;
// Hold the success screen long enough to be readable before dismissing.
const SUCCESS_HOLD_MS = 1800;

// Rough ceiling for normal speech RMS. Normalises the meter bar to 0..1.
const METER_CEILING = 0.25;

const REJECTION_KEYS: Record<string, TKey> = {
  too_short: 'voiceprint.rejection.tooShort',
  too_long: 'voiceprint.rejection.tooLong',
  low_quality: 'voiceprint.rejection.lowQuality',
};

export function VoiceprintRecorder({
  speakerId,
  speakerName,
  onDone,
  onCancel,
}: {
  speakerId: string;
  speakerName: string;
  onDone: () => void;
  onCancel: () => void;
}) {
  const [state, setState] = useState<LiveEnrollmentState | null>(null);
  const [error, setError] = useState<string | null>(null);
  const fetchSpeakers = useVoiceStore((s) => s.fetchSpeakers);
  const t = useT();
  const { lang } = useLanguage();
  const [passageSet, setPassageSet] = useState<PassageSet>(
    lang === 'zh-CN' ? 'zh' : 'en',
  );
  const passages = PASSAGE_SETS[passageSet];

  // The cleanup path must not cancel the backend session when enrollment
  // has already concluded naturally (Complete) or been explicitly cancelled
  // by the user (Cancelled) — otherwise React 18 StrictMode's double-mount
  // and the normal Complete → onDone → unmount sequence both fire a spurious
  // cancelLiveEnrollment that tears the pipeline down right as the next
  // action (fetchSpeakers, re-enroll) starts. Keep the latest status in a
  // ref so the mount effect's cleanup can read it without re-subscribing.
  const statusRef = useRef<LiveEnrollmentState['status'] | null>(null);
  statusRef.current = state?.status ?? null;

  useEffect(() => {
    let mounted = true;
    (async () => {
      try {
        const s = await speakerApi.startLiveEnrollment(speakerId, TARGET);
        if (!mounted) return;
        setState(s);
      } catch (e) {
        if (!mounted) return;
        setError((e as Error).message);
      }
    })();
    return () => {
      mounted = false;
      // Only cancel if still active. If the session already reached Complete
      // or the user hit Cancel, the backend is already torn down.
      if (statusRef.current === 'active' || statusRef.current === null) {
        speakerApi.cancelLiveEnrollment(speakerId).catch(() => {});
      }
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [speakerId]);

  // Polling. Depends only on `status` so the interval isn't torn down and
  // rebuilt on every tick. The tick body reads the status through a ref.
  const pollingStatusRef = useRef<LiveEnrollmentState['status'] | null>(null);
  pollingStatusRef.current = state?.status ?? null;
  useEffect(() => {
    const currentStatus = state?.status;
    if (!currentStatus) return;
    // Stop polling once the session leaves Active — no further meaningful
    // state changes will happen, and the close-out effect handles dismissal.
    if (currentStatus !== 'active') return;

    const interval = POLL_MS_ACTIVE;
    const idleInterval = POLL_MS_IDLE;
    const chosen = pollingStatusRef.current === 'active' ? interval : idleInterval;
    const tick = async () => {
      try {
        const s = await speakerApi.getLiveEnrollmentStatus();
        if (s) setState(s);
      } catch {
        /* keep polling — transient errors are fine */
      }
    };
    const timer = window.setInterval(() => void tick(), chosen);
    return () => window.clearInterval(timer);
  }, [state?.status]);

  // One-shot effect: when status first transitions to `complete`, refresh
  // the speaker list and schedule onDone exactly once. Keying on a boolean
  // means React won't re-run the effect on subsequent state updates (late
  // polls), so the setTimeout is never cleared-then-skipped and onDone
  // always fires.
  const isComplete = state?.status === 'complete';
  useEffect(() => {
    if (!isComplete) return;
    void fetchSpeakers();
    const id = window.setTimeout(onDone, SUCCESS_HOLD_MS);
    return () => window.clearTimeout(id);
  }, [isComplete, fetchSpeakers, onDone]);

  const captured = state?.captured ?? 0;
  const target = state?.target ?? TARGET;
  const currentPassage = passages[Math.min(captured, passages.length - 1)];
  const done = captured >= target;
  const isActive = state?.status === 'active';
  const level = state?.rms_level ?? 0;
  const meterPct = Math.min(1, level / METER_CEILING) * 100;
  // Generous cutoff so even quiet breathing nudges the dot past idle.
  const hearing = isActive && level > 0.005;
  const rejectionKey =
    state?.last_rejected_reason && REJECTION_KEYS[state.last_rejected_reason];
  const rejectionHint = rejectionKey ? t(rejectionKey) : null;

  if (done) {
    return (
      <div className="voiceprint-recorder voiceprint-recorder--success">
        <motion.div
          className="voiceprint-recorder__success-check"
          initial={{ scale: 0, opacity: 0 }}
          animate={{ scale: 1, opacity: 1 }}
          transition={{ type: 'spring', stiffness: 320, damping: 18 }}
          aria-hidden="true"
        >
          <svg viewBox="0 0 24 24" width="48" height="48" fill="none">
            <motion.circle
              cx="12"
              cy="12"
              r="11"
              fill="#22c55e"
              initial={{ scale: 0.3 }}
              animate={{ scale: 1 }}
              transition={{ type: 'spring', stiffness: 260, damping: 20 }}
            />
            <motion.path
              d="M7.5 12.5l3 3 6-6.5"
              stroke="white"
              strokeWidth="2.5"
              strokeLinecap="round"
              strokeLinejoin="round"
              initial={{ pathLength: 0 }}
              animate={{ pathLength: 1 }}
              transition={{ duration: 0.45, ease: 'easeOut', delay: 0.15 }}
            />
          </svg>
        </motion.div>
        <motion.h3
          className="voiceprint-recorder__success-title"
          initial={{ opacity: 0, y: 6 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.25, delay: 0.1 }}
        >
          {t('voiceprint.success.title', { name: speakerName })}
        </motion.h3>
        <motion.p
          className="voiceprint-recorder__success-sub"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          transition={{ duration: 0.25, delay: 0.25 }}
        >
          {t('voiceprint.success.sub')}
        </motion.p>
      </div>
    );
  }

  return (
    <div className="voiceprint-recorder">
      <h3 className="voiceprint-recorder__title">
        {t('voiceprint.title', { name: speakerName })}
      </h3>

      {isActive && !error && captured === 0 && (
        <div
          className="voiceprint-recorder__passage-switcher"
          role="group"
          aria-label={t('voiceprint.passageSet.label')}
        >
          {(['en', 'zh', 'mixed'] as const).map((k) => (
            <button
              key={k}
              type="button"
              className={`voiceprint-recorder__passage-chip${
                passageSet === k ? ' is-active' : ''
              }`}
              onClick={() => setPassageSet(k)}
            >
              {t(
                k === 'en'
                  ? 'voiceprint.passageSet.en'
                  : k === 'zh'
                    ? 'voiceprint.passageSet.zh'
                    : 'voiceprint.passageSet.mixed',
              )}
            </button>
          ))}
        </div>
      )}

      {isActive && !error && (
        <div className="voiceprint-recorder__passage-block">
          <p className="voiceprint-recorder__hint">{t('voiceprint.readHint')}</p>
          <p className="voiceprint-recorder__passage">“{currentPassage}”</p>
        </div>
      )}

      {!state && !error && (
        <p className="voiceprint-recorder__hint">{t('voiceprint.arming')}</p>
      )}

      {isActive && !error && (
        <div
          className="voiceprint-recorder__meter"
          aria-label={t('voiceprint.aria.meter')}
        >
          <div className="voiceprint-recorder__meter-label">
            <motion.span
              className={`voiceprint-recorder__dot${hearing ? ' is-hearing' : ''}`}
              animate={hearing ? { scale: [1, 1.25, 1] } : { scale: 1 }}
              transition={
                hearing
                  ? { duration: 0.9, repeat: Infinity, ease: 'easeInOut' }
                  : { duration: 0.2 }
              }
              aria-hidden="true"
            />
            <span>
              {hearing ? t('voiceprint.listening') : t('voiceprint.waitingSound')}
            </span>
          </div>
          <div className="voiceprint-recorder__meter-track">
            <motion.div
              className="voiceprint-recorder__meter-fill"
              animate={{ width: `${meterPct}%` }}
              transition={{ type: 'tween', duration: 0.12, ease: 'linear' }}
            />
          </div>
        </div>
      )}

      <AnimatePresence>
        {isActive && rejectionHint && (
          <motion.p
            key={`${state?.version}-rejection`}
            className="voiceprint-recorder__rejection"
            initial={{ opacity: 0, y: -4 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -4 }}
            transition={{ duration: 0.2 }}
          >
            {rejectionHint}
          </motion.p>
        )}
      </AnimatePresence>

      <div
        className="voiceprint-recorder__captured"
        aria-label={t('voiceprint.aria.captured', { captured, target })}
      >
        {Array.from({ length: target }).map((_, i) => (
          <motion.span
            key={i}
            className={`voiceprint-recorder__chip${i < captured ? ' is-done' : ''}`}
            animate={
              i === captured
                ? { scale: [1, 1.12, 1], opacity: [0.7, 1, 0.7] }
                : { scale: 1, opacity: 1 }
            }
            transition={
              i === captured
                ? { duration: 1.3, repeat: Infinity, ease: 'easeInOut' }
                : { duration: 0.2 }
            }
          >
            {i < captured ? '✓' : '·'}
          </motion.span>
        ))}
      </div>

      {error && <p className="voiceprint-recorder__error">{error}</p>}
      {state?.status === 'cancelled' && (
        <p className="voiceprint-recorder__error">{t('voiceprint.cancelled')}</p>
      )}

      <div className="voiceprint-recorder__actions">
        <button type="button" className="secondary-button" onClick={onCancel}>
          {t('voiceprint.cancel')}
        </button>
      </div>
    </div>
  );
}
