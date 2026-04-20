import { useEffect, useState } from 'react';

const API_BASE = 'http://127.0.0.1:3000';

interface AudioDeviceInfo {
  name: string;
  is_default: boolean;
}

interface AudioSettingsShape {
  device_name?: string;
  speaker_confirm_threshold?: number;
  speaker_tentative_threshold?: number;
  speaker_min_duration_ms?: number;
  speaker_continuity_window_ms?: number;
}

async function fetchDevices(): Promise<AudioDeviceInfo[]> {
  const res = await fetch(`${API_BASE}/settings/audio-devices`);
  if (!res.ok) throw new Error('Failed to fetch audio devices');
  return res.json();
}

async function fetchSettings(): Promise<{ audio?: AudioSettingsShape }> {
  const res = await fetch(`${API_BASE}/settings`);
  if (!res.ok) throw new Error('Failed to fetch settings');
  return res.json();
}

async function patchAudio(patch: Partial<AudioSettingsShape>): Promise<void> {
  const res = await fetch(`${API_BASE}/settings`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ audio: patch }),
  });
  if (!res.ok) throw new Error('Failed to save audio setting');
}

export function AudioSettings() {
  const [devices, setDevices] = useState<AudioDeviceInfo[]>([]);
  const [selected, setSelected] = useState('');
  const [confirmT, setConfirmT] = useState(0.55);
  const [tentativeT, setTentativeT] = useState(0.4);
  const [minMs, setMinMs] = useState(1500);
  const [continuityMs, setContinuityMs] = useState(15000);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    Promise.all([fetchDevices(), fetchSettings()])
      .then(([devs, settings]) => {
        setDevices(devs);
        const saved = settings.audio?.device_name;
        if (saved) {
          setSelected(saved);
        } else {
          const def = devs.find((d) => d.is_default);
          if (def) setSelected(def.name);
        }
        if (typeof settings.audio?.speaker_confirm_threshold === 'number') {
          setConfirmT(settings.audio.speaker_confirm_threshold);
        }
        if (typeof settings.audio?.speaker_tentative_threshold === 'number') {
          setTentativeT(settings.audio.speaker_tentative_threshold);
        }
        if (typeof settings.audio?.speaker_min_duration_ms === 'number') {
          setMinMs(settings.audio.speaker_min_duration_ms);
        }
        if (typeof settings.audio?.speaker_continuity_window_ms === 'number') {
          setContinuityMs(settings.audio.speaker_continuity_window_ms);
        }
      })
      .catch(() => {});
  }, []);

  const handleDeviceChange = async (name: string) => {
    setSelected(name);
    setError(null);
    try {
      await patchAudio({ device_name: name });
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to save');
    }
  };

  // Slider commits fire on release rather than on every pixel to avoid
  // PATCH-flooding during a drag.
  const commit = async <K extends keyof AudioSettingsShape>(
    key: K,
    v: AudioSettingsShape[K],
  ) => {
    setError(null);
    try {
      await patchAudio({ [key]: v } as Partial<AudioSettingsShape>);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to save');
    }
  };

  return (
    <section className="settings-section">
      <div className="settings-section__title">Audio Input</div>

      <label className="settings-row">
        <span className="settings-row__label">Microphone</span>
        <select
          className="settings-row__select"
          value={selected}
          onChange={(e) => handleDeviceChange(e.target.value)}
          disabled={devices.length === 0}
        >
          {devices.length === 0 && <option value="">No devices found</option>}
          {devices.map((d) => (
            <option key={d.name} value={d.name}>
              {d.name}
              {d.is_default ? ' (default)' : ''}
            </option>
          ))}
        </select>
      </label>

      <div className="settings-section__title" style={{ marginTop: 20 }}>
        Speaker Recognition
      </div>
      <p className="settings-field__hint" style={{ margin: '0 0 10px' }}>
        How confident the app must be before labelling a transcript segment
        with a known speaker. Tentative matches show a <b>?</b> badge; below
        the tentative threshold the segment is left unattributed. Continuity
        window lets a recent confirmed speaker inherit subsequent weak
        segments so one speech turn renders under one speaker. Changes take
        effect on the next pipeline restart.
      </p>

      <label className="settings-row">
        <span className="settings-row__label">
          Confirm threshold <code>{confirmT.toFixed(2)}</code>
        </span>
        <input
          type="range"
          min="0.2"
          max="0.9"
          step="0.01"
          value={confirmT}
          onChange={(e) => setConfirmT(parseFloat(e.target.value))}
          onMouseUp={() => void commit('speaker_confirm_threshold', confirmT)}
          onTouchEnd={() => void commit('speaker_confirm_threshold', confirmT)}
          onBlur={() => void commit('speaker_confirm_threshold', confirmT)}
        />
      </label>

      <label className="settings-row">
        <span className="settings-row__label">
          Tentative threshold <code>{tentativeT.toFixed(2)}</code>
        </span>
        <input
          type="range"
          min="0.1"
          max={Math.min(confirmT, 0.8)}
          step="0.01"
          value={tentativeT}
          onChange={(e) => setTentativeT(parseFloat(e.target.value))}
          onMouseUp={() => void commit('speaker_tentative_threshold', tentativeT)}
          onTouchEnd={() => void commit('speaker_tentative_threshold', tentativeT)}
          onBlur={() => void commit('speaker_tentative_threshold', tentativeT)}
        />
      </label>

      <label className="settings-row">
        <span className="settings-row__label">
          Min speech duration <code>{minMs} ms</code>
        </span>
        <input
          type="range"
          min="500"
          max="4000"
          step="100"
          value={minMs}
          onChange={(e) => setMinMs(parseInt(e.target.value, 10))}
          onMouseUp={() => void commit('speaker_min_duration_ms', minMs)}
          onTouchEnd={() => void commit('speaker_min_duration_ms', minMs)}
          onBlur={() => void commit('speaker_min_duration_ms', minMs)}
        />
      </label>

      <label className="settings-row">
        <span className="settings-row__label">
          Continuity window{' '}
          <code>
            {continuityMs === 0 ? 'Off' : `${Math.round(continuityMs / 1000)} s`}
          </code>
        </span>
        <input
          type="range"
          min="0"
          max="60000"
          step="1000"
          value={continuityMs}
          onChange={(e) => setContinuityMs(parseInt(e.target.value, 10))}
          onMouseUp={() => void commit('speaker_continuity_window_ms', continuityMs)}
          onTouchEnd={() => void commit('speaker_continuity_window_ms', continuityMs)}
          onBlur={() => void commit('speaker_continuity_window_ms', continuityMs)}
        />
      </label>

      {error && (
        <div
          className="settings-row__sublabel"
          style={{ color: 'var(--color-priority-high-text)' }}
        >
          {error}
        </div>
      )}
    </section>
  );
}
