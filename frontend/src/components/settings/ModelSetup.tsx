import { useEffect, useState, useCallback } from 'react';

type DownloadTarget =
  | { type: 'shared' }
  | { type: 'model'; id: string };

interface ModelStatus {
  state: 'not_downloaded' | 'downloading' | 'ready' | 'error';
  target?: DownloadTarget;
  progress?: number;
  current_file?: string;
  message?: string;
}

interface AsrModelInfo {
  id: string;
  name: string;
  languages: string;
  size_mb: number;
  ram_mb: number;
  recommended_cpu: string;
  streaming: boolean;
  description: string;
  downloaded: boolean;
  runtime_supported: boolean;
}

interface Settings {
  audio?: {
    device_name?: string;
    asr_model?: string;
  };
  llm?: Record<string, unknown>;
}

const API_BASE = 'http://127.0.0.1:3000';

export function ModelSetup({ onReady }: { onReady?: () => void }) {
  const [status, setStatus] = useState<ModelStatus>({ state: 'not_downloaded' });
  const [models, setModels] = useState<AsrModelInfo[]>([]);
  const [activeModel, setActiveModel] = useState<string>('');
  const [error, setError] = useState<string | null>(null);

  const refreshAll = useCallback(async () => {
    try {
      const [statusRes, modelsRes, settingsRes] = await Promise.all([
        fetch(`${API_BASE}/settings/models`),
        fetch(`${API_BASE}/settings/models/available`),
        fetch(`${API_BASE}/settings`),
      ]);
      if (statusRes.ok) {
        const s: ModelStatus = await statusRes.json();
        setStatus(s);
        if (s.state === 'ready' && onReady) onReady();
      }
      if (modelsRes.ok) setModels(await modelsRes.json());
      if (settingsRes.ok) {
        const settings: Settings = await settingsRes.json();
        setActiveModel(settings.audio?.asr_model ?? '');
      }
    } catch {
      // Server not ready
    }
  }, [onReady]);

  useEffect(() => {
    void refreshAll();
  }, [refreshAll]);

  // Poll during download
  useEffect(() => {
    if (status.state !== 'downloading') return;
    const interval = setInterval(() => void refreshAll(), 1000);
    return () => clearInterval(interval);
  }, [status.state, refreshAll]);

  const handleDownload = async (target: DownloadTarget) => {
    setError(null);
    try {
      const res = await fetch(`${API_BASE}/settings/models/download`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ target }),
      });
      if (!res.ok) {
        const text = await res.text();
        throw new Error(text || 'Download failed');
      }
      setStatus({ state: 'downloading', target, progress: 0 });
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Download failed');
    }
  };

  const handleSelectModel = async (modelId: string) => {
    setActiveModel(modelId);
    try {
      await fetch(`${API_BASE}/settings`, {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ audio: { asr_model: modelId } }),
      });
    } catch {
      // Silently fail
    }
  };

  const handleDelete = async (modelId: string, modelName: string) => {
    setError(null);
    const confirmed = window.confirm(
      `Delete ${modelName}? The files will be removed from disk. You can re-download later.`,
    );
    if (!confirmed) return;
    try {
      const res = await fetch(
        `${API_BASE}/settings/models/${encodeURIComponent(modelId)}`,
        { method: 'DELETE' },
      );
      if (!res.ok) {
        const text = await res.text();
        throw new Error(text || 'Delete failed');
      }
      // If the user deleted the model they currently have selected,
      // clear the setting so they don't hit a missing-model error later.
      if (activeModel === modelId) {
        setActiveModel('');
        try {
          await fetch(`${API_BASE}/settings`, {
            method: 'PATCH',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ audio: { asr_model: null } }),
          });
        } catch {
          /* ignore */
        }
      }
      await refreshAll();
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Delete failed');
    }
  };

  const isDownloading = status.state === 'downloading';

  // Describe the current download in the progress bar.
  const downloadLabel = (() => {
    const t = status.target;
    if (!t) return 'Preparing...';
    if (t.type === 'shared') return 'Shared files (VAD)'; // unreachable in UI, kept for type safety
    const m = models.find((x) => x.id === t.id);
    return m?.name ?? t.id;
  })();

  return (
    <section className="settings-section">
      <div className="settings-section__title">Speech Models</div>

      {error && <div className="model-error">{error}</div>}

      {isDownloading && (
        <div className="settings-field" style={{ marginBottom: 12 }}>
          <div className="settings-field__label">
            Downloading: {downloadLabel} — {status.current_file ?? '...'}
          </div>
          <div className="model-progress">
            <div className="model-progress__bar">
              <div
                className="model-progress__fill"
                style={{ width: `${Math.round((status.progress ?? 0) * 100)}%` }}
              />
            </div>
            <div className="model-progress__text">
              {Math.round((status.progress ?? 0) * 100)}%
            </div>
          </div>
        </div>
      )}

      {models.length > 0 && (
        <div className="settings-field">
          <div className="settings-field__label">ASR Models</div>
          <div className="model-list">
            {models.map((m) => {
              const isActive = activeModel === m.id;
              // Selecting the radio is only allowed when the model is both
              // downloaded and runtime-supported. Downloading is always
              // offered when the file set is missing.
              const selectDisabled = !m.downloaded || !m.runtime_supported;
              return (
                <div key={m.id} className="model-list__item">
                  <label
                    className={`model-list__row${selectDisabled ? ' model-list__row--disabled' : ''}`}
                  >
                    <input
                      type="radio"
                      name="asr-model"
                      value={m.id}
                      checked={isActive}
                      disabled={selectDisabled}
                      onChange={() => handleSelectModel(m.id)}
                    />
                    <span className="model-list__info">
                      <span className="model-list__name">
                        {m.name}
                        {m.downloaded && (
                          <span className="model-list__check" title="Downloaded">
                            {' \u2713'}
                          </span>
                        )}
                        {!m.runtime_supported && (
                          <span className="model-list__badge" title="Catalog only, not yet runtime-supported">
                            {' (preview)'}
                          </span>
                        )}
                      </span>
                      <span className="model-list__lang">{m.languages}</span>
                      <span className="model-list__spec">
                        {m.streaming ? 'Streaming' : 'Offline'} · {m.size_mb} MB on disk · ~{m.ram_mb} MB RAM · {m.recommended_cpu}
                      </span>
                      <span className="model-list__desc">{m.description}</span>
                    </span>
                  </label>
                  <div className="model-list__actions">
                    {!m.downloaded && (
                      <button
                        type="button"
                        className="model-list__download-btn"
                        onClick={() => handleDownload({ type: 'model', id: m.id })}
                        disabled={isDownloading}
                      >
                        {isDownloading ? 'Another download in progress…' : `Download (${m.size_mb} MB)`}
                      </button>
                    )}
                    {m.downloaded && (
                      <button
                        type="button"
                        className="model-list__delete-btn"
                        onClick={() => handleDelete(m.id, m.name)}
                        disabled={isDownloading}
                        title={`Delete ${m.name} from disk`}
                      >
                        Delete
                      </button>
                    )}
                  </div>
                </div>
              );
            })}
          </div>
        </div>
      )}
    </section>
  );
}
