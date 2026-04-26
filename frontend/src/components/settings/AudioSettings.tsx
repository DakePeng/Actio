import { useEffect, useState } from 'react';
import { useT } from '../../i18n';
import { getApiUrl } from '../../api/backend-url';

interface AudioDeviceInfo {
  name: string;
  is_default: boolean;
}

interface AsrModelOption {
  id: string;
  name: string;
  languages: string;
  streaming: boolean;
  downloaded: boolean;
}

interface AudioSettingsShape {
  device_name?: string;
  live_asr_model?: string | null;
  archive_asr_model?: string | null;
  speaker_confirm_threshold?: number;
  speaker_tentative_threshold?: number;
  speaker_min_duration_ms?: number;
  speaker_continuity_window_ms?: number;
  always_listening?: boolean;
  window_length_ms?: number;
  window_step_ms?: number;
  extraction_tick_secs?: number;
  // Batch clip processing knobs (Plan Task 2 of batch-clip-processing).
  clip_target_secs?: number;
  cluster_cosine_threshold?: number;
  audio_retention_days?: number;
  provisional_voiceprint_gc_days?: number;
  use_batch_pipeline?: boolean;
}

async function fetchDevices(): Promise<AudioDeviceInfo[]> {
  const res = await fetch(await getApiUrl('/settings/audio-devices'));
  if (!res.ok) throw new Error('Failed to fetch audio devices');
  return res.json();
}

async function fetchSettings(): Promise<{ audio?: AudioSettingsShape }> {
  const res = await fetch(await getApiUrl('/settings'));
  if (!res.ok) throw new Error('Failed to fetch settings');
  return res.json();
}

async function patchAudio(patch: Partial<AudioSettingsShape>): Promise<void> {
  const res = await fetch(await getApiUrl('/settings'), {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ audio: patch }),
  });
  if (!res.ok) throw new Error('Failed to save audio setting');
}

export function AudioSettings() {
  const t = useT();
  const [devices, setDevices] = useState<AudioDeviceInfo[]>([]);
  const [selected, setSelected] = useState('');
  const [confirmT, setConfirmT] = useState(0.55);
  const [tentativeT, setTentativeT] = useState(0.4);
  const [minMs, setMinMs] = useState(1500);
  const [continuityMs, setContinuityMs] = useState(15000);
  // Always-listening + windowed extractor params.
  const [alwaysListening, setAlwaysListening] = useState(true);
  const [windowLengthMs, setWindowLengthMs] = useState(5 * 60 * 1000);
  const [windowStepMs, setWindowStepMs] = useState(4 * 60 * 1000);
  const [extractionTickSecs, setExtractionTickSecs] = useState(60);
  // Batch clip processing knobs.
  const [clipTargetSecs, setClipTargetSecs] = useState(300);
  const [clusterCosThreshold, setClusterCosThreshold] = useState(0.4);
  const [audioRetentionDays, setAudioRetentionDays] = useState(14);
  const [provisionalGcDays, setProvisionalGcDays] = useState(30);
  const [useBatchPipeline, setUseBatchPipeline] = useState(false);
  const [models, setModels] = useState<AsrModelOption[]>([]);
  const [liveModel, setLiveModel] = useState<string>('');
  const [archiveModel, setArchiveModel] = useState<string>('');
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void (async () => {
      try {
        const r = await fetch(await getApiUrl('/settings/models/available'));
        const rows: AsrModelOption[] = r.ok ? await r.json() : [];
        setModels(rows);
      } catch {
        setModels([]);
      }
    })();
  }, []);

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
        if (typeof settings.audio?.always_listening === 'boolean') {
          setAlwaysListening(settings.audio.always_listening);
        }
        if (typeof settings.audio?.window_length_ms === 'number') {
          setWindowLengthMs(settings.audio.window_length_ms);
        }
        if (typeof settings.audio?.window_step_ms === 'number') {
          setWindowStepMs(settings.audio.window_step_ms);
        }
        if (typeof settings.audio?.extraction_tick_secs === 'number') {
          setExtractionTickSecs(settings.audio.extraction_tick_secs);
        }
        if (typeof settings.audio?.clip_target_secs === 'number') {
          setClipTargetSecs(settings.audio.clip_target_secs);
        }
        if (typeof settings.audio?.cluster_cosine_threshold === 'number') {
          setClusterCosThreshold(settings.audio.cluster_cosine_threshold);
        }
        if (typeof settings.audio?.audio_retention_days === 'number') {
          setAudioRetentionDays(settings.audio.audio_retention_days);
        }
        if (typeof settings.audio?.provisional_voiceprint_gc_days === 'number') {
          setProvisionalGcDays(settings.audio.provisional_voiceprint_gc_days);
        }
        if (typeof settings.audio?.use_batch_pipeline === 'boolean') {
          setUseBatchPipeline(settings.audio.use_batch_pipeline);
        }
        if (typeof settings.audio?.live_asr_model === 'string') {
          setLiveModel(settings.audio.live_asr_model);
        }
        if (typeof settings.audio?.archive_asr_model === 'string') {
          setArchiveModel(settings.audio.archive_asr_model);
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
      setError(e instanceof Error ? e.message : t('settings.audio.saveFailed'));
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
      setError(e instanceof Error ? e.message : t('settings.audio.saveFailed'));
    }
  };

  return (
    <section className="settings-section">
      <div className="settings-section__title">{t('settings.audio.inputTitle')}</div>

      <label className="settings-row">
        <span className="settings-row__label">{t('settings.audio.microphone')}</span>
        <select
          className="settings-row__select"
          value={selected}
          onChange={(e) => handleDeviceChange(e.target.value)}
          disabled={devices.length === 0}
        >
          {devices.length === 0 && (
            <option value="">{t('settings.audio.noDevices')}</option>
          )}
          {devices.map((d) => (
            <option key={d.name} value={d.name}>
              {d.name}
              {d.is_default ? t('settings.audio.defaultSuffix') : ''}
            </option>
          ))}
        </select>
      </label>

      <div className="settings-section__title" style={{ marginTop: 20 }}>
        {t('settings.audio.speakerTitle')}
      </div>
      <p className="settings-field__hint" style={{ margin: '0 0 10px' }}>
        {t('settings.audio.speakerHint')}
      </p>

      <label className="settings-row">
        <span className="settings-row__label">
          {t('settings.audio.confirmThreshold')} <code>{confirmT.toFixed(2)}</code>
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
          {t('settings.audio.tentativeThreshold')} <code>{tentativeT.toFixed(2)}</code>
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
          {t('settings.audio.minSpeechDuration')} <code>{minMs} ms</code>
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
          {t('settings.audio.continuityWindow')}{' '}
          <code>
            {continuityMs === 0
              ? t('settings.audio.continuityOff')
              : `${Math.round(continuityMs / 1000)} s`}
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

      <div className="settings-section__title" style={{ marginTop: 20 }}>
        {t('settings.audio.extractionTitle')}
      </div>
      <p className="settings-field__hint" style={{ margin: '0 0 10px' }}>
        {t('settings.audio.extractionHint')}
      </p>

      <label className="settings-row">
        <span className="settings-row__label">
          {t('settings.audio.alwaysListening')}
        </span>
        <input
          type="checkbox"
          className="settings-check"
          role="switch"
          aria-checked={alwaysListening}
          checked={alwaysListening}
          onChange={(e) => {
            setAlwaysListening(e.target.checked);
            void commit('always_listening', e.target.checked);
          }}
        />
      </label>
      <p className="settings-field__hint" style={{ margin: '0 0 10px' }}>
        {t('settings.audio.alwaysListeningHint')}
      </p>

      {useBatchPipeline && (
        <p
          className="settings-field__hint"
          style={{ margin: '0 0 10px', fontStyle: 'italic' }}
        >
          {t('settings.audio.legacyOnlyHint')}
        </p>
      )}

      <label className="settings-row">
        <span className="settings-row__label">
          {t('settings.audio.windowLength')}{' '}
          <code>{t('settings.audio.minutes', { n: Math.round(windowLengthMs / 60000) })}</code>
        </span>
        <input
          type="range"
          min={60000}
          max={15 * 60 * 1000}
          step={60000}
          value={windowLengthMs}
          onChange={(e) => setWindowLengthMs(parseInt(e.target.value, 10))}
          onMouseUp={() => {
            // Step must not exceed length — clamp both before committing so
            // the backend doesn't receive an inconsistent pair.
            const len = windowLengthMs;
            const step = Math.min(windowStepMs, len);
            setWindowStepMs(step);
            void commit('window_length_ms', len);
            void commit('window_step_ms', step);
          }}
        />
      </label>

      <label className="settings-row">
        <span className="settings-row__label">
          {t('settings.audio.windowStep')}{' '}
          <code>{t('settings.audio.minutes', { n: Math.round(windowStepMs / 60000) })}</code>
        </span>
        <input
          type="range"
          min={30000}
          max={windowLengthMs}
          step={30000}
          value={Math.min(windowStepMs, windowLengthMs)}
          onChange={(e) => setWindowStepMs(parseInt(e.target.value, 10))}
          onMouseUp={() =>
            void commit('window_step_ms', Math.min(windowStepMs, windowLengthMs))
          }
        />
      </label>

      <label className="settings-row">
        <span className="settings-row__label">
          {t('settings.audio.extractionTick')}{' '}
          <code>{t('settings.audio.seconds', { n: extractionTickSecs })}</code>
        </span>
        <input
          type="range"
          min={10}
          max={300}
          step={5}
          value={extractionTickSecs}
          onChange={(e) => setExtractionTickSecs(parseInt(e.target.value, 10))}
          onMouseUp={() => void commit('extraction_tick_secs', extractionTickSecs)}
        />
      </label>

      <div className="settings-section__title" style={{ marginTop: 20 }}>
        {t('settings.audio.batchTitle')}
      </div>
      <p className="settings-field__hint" style={{ margin: '0 0 10px' }}>
        {t('settings.audio.batchHint')}
      </p>

      <label className="settings-row">
        <span className="settings-row__label">
          {t('settings.audio.useBatchPipeline')}
        </span>
        <input
          type="checkbox"
          className="settings-check"
          role="switch"
          aria-checked={useBatchPipeline}
          checked={useBatchPipeline}
          onChange={(e) => {
            setUseBatchPipeline(e.target.checked);
            void commit('use_batch_pipeline', e.target.checked);
          }}
        />
      </label>
      <p className="settings-field__hint" style={{ margin: '0 0 10px' }}>
        {t('settings.audio.useBatchPipelineHint')}
      </p>

      <label className="settings-row">
        <span className="settings-row__label">
          {t('settings.audio.liveAsrModel')}
        </span>
        <select
          className="settings-row__select"
          value={liveModel}
          onChange={(e) => {
            setLiveModel(e.target.value);
            void commit('live_asr_model', e.target.value || null);
          }}
        >
          <option value="">
            {t('settings.audio.liveAsrFallback')}
          </option>
          {models
            .filter((m) => m.downloaded)
            .map((m) => (
              <option key={m.id} value={m.id}>
                {m.name} ·{' '}
                {m.streaming
                  ? t('settings.audio.streamingTag')
                  : t('settings.audio.offlineTag')}{' '}
                · {m.languages}
              </option>
            ))}
        </select>
      </label>
      <p className="settings-field__hint" style={{ margin: '0 0 10px' }}>
        {t('settings.audio.liveAsrModelHint')}
      </p>

      <label className="settings-row">
        <span className="settings-row__label">
          {t('settings.audio.archiveAsrModel')}
        </span>
        <select
          className="settings-row__select"
          value={archiveModel}
          onChange={(e) => {
            setArchiveModel(e.target.value);
            void commit('archive_asr_model', e.target.value || null);
          }}
        >
          <option value="">
            {t('settings.audio.archiveAsrFallback')}
          </option>
          {models
            .filter((m) => m.downloaded && !m.streaming)
            .map((m) => (
              <option key={m.id} value={m.id}>
                {m.name} · {m.languages}
              </option>
            ))}
        </select>
      </label>
      <p className="settings-field__hint" style={{ margin: '0 0 10px' }}>
        {t('settings.audio.archiveAsrModelHint')}
      </p>

      <label className="settings-row">
        <span className="settings-row__label">
          {t('settings.audio.clipTarget')}{' '}
          <code>
            {t('settings.audio.minutes', {
              n: Math.round(clipTargetSecs / 60),
            })}
          </code>
        </span>
        <input
          type="range"
          min={60}
          max={600}
          step={30}
          value={clipTargetSecs}
          onChange={(e) => setClipTargetSecs(parseInt(e.target.value, 10))}
          onMouseUp={() => void commit('clip_target_secs', clipTargetSecs)}
        />
      </label>

      <label className="settings-row">
        <span className="settings-row__label">
          {t('settings.audio.clusterThreshold')}{' '}
          <code>{clusterCosThreshold.toFixed(2)}</code>
        </span>
        <input
          type="range"
          min={0.2}
          max={0.7}
          step={0.01}
          value={clusterCosThreshold}
          onChange={(e) =>
            setClusterCosThreshold(parseFloat(e.target.value))
          }
          onMouseUp={() =>
            void commit('cluster_cosine_threshold', clusterCosThreshold)
          }
        />
      </label>

      <label className="settings-row">
        <span className="settings-row__label">
          {t('settings.audio.audioRetention')}{' '}
          <code>{t('settings.audio.days', { n: audioRetentionDays })}</code>
        </span>
        <input
          type="range"
          min={1}
          max={60}
          step={1}
          value={audioRetentionDays}
          onChange={(e) => setAudioRetentionDays(parseInt(e.target.value, 10))}
          onMouseUp={() =>
            void commit('audio_retention_days', audioRetentionDays)
          }
        />
      </label>

      <label className="settings-row">
        <span className="settings-row__label">
          {t('settings.audio.provisionalGc')}{' '}
          <code>{t('settings.audio.days', { n: provisionalGcDays })}</code>
        </span>
        <input
          type="range"
          min={7}
          max={180}
          step={1}
          value={provisionalGcDays}
          onChange={(e) => setProvisionalGcDays(parseInt(e.target.value, 10))}
          onMouseUp={() =>
            void commit('provisional_voiceprint_gc_days', provisionalGcDays)
          }
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
