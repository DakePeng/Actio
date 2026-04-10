import { useVoiceStore } from '../../store/use-voice-store';
import type { ClipInterval } from '../../store/use-voice-store';

const INTERVAL_OPTIONS: { value: ClipInterval; label: string }[] = [
  { value: 1, label: '1 minute' },
  { value: 2, label: '2 minutes' },
  { value: 5, label: '5 minutes' },
  { value: 10, label: '10 minutes' },
  { value: 30, label: '30 minutes' },
];

export function RecordingSection() {
  const clipInterval = useVoiceStore((s) => s.clipInterval);
  const setClipInterval = useVoiceStore((s) => s.setClipInterval);

  return (
    <div className="settings-section">
      <h3 className="settings-section__title">Recording</h3>
      <label className="settings-row">
        <span className="settings-row__label">Auto-clip interval</span>
        <select
          className="settings-row__select"
          value={clipInterval}
          onChange={(e) => setClipInterval(Number(e.target.value) as ClipInterval)}
        >
          {INTERVAL_OPTIONS.map((opt) => (
            <option key={opt.value} value={opt.value}>
              {opt.label}
            </option>
          ))}
        </select>
      </label>
    </div>
  );
}
