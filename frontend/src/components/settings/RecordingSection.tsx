import { useVoiceStore } from '../../store/use-voice-store';
import type { ClipInterval } from '../../store/use-voice-store';
import { useT, type TKey } from '../../i18n';

const INTERVAL_OPTIONS: { value: ClipInterval; labelKey: TKey }[] = [
  { value: 1, labelKey: 'settings.recording.interval.min1' },
  { value: 2, labelKey: 'settings.recording.interval.min2' },
  { value: 5, labelKey: 'settings.recording.interval.min5' },
  { value: 10, labelKey: 'settings.recording.interval.min10' },
  { value: 30, labelKey: 'settings.recording.interval.min30' },
];

export function RecordingSection() {
  const clipInterval = useVoiceStore((s) => s.clipInterval);
  const setClipInterval = useVoiceStore((s) => s.setClipInterval);
  const t = useT();

  return (
    <div className="settings-section">
      <h3 className="settings-section__title">{t('settings.recording.title')}</h3>
      <label className="settings-row">
        <span className="settings-row__label">{t('settings.recording.autoClip')}</span>
        <select
          className="settings-row__select"
          value={clipInterval}
          onChange={(e) => setClipInterval(Number(e.target.value) as ClipInterval)}
        >
          {INTERVAL_OPTIONS.map((opt) => (
            <option key={opt.value} value={opt.value}>
              {t(opt.labelKey)}
            </option>
          ))}
        </select>
      </label>
    </div>
  );
}
