import { useEffect, useState, useCallback } from 'react';

const API_BASE = 'http://127.0.0.1:3000';

// --- Types ---

interface LocalLlmInfo {
  id: string;
  name: string;
  size_mb: number;
  ram_mb: number;
  recommended_ram_gb: number;
  description: string;
  runtime_supported: boolean;
}

type LlmSelection =
  | { kind: 'disabled' }
  | { kind: 'local'; id: string }
  | { kind: 'remote' };

interface RemoteLlmSettings {
  base_url?: string;
  api_key?: string;
  model?: string;
}

type DownloadSource = 'hugging_face' | 'hf_mirror' | 'model_scope';

interface LlmSettingsData {
  selection: LlmSelection;
  remote: RemoteLlmSettings;
  local_endpoint_port: number;
  download_source: DownloadSource;
  load_on_startup: boolean;
}

type LoadStatus =
  | { state: 'idle' }
  | { state: 'downloading'; llm_id: string; progress: number }
  | { state: 'quantizing'; llm_id: string }
  | { state: 'loading'; llm_id: string }
  | { state: 'loaded'; llm_id: string }
  | { state: 'error'; llm_id: string; message: string };

interface LlmTestResult {
  success: boolean;
  message: string;
}

// --- API helpers ---

async function fetchSettings(): Promise<{ llm: LlmSettingsData }> {
  const res = await fetch(`${API_BASE}/settings`);
  if (!res.ok) throw new Error('Failed to fetch settings');
  return res.json();
}

async function patchLlmSettings(patch: Record<string, unknown>): Promise<void> {
  const res = await fetch(`${API_BASE}/settings`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ llm: patch }),
  });
  if (!res.ok) throw new Error('Failed to save settings');
}

async function listLocalLlms(): Promise<LocalLlmInfo[]> {
  const res = await fetch(`${API_BASE}/settings/llm/models`);
  if (!res.ok) throw new Error('Failed to list models');
  return res.json();
}

async function startLlmLoad(llmId: string): Promise<void> {
  const res = await fetch(`${API_BASE}/settings/llm/load`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ llm_id: llmId }),
  });
  if (!res.ok) throw new Error('Failed to start loading');
}

async function cancelLlmLoad(): Promise<void> {
  await fetch(`${API_BASE}/settings/llm/cancel-load`, { method: 'POST' });
}

async function fetchLoadStatus(): Promise<LoadStatus> {
  const res = await fetch(`${API_BASE}/settings/llm/load-status`);
  if (!res.ok) throw new Error('Failed to fetch status');
  return res.json();
}

async function testLlm(): Promise<LlmTestResult> {
  const res = await fetch(`${API_BASE}/settings/llm/test`, { method: 'POST' });
  if (!res.ok) throw new Error('Failed to test connection');
  return res.json();
}

// --- Component ---

export function LlmSettings() {
  const [selection, setSelection] = useState<LlmSelection>({ kind: 'disabled' });
  const [remote, setRemote] = useState<RemoteLlmSettings>({});
  const [portInput, setPortInput] = useState('3001');
  const [portError, setPortError] = useState<string | null>(null);
  const [downloadSource, setDownloadSource] = useState<DownloadSource>('hugging_face');
  const [loadOnStartup, setLoadOnStartup] = useState(false);

  const [models, setModels] = useState<LocalLlmInfo[]>([]);
  const [loadStatus, setLoadStatus] = useState<LoadStatus>({ state: 'idle' });
  const [testResult, setTestResult] = useState<LlmTestResult | null>(null);
  const [testing, setTesting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refreshModels = useCallback(async () => {
    try {
      const m = await listLocalLlms();
      setModels(m);
    } catch {}
  }, []);

  const refreshSettings = useCallback(async () => {
    try {
      const s = await fetchSettings();
      if (s.llm) {
        setSelection(s.llm.selection ?? { kind: 'disabled' });
        setRemote(s.llm.remote ?? {});
        setPortInput(String(s.llm.local_endpoint_port ?? 3000));
        setDownloadSource(s.llm.download_source ?? 'hugging_face');
        setLoadOnStartup(s.llm.load_on_startup ?? false);
      }
    } catch {}
  }, []);

  useEffect(() => {
    refreshSettings();
    refreshModels();
    // Restore in-progress load status if navigating back
    fetchLoadStatus().then(setLoadStatus).catch(() => {});
  }, [refreshSettings, refreshModels]);

  // Poll load status while loading/downloading/quantizing
  useEffect(() => {
    const active = loadStatus.state === 'downloading' || loadStatus.state === 'loading';
    if (!active) return;
    const interval = setInterval(async () => {
      try {
        const status = await fetchLoadStatus();
        setLoadStatus(status);
      } catch {}
    }, 2000);
    return () => clearInterval(interval);
  }, [loadStatus.state]);

  const handleSelectionChange = async (sel: LlmSelection) => {
    setSelection(sel);
    setTestResult(null);
    setError(null);
    try {
      await patchLlmSettings({ selection: sel });
      // If selecting a local model, trigger load then fetch real status
      if (sel.kind === 'local' && sel.id) {
        // Immediately show loading state so the UI updates before the
        // async backend call returns.
        setLoadStatus({ state: 'loading', llm_id: sel.id });
        await startLlmLoad(sel.id);
        const status = await fetchLoadStatus();
        setLoadStatus(status);
      } else {
        setLoadStatus({ state: 'idle' });
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to save');
    }
  };

  const handleApplyPort = async () => {
    const port = parseInt(portInput, 10);
    if (isNaN(port) || port < 1024 || port > 65535) {
      setPortError('Port must be 1024\u201365535');
      return;
    }
    setPortError(null);
    try {
      await patchLlmSettings({ local_endpoint_port: port });
    } catch (e) {
      setPortError(e instanceof Error ? e.message : 'Failed to apply port');
    }
  };

  const handleRemoteSave = async () => {
    setError(null);
    try {
      await patchLlmSettings({
        remote: {
          base_url: remote.base_url || undefined,
          api_key: remote.api_key || undefined,
          model: remote.model || undefined,
        },
      });
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Save failed');
    }
  };

  const handleTest = async () => {
    setTesting(true);
    setTestResult(null);
    setError(null);
    try {
      const result = await testLlm();
      setTestResult(result);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Test failed');
    } finally {
      setTesting(false);
    }
  };

  return (
    <section className="settings-section">
      <div className="settings-section__title">Language Models</div>

      {/* Backend radio */}
      <div className="language-model-radio-group">
        <label className="language-model-radio-row">
          <input
            type="radio"
            checked={selection.kind === 'disabled'}
            onChange={() => handleSelectionChange({ kind: 'disabled' })}
          />
          <span>Disabled</span>
          <span className="settings-row__sublabel"> — Action item extraction is off</span>
        </label>
        <label className="language-model-radio-row">
          <input
            type="radio"
            checked={selection.kind === 'local'}
            onChange={() => {
              setSelection({ kind: 'local', id: '' });
              patchLlmSettings({ selection: { kind: 'local', id: '' } }).catch(() => {});
            }}
          />
          <span>Local</span>
          <span className="settings-row__sublabel"> — Run a model on this machine</span>
        </label>
        <label className="language-model-radio-row">
          <input
            type="radio"
            checked={selection.kind === 'remote'}
            onChange={() => handleSelectionChange({ kind: 'remote' })}
          />
          <span>Remote</span>
          <span className="settings-row__sublabel"> — Use an OpenAI-compatible API</span>
        </label>
      </div>

      {/* Local model picker */}
      {selection.kind === 'local' && (
        <div style={{ marginTop: 16 }}>
          <div className="language-model-source-row">
            <span className="settings-field__label">Download from</span>
            <select
              className="settings-input"
              value={downloadSource}
              onChange={async (e) => {
                const src = e.target.value as DownloadSource;
                setDownloadSource(src);
                try {
                  await patchLlmSettings({ download_source: src });
                } catch {}
              }}
              style={{ width: 'auto' }}
            >
              <option value="hugging_face">Hugging Face</option>
              <option value="hf_mirror">HF Mirror (hf-mirror.com)</option>
              <option value="model_scope">ModelScope (modelscope.cn)</option>
            </select>
          </div>

          <div className="settings-field__label">Local model</div>
          <div className="model-list">
            {models.map((m) => {
              const isSelected = selection.kind === 'local' && selection.id === m.id;
              const needsLoad = isSelected && (loadStatus.state === 'idle' || (loadStatus.state === 'error' && loadStatus.llm_id === m.id));
              const isDownloading = loadStatus.state === 'downloading' && loadStatus.llm_id === m.id;
              const isLoadingPhase = loadStatus.state === 'loading' && loadStatus.llm_id === m.id;
              const isLoaded = loadStatus.state === 'loaded' && loadStatus.llm_id === m.id;
              const hasError = loadStatus.state === 'error' && loadStatus.llm_id === m.id;
              const anyBusy = loadStatus.state === 'downloading' || loadStatus.state === 'loading';

              const cancelAndUnselect = async () => {
                await cancelLlmLoad();
                setLoadStatus({ state: 'idle' });
                const cleared: LlmSelection = { kind: 'local', id: '' };
                setSelection(cleared);
                patchLlmSettings({ selection: cleared }).catch(() => {});
              };

              return (
                <div key={m.id} className="model-list__item">
                  <label className="language-model-radio-row">
                    <input
                      type="radio"
                      name="local-model"
                      checked={selection.kind === 'local' && selection.id === m.id}
                      disabled={anyBusy}
                      onChange={() => handleSelectionChange({ kind: 'local', id: m.id })}
                    />
                    <div className="model-list__info">
                      <div className="model-list__name">
                        {m.name}
                        <span style={{ fontSize: 11, color: 'var(--color-text-secondary)', marginLeft: 6, fontFamily: 'monospace' }}>{m.id}</span>
                        {isLoaded && <span className="model-list__check" style={{ marginLeft: 8 }}>Loaded</span>}
                      </div>
                      <div className="model-list__spec">
                        ~{m.size_mb} MB download · ~{m.ram_mb} MB RAM · {m.recommended_ram_gb} GB+ recommended
                      </div>
                      <div className="model-list__desc">{m.description}</div>
                    </div>
                  </label>
                  {isDownloading && (
                    <div className="model-list__actions" style={{ flexDirection: 'column', gap: 6 }}>
                      <div style={{ display: 'flex', alignItems: 'center', gap: 8, width: '100%' }}>
                        <div className="model-list__loading-bar" style={{ background: 'var(--color-bg-hover, #e0e0e0)' }}>
                          <div style={{
                            height: '100%',
                            width: `${Math.round((loadStatus.progress ?? 0) * 100)}%`,
                            background: 'var(--color-accent, #3b82f6)',
                            borderRadius: 3,
                            transition: 'width 0.5s ease',
                          }} />
                        </div>
                        <span style={{ fontSize: 12, minWidth: 36 }}>{Math.round((loadStatus.progress ?? 0) * 100)}%</span>
                        <button type="button" className="model-list__delete-btn" onClick={cancelAndUnselect}>Cancel</button>
                      </div>
                      <div style={{ fontSize: 12, color: 'var(--color-text-secondary)' }}>
                        Downloading model…
                      </div>
                    </div>
                  )}
                  {/* quantizing UI removed — GGUF models are pre-quantized */}
                  {isLoadingPhase && (
                    <div className="model-list__actions" style={{ flexDirection: 'column', gap: 6 }}>
                      <div style={{ display: 'flex', alignItems: 'center', gap: 8, width: '100%' }}>
                        <div className="model-list__loading-bar"><div className="model-list__loading-bar-fill" /></div>
                        <button type="button" className="model-list__delete-btn" onClick={cancelAndUnselect}>Cancel</button>
                      </div>
                      <div style={{ fontSize: 12, color: 'var(--color-text-secondary)' }}>
                        Loading model…
                      </div>
                    </div>
                  )}
                  {needsLoad && (
                    <div className="model-list__actions">
                      <button
                        type="button"
                        className="model-list__download-btn"
                        onClick={async () => {
                          await startLlmLoad(m.id);
                          const status = await fetchLoadStatus();
                          setLoadStatus(status);
                        }}
                      >
                        Load model
                      </button>
                      {hasError && (
                        <span style={{ fontSize: 12, color: 'var(--color-priority-high-text, #d33)' }}>
                          {loadStatus.message}
                        </span>
                      )}
                    </div>
                  )}
                </div>
              );
            })}
          </div>

          {/* Load on startup */}
          <label className="language-model-radio-row" style={{ marginTop: 12 }}>
            <input
              type="checkbox"
              checked={loadOnStartup}
              onChange={async (e) => {
                const v = e.target.checked;
                setLoadOnStartup(v);
                try {
                  await patchLlmSettings({ load_on_startup: v });
                } catch {}
              }}
            />
            <span>Load model at application startup</span>
          </label>
          <div className="language-model-endpoint-hint">
            When enabled, the selected model downloads and loads automatically when Actio starts.
          </div>

          {/* Endpoint port */}
          <div style={{ marginTop: 16 }}>
            <div className="settings-field__label">Endpoint</div>
            <div className="language-model-port-row">
              <span>Port:</span>
              <input
                className={`settings-input ${portError ? 'language-model-port-input--bad' : ''}`}
                type="number"
                min={1024}
                max={65535}
                value={portInput}
                onChange={(e) => setPortInput(e.target.value)}
                style={{ width: 120 }}
              />
              <button
                type="button"
                className="settings-btn settings-btn--secondary"
                onClick={handleApplyPort}
              >
                Apply
              </button>
            </div>
            {portError && (
              <div className="settings-row__sublabel" style={{ color: 'var(--color-priority-high-text)' }}>
                {portError}
              </div>
            )}
            <div className="language-model-endpoint-url">
              Other tools can reach this at: <code>http://127.0.0.1:{portInput}/v1</code>
            </div>
            <div className="language-model-endpoint-hint">
              {parseInt(portInput, 10) === 3000
                ? 'Currently sharing the actio backend port. Pick a different port to expose the LLM separately.'
                : 'LLM endpoint is on a separate port. The actio backend remains on port 3000.'}
            </div>
          </div>
        </div>
      )}

      {/* Remote config */}
      {selection.kind === 'remote' && (
        <div style={{ marginTop: 16 }}>
          <div className="language-model-input-row">
            <span className="settings-field__label" style={{ minWidth: 70 }}>Base URL</span>
            <input
              className="settings-input"
              type="url"
              placeholder="https://api.openai.com/v1"
              value={remote.base_url ?? ''}
              onChange={(e) => setRemote({ ...remote, base_url: e.target.value })}
              onBlur={handleRemoteSave}
            />
          </div>
          <div className="language-model-input-row">
            <span className="settings-field__label" style={{ minWidth: 70 }}>API Key</span>
            <input
              className="settings-input"
              type="password"
              placeholder="sk-..."
              value={remote.api_key ?? ''}
              onChange={(e) => setRemote({ ...remote, api_key: e.target.value })}
              onBlur={handleRemoteSave}
            />
          </div>
          <div className="language-model-input-row">
            <span className="settings-field__label" style={{ minWidth: 70 }}>Model</span>
            <input
              className="settings-input"
              type="text"
              placeholder="gpt-4o-mini"
              value={remote.model ?? ''}
              onChange={(e) => setRemote({ ...remote, model: e.target.value })}
              onBlur={handleRemoteSave}
            />
          </div>
        </div>
      )}

      {/* Error banner */}
      {error && (
        <div className="settings-row__sublabel" style={{ color: 'var(--color-priority-high-text)', marginTop: 8 }}>
          {error}
        </div>
      )}

      {/* Test result */}
      {testResult && (
        <div
          className={testResult.success ? 'language-model-test-ok' : 'language-model-test-fail'}
          style={{ marginTop: 8 }}
        >
          {testResult.success ? '\u2713 ' : '\u2717 '}{testResult.message}
        </div>
      )}

      {/* Test connection button */}
      {selection.kind !== 'disabled' && (
        <div style={{ marginTop: 12 }}>
          <button
            type="button"
            className="settings-btn settings-btn--secondary"
            onClick={handleTest}
            disabled={testing || loadStatus.state === 'loading'}
          >
            {testing ? 'Testing...' : 'Test Connection'}
          </button>
        </div>
      )}
    </section>
  );
}
