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
  downloaded: boolean;
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

interface LlmSettingsData {
  selection: LlmSelection;
  remote: RemoteLlmSettings;
  local_endpoint_port: number;
}

interface LlmDownloadStatus {
  state: 'idle' | 'downloading' | 'error';
  llm_id?: string;
  progress?: number;
  bytes_downloaded?: number;
  bytes_total?: number;
  message?: string;
}

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

async function startLlmDownload(llmId: string): Promise<void> {
  const res = await fetch(`${API_BASE}/settings/llm/models/download`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ llm_id: llmId }),
  });
  if (!res.ok) throw new Error('Failed to start download');
}

async function deleteLocalLlm(llmId: string): Promise<void> {
  const res = await fetch(`${API_BASE}/settings/llm/models/${llmId}`, {
    method: 'DELETE',
  });
  if (!res.ok) throw new Error('Failed to delete model');
}

async function fetchLlmDownloadStatus(): Promise<LlmDownloadStatus> {
  const res = await fetch(`${API_BASE}/settings/llm/download-status`);
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
  const [portInput, setPortInput] = useState('3000');
  const [portError, setPortError] = useState<string | null>(null);

  const [models, setModels] = useState<LocalLlmInfo[]>([]);
  const [downloadStatus, setDownloadStatus] = useState<LlmDownloadStatus>({ state: 'idle' });
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
      }
    } catch {}
  }, []);

  useEffect(() => {
    refreshSettings();
    refreshModels();
  }, [refreshSettings, refreshModels]);

  // Poll download status while downloading
  useEffect(() => {
    if (downloadStatus.state !== 'downloading') return;
    const interval = setInterval(async () => {
      try {
        const status = await fetchLlmDownloadStatus();
        setDownloadStatus(status);
        if (status.state === 'idle' || status.state === 'error') {
          refreshModels();
        }
      } catch {}
    }, 1000);
    return () => clearInterval(interval);
  }, [downloadStatus.state, refreshModels]);

  const handleSelectionChange = async (sel: LlmSelection) => {
    setSelection(sel);
    setTestResult(null);
    setError(null);
    try {
      await patchLlmSettings({ selection: sel });
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to save');
    }
  };

  const handleDownload = async (llmId: string) => {
    setError(null);
    try {
      await startLlmDownload(llmId);
      setDownloadStatus({ state: 'downloading', llm_id: llmId, progress: 0 });
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Download failed');
    }
  };

  const handleDelete = async (llmId: string, name: string) => {
    const isActive = selection.kind === 'local' && selection.id === llmId;
    const msg = isActive
      ? `Delete ${name}? It is currently selected — action item extraction will be disabled until you pick another model.`
      : `Delete ${name}?`;
    if (!confirm(msg)) return;

    setError(null);
    try {
      await deleteLocalLlm(llmId);
      await refreshModels();
      if (isActive) {
        await refreshSettings();
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Delete failed');
    }
  };

  const handleApplyPort = async () => {
    const port = parseInt(portInput, 10);
    if (isNaN(port) || port < 1024 || port > 65535) {
      setPortError('Port must be 1024–65535');
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
              const first = models.find((m) => m.downloaded);
              if (first) {
                handleSelectionChange({ kind: 'local', id: first.id });
              } else {
                // Allow selecting Local even without downloads so the
                // model list (with download buttons) becomes visible.
                setSelection({ kind: 'local', id: '' });
              }
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
          <div className="settings-field__label">Local model</div>
          {models.map((m) => (
            <div key={m.id} className={`model-list__row ${!m.downloaded ? 'model-list__row--disabled' : ''}`}>
              <label className="language-model-radio-row">
                <input
                  type="radio"
                  name="local-model"
                  checked={selection.kind === 'local' && selection.id === m.id}
                  disabled={!m.downloaded}
                  onChange={() => handleSelectionChange({ kind: 'local', id: m.id })}
                />
                <span>{m.name}</span>
                {m.downloaded && <span className="model-list__check">Downloaded</span>}
              </label>
              <div className="settings-row__sublabel" style={{ marginLeft: 24 }}>
                ~{m.size_mb} MB on disk · ~{m.ram_mb} MB RAM · {m.recommended_ram_gb} GB+ recommended
              </div>
              <div className="settings-row__sublabel" style={{ marginLeft: 24 }}>
                {m.description}
              </div>
              <div style={{ marginLeft: 24, marginTop: 4 }}>
                {!m.downloaded && downloadStatus.state !== 'downloading' && (
                  <button
                    type="button"
                    className="model-list__download-btn"
                    onClick={() => handleDownload(m.id)}
                  >
                    Download {m.size_mb} MB
                  </button>
                )}
                {downloadStatus.state === 'downloading' && downloadStatus.llm_id === m.id && (
                  <div className="model-progress">
                    <div
                      className="model-progress__bar"
                      style={{ width: `${(downloadStatus.progress ?? 0) * 100}%` }}
                    />
                    <span>{Math.round((downloadStatus.progress ?? 0) * 100)}%</span>
                  </div>
                )}
                {m.downloaded && (
                  <button
                    type="button"
                    className="model-list__delete-btn"
                    onClick={() => handleDelete(m.id, m.name)}
                  >
                    Delete
                  </button>
                )}
              </div>
            </div>
          ))}

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
                style={{ width: 80 }}
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
            disabled={testing}
          >
            {testing ? 'Testing...' : 'Test Connection'}
          </button>
        </div>
      )}
    </section>
  );
}
