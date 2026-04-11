import { useEffect, useState } from 'react';

const API_BASE = 'http://127.0.0.1:3000';

interface AudioDeviceInfo {
  name: string;
  is_default: boolean;
}

async function fetchDevices(): Promise<AudioDeviceInfo[]> {
  const res = await fetch(`${API_BASE}/settings/audio-devices`);
  if (!res.ok) throw new Error('Failed to fetch audio devices');
  return res.json();
}

async function fetchSettings(): Promise<{ audio?: { device_name?: string } }> {
  const res = await fetch(`${API_BASE}/settings`);
  if (!res.ok) throw new Error('Failed to fetch settings');
  return res.json();
}

async function patchAudioDevice(device_name: string): Promise<void> {
  const res = await fetch(`${API_BASE}/settings`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ audio: { device_name } }),
  });
  if (!res.ok) throw new Error('Failed to save audio device');
}

export function AudioSettings() {
  const [devices, setDevices] = useState<AudioDeviceInfo[]>([]);
  const [selected, setSelected] = useState('');
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
      })
      .catch(() => {});
  }, []);

  const handleChange = async (name: string) => {
    setSelected(name);
    setError(null);
    try {
      await patchAudioDevice(name);
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
          onChange={(e) => handleChange(e.target.value)}
          disabled={devices.length === 0}
        >
          {devices.length === 0 && (
            <option value="">No devices found</option>
          )}
          {devices.map((d) => (
            <option key={d.name} value={d.name}>
              {d.name}{d.is_default ? ' (default)' : ''}
            </option>
          ))}
        </select>
      </label>

      {error && (
        <div className="settings-row__sublabel" style={{ color: 'var(--color-priority-high-text)' }}>
          {error}
        </div>
      )}
    </section>
  );
}
