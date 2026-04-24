import { useEffect, useState, useCallback } from 'react';
import { useT, useTMaybe, type TKey } from '../../i18n';

type DownloadTarget =
  | { type: 'shared' }
  | { type: 'model'; id: string }
  | { type: 'embedding'; id: string };

interface ModelStatus {
  state: 'not_downloaded' | 'downloading' | 'ready' | 'error';
  target?: DownloadTarget;
  progress?: number;
  current_file?: string;
  message?: string;
}

interface SpeakerEmbeddingModelInfo {
  id: string;
  name: string;
  description: string;
  languages: string;
  size_mb: number;
  embedding_dim: number;
  downloaded: boolean;
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

type AsrDownloadSource = 'hugging_face' | 'hf_mirror';

interface Settings {
  audio?: {
    device_name?: string;
    asr_model?: string;
    speaker_embedding_model?: string;
    download_source?: AsrDownloadSource;
  };
  llm?: Record<string, unknown>;
}

const API_BASE = 'http://127.0.0.1:3000';

type LanguageTab = 'all' | 'chinese' | 'english' | 'korean' | 'french' | 'multilingual';

const LANGUAGE_TABS: { id: LanguageTab; labelKey: TKey }[] = [
  { id: 'all', labelKey: 'settings.models.lang.all' },
  { id: 'chinese', labelKey: 'settings.models.lang.chinese' },
  { id: 'english', labelKey: 'settings.models.lang.english' },
  { id: 'korean', labelKey: 'settings.models.lang.korean' },
  { id: 'french', labelKey: 'settings.models.lang.french' },
  { id: 'multilingual', labelKey: 'settings.models.lang.multilingual' },
];

function isMultilingual(languages: string): boolean {
  return languages.includes(',') || languages.includes('languages');
}

function matchesTab(m: AsrModelInfo, tab: LanguageTab): boolean {
  if (tab === 'all') return true;
  if (tab === 'multilingual') return isMultilingual(m.languages);
  const lang = m.languages.toLowerCase();
  if (tab === 'chinese') return lang === 'chinese';
  if (tab === 'english') return lang === 'english';
  if (tab === 'korean') return lang === 'korean';
  if (tab === 'french') return lang === 'french';
  return false;
}

export function ModelSetup({ onReady }: { onReady?: () => void }) {
  const t = useT();
  const tMaybe = useTMaybe();
  const [status, setStatus] = useState<ModelStatus>({ state: 'not_downloaded' });
  const [models, setModels] = useState<AsrModelInfo[]>([]);
  const [activeModel, setActiveModel] = useState<string>('');
  const [error, setError] = useState<string | null>(null);
  const [langTab, setLangTab] = useState<LanguageTab>('all');
  const [downloadSource, setDownloadSource] = useState<AsrDownloadSource>('hugging_face');
  const [embeddingModels, setEmbeddingModels] = useState<SpeakerEmbeddingModelInfo[]>([]);
  const [activeEmbedding, setActiveEmbedding] = useState<string>('');
  const [hasSpeakers, setHasSpeakers] = useState<boolean>(false);

  const refreshAll = useCallback(async () => {
    try {
      const [statusRes, modelsRes, settingsRes, embeddingsRes, speakersRes] = await Promise.all([
        fetch(`${API_BASE}/settings/models`),
        fetch(`${API_BASE}/settings/models/available`),
        fetch(`${API_BASE}/settings`),
        fetch(`${API_BASE}/settings/models/embeddings`),
        fetch(`${API_BASE}/speakers`),
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
        setDownloadSource(settings.audio?.download_source ?? 'hugging_face');
        setActiveEmbedding(settings.audio?.speaker_embedding_model ?? '');
      }
      if (embeddingsRes.ok) setEmbeddingModels(await embeddingsRes.json());
      if (speakersRes.ok) {
        const speakers: unknown[] = await speakersRes.json();
        setHasSpeakers(speakers.length > 0);
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

  const handleCancelDownload = async () => {
    try {
      await fetch(`${API_BASE}/settings/models/cancel-download`, { method: 'POST' });
      await refreshAll();
    } catch {
      // ignore
    }
  };

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

  const handleSelectEmbedding = async (id: string) => {
    const next = embeddingModels.find((m) => m.id === id);
    const prev = embeddingModels.find((m) => m.id === activeEmbedding);
    const dimChanged = !!prev && !!next && prev.embedding_dim !== next.embedding_dim;
    if (hasSpeakers && prev && prev.id !== id && dimChanged) {
      const ok = window.confirm(t('settings.models.switchEmbeddingConfirm'));
      if (!ok) return;
    }
    setActiveEmbedding(id);
    try {
      await fetch(`${API_BASE}/settings`, {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ audio: { speaker_embedding_model: id } }),
      });
    } catch {
      // silent
    }
  };

  const handleDelete = async (modelId: string, modelName: string) => {
    setError(null);
    const confirmed = window.confirm(
      t('settings.models.deleteConfirm', { name: modelName }),
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
      if (activeEmbedding === modelId) {
        setActiveEmbedding('');
        try {
          await fetch(`${API_BASE}/settings`, {
            method: 'PATCH',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ audio: { speaker_embedding_model: null } }),
          });
        } catch { /* ignore */ }
      }
      await refreshAll();
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Delete failed');
    }
  };

  const isDownloading = status.state === 'downloading';

  // Describe the current download in the progress bar.
  const downloadLabel = (() => {
    const target = status.target;
    if (!target) return t('settings.models.preparing');
    if (target.type === 'shared') return 'Shared files (VAD)'; // unreachable in UI
    if (target.type === 'embedding') {
      return embeddingModels.find((x) => x.id === target.id)?.name ?? target.id;
    }
    const m = models.find((x) => x.id === target.id);
    return m?.name ?? target.id;
  })();

  return (
    <section className="settings-section">
      <div className="settings-section__title">{t('settings.models.title')}</div>

      <div className="settings-field" style={{ marginBottom: 8 }}>
        <span className="settings-field__label">{t('settings.models.downloadFrom')}</span>
        <select
          className="settings-input"
          value={downloadSource}
          onChange={async (e) => {
            const src = e.target.value as AsrDownloadSource;
            setDownloadSource(src);
            try {
              await fetch(`${API_BASE}/settings`, {
                method: 'PATCH',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ audio: { download_source: src } }),
              });
            } catch { /* ignore */ }
          }}
          style={{ width: 'auto' }}
        >
          <option value="hugging_face">Hugging Face</option>
          <option value="hf_mirror">HF Mirror (hf-mirror.com)</option>
        </select>
      </div>

      {error && <div className="model-error">{error}</div>}

      {/* Per-card progress lives inside each model's own item (see below).
          A non-specific download (e.g. shared files before a target is
          picked, or preparing state) still needs a place to live — show
          a tiny inline strip here for that edge case only. */}
      {isDownloading && (!status.target || status.target.type === 'shared') && (
        <div className="settings-field" style={{ marginBottom: 12 }}>
          <div className="settings-field__label">
            {t('settings.models.downloading', {
              label: downloadLabel,
              file: status.current_file ?? '...',
            })}
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
            <button
              type="button"
              className="model-progress__cancel-btn"
              onClick={handleCancelDownload}
              title={t('settings.models.cancelTitle')}
            >
              {t('settings.models.cancel')}
            </button>
          </div>
        </div>
      )}

      {embeddingModels.length > 0 && (
        <div className="settings-field">
          <div className="settings-field__label">{t('settings.models.commonModels')}</div>
          <div
            className="settings-field__sublabel"
            style={{ marginBottom: 8, opacity: 0.7 }}
          >
            {t('settings.models.commonModels.sub')}
          </div>
          <div className="model-list">
            {embeddingModels.map((m) => {
              const isActive = activeEmbedding === m.id;
              const selectDisabled = !m.downloaded;
              const isDownloadingThis =
                isDownloading &&
                status.target?.type === 'embedding' &&
                status.target.id === m.id;
              return (
                <div key={m.id} className="model-list__item">
                  <label
                    className={`model-list__row${selectDisabled ? ' model-list__row--disabled' : ''}`}
                  >
                    <input
                      type="radio"
                      name="embedding-model"
                      value={m.id}
                      checked={isActive}
                      disabled={selectDisabled}
                      onChange={() => void handleSelectEmbedding(m.id)}
                    />
                    <span className="model-list__info">
                      <span className="model-list__name">
                        {m.name}
                        {m.downloaded && (
                          <span className="model-list__check" title={t('settings.models.downloaded')}>
                            {' \u2713'}
                          </span>
                        )}
                      </span>
                      <span className="model-list__lang">{m.languages}</span>
                      <span className="model-list__spec">
                        {m.size_mb} MB · {m.embedding_dim}-dim embeddings
                      </span>
                      <span className="model-list__desc">
                        {tMaybe(`model.desc.${m.id}`) ?? m.description}
                      </span>
                    </span>
                  </label>
                  <div className="model-list__actions">
                    {!m.downloaded && !isDownloadingThis && (
                      <button
                        type="button"
                        className="model-list__download-btn"
                        onClick={() => handleDownload({ type: 'embedding', id: m.id })}
                        disabled={isDownloading}
                      >
                        {isDownloading
                          ? t('settings.models.downloadOther')
                          : t('settings.models.download', { size: m.size_mb })}
                      </button>
                    )}
                    {m.downloaded && (
                      <button
                        type="button"
                        className="model-list__delete-btn"
                        onClick={() => handleDelete(m.id, m.name)}
                        disabled={isDownloading}
                        title={t('settings.models.deleteTitle', { name: m.name })}
                      >
                        {t('settings.models.delete')}
                      </button>
                    )}
                  </div>
                  {isDownloadingThis && (
                    <div className="model-progress" aria-live="polite">
                      <div className="model-progress__bar">
                        <div
                          className="model-progress__fill"
                          style={{ width: `${Math.round((status.progress ?? 0) * 100)}%` }}
                        />
                      </div>
                      <div className="model-progress__text">
                        {Math.round((status.progress ?? 0) * 100)}%
                      </div>
                      <button
                        type="button"
                        className="model-progress__cancel-btn"
                        onClick={handleCancelDownload}
                        title={t('settings.models.cancelTitle')}
                      >
                        {t('settings.models.cancel')}
                      </button>
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        </div>
      )}

      {models.length > 0 && (
        <div className="settings-field">
          <div className="settings-field__label">{t('settings.models.asrModels')}</div>
          <div className="model-lang-tabs">
            {LANGUAGE_TABS
              .filter((tab) => tab.id === 'all' || models.some((m) => matchesTab(m, tab.id)))
              .map((tab) => (
                <button
                  key={tab.id}
                  type="button"
                  className={`model-lang-tab${langTab === tab.id ? ' is-active' : ''}`}
                  onClick={() => setLangTab(tab.id)}
                >
                  {t(tab.labelKey)}
                </button>
              ))}
          </div>
          <div className="model-list">
            {models.filter((m) => matchesTab(m, langTab)).map((m) => {
              const isActive = activeModel === m.id;
              // Selecting the radio is only allowed when the model is both
              // downloaded and runtime-supported. Downloading is always
              // offered when the file set is missing.
              const selectDisabled = !m.downloaded || !m.runtime_supported;
              const isDownloadingThis =
                isDownloading &&
                status.target?.type === 'model' &&
                status.target.id === m.id;
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
                          <span className="model-list__check" title={t('settings.models.downloaded')}>
                            {' \u2713'}
                          </span>
                        )}
                        {!m.runtime_supported && (
                          <span className="model-list__badge" title="Catalog only, not yet runtime-supported">
                            {t('settings.models.preview')}
                          </span>
                        )}
                      </span>
                      <span className="model-list__lang">{m.languages}</span>
                      <span className="model-list__spec">
                        {m.streaming ? t('settings.models.streaming') : t('settings.models.offline')} · {m.size_mb} MB on disk · ~{m.ram_mb} MB RAM · {m.recommended_cpu}
                      </span>
                      <span className="model-list__desc">
                        {tMaybe(`model.desc.${m.id}`) ?? m.description}
                      </span>
                    </span>
                  </label>
                  <div className="model-list__actions">
                    {!m.downloaded && !isDownloadingThis && (
                      <button
                        type="button"
                        className="model-list__download-btn"
                        onClick={() => handleDownload({ type: 'model', id: m.id })}
                        disabled={isDownloading}
                      >
                        {isDownloading
                          ? t('settings.models.downloadOther')
                          : t('settings.models.download', { size: m.size_mb })}
                      </button>
                    )}
                    {m.downloaded && (
                      <button
                        type="button"
                        className="model-list__delete-btn"
                        onClick={() => handleDelete(m.id, m.name)}
                        disabled={isDownloading}
                        title={t('settings.models.deleteTitle', { name: m.name })}
                      >
                        {t('settings.models.delete')}
                      </button>
                    )}
                  </div>
                  {isDownloadingThis && (
                    <div className="model-progress" aria-live="polite">
                      <div className="model-progress__bar">
                        <div
                          className="model-progress__fill"
                          style={{ width: `${Math.round((status.progress ?? 0) * 100)}%` }}
                        />
                      </div>
                      <div className="model-progress__text">
                        {Math.round((status.progress ?? 0) * 100)}%
                      </div>
                      <button
                        type="button"
                        className="model-progress__cancel-btn"
                        onClick={handleCancelDownload}
                        title={t('settings.models.cancelTitle')}
                      >
                        {t('settings.models.cancel')}
                      </button>
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        </div>
      )}
    </section>
  );
}
