import { useEffect, useState } from 'react';

const API_BASE = 'http://127.0.0.1:3000';

interface LlmSettingsData {
  base_url: string;
  api_key: string;
  model: string;
}

interface LlmTestResult {
  success: boolean;
  message: string;
}

async function fetchSettings(): Promise<{ llm?: Partial<LlmSettingsData> }> {
  const res = await fetch(`${API_BASE}/settings`);
  if (!res.ok) throw new Error('Failed to fetch settings');
  return res.json();
}

async function patchSettings(llm: Partial<LlmSettingsData>): Promise<void> {
  const res = await fetch(`${API_BASE}/settings`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ llm }),
  });
  if (!res.ok) throw new Error('Failed to save settings');
}

async function testLlm(): Promise<LlmTestResult> {
  const res = await fetch(`${API_BASE}/settings/llm/test`, { method: 'POST' });
  if (!res.ok) throw new Error('Failed to test connection');
  return res.json();
}

export function LlmSettings() {
  const [baseUrl, setBaseUrl] = useState('');
  const [apiKey, setApiKey] = useState('');
  const [model, setModel] = useState('');
  const [saving, setSaving] = useState(false);
  const [testing, setTesting] = useState(false);
  const [saveMsg, setSaveMsg] = useState<string | null>(null);
  const [testResult, setTestResult] = useState<LlmTestResult | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    fetchSettings()
      .then((s) => {
        if (s.llm) {
          setBaseUrl(s.llm.base_url ?? '');
          setApiKey(s.llm.api_key ?? '');
          setModel(s.llm.model ?? '');
        }
      })
      .catch(() => {});
  }, []);

  const handleSave = async () => {
    setSaving(true);
    setError(null);
    setSaveMsg(null);
    try {
      await patchSettings({
        base_url: baseUrl || undefined,
        api_key: apiKey || undefined,
        model: model || undefined,
      });
      setSaveMsg('Saved');
      setTimeout(() => setSaveMsg(null), 2000);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Save failed');
    } finally {
      setSaving(false);
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
      <div className="settings-section__title">LLM Configuration</div>

      <div className="settings-field">
        <div className="settings-field__label">Base URL</div>
        <input
          className="settings-input"
          type="url"
          placeholder="https://api.openai.com/v1"
          value={baseUrl}
          onChange={(e) => setBaseUrl(e.target.value)}
        />
      </div>

      <div className="settings-field">
        <div className="settings-field__label">API Key</div>
        <input
          className="settings-input"
          type="password"
          placeholder="sk-..."
          value={apiKey}
          onChange={(e) => setApiKey(e.target.value)}
        />
      </div>

      <div className="settings-field">
        <div className="settings-field__label">Model</div>
        <input
          className="settings-input"
          type="text"
          placeholder="gpt-4o-mini"
          value={model}
          onChange={(e) => setModel(e.target.value)}
        />
      </div>

      {error && (
        <div className="settings-row__sublabel" style={{ color: 'var(--color-priority-high-text)' }}>
          {error}
        </div>
      )}

      {testResult && (
        <div
          className="settings-row__sublabel"
          style={{
            color: testResult.success
              ? 'var(--color-success)'
              : 'var(--color-priority-high-text)',
          }}
        >
          {testResult.message}
        </div>
      )}

      <div className="settings-row" style={{ marginTop: 12 }}>
        <button
          type="button"
          className="settings-btn settings-btn--secondary"
          onClick={handleTest}
          disabled={testing}
        >
          {testing ? 'Testing...' : 'Test Connection'}
        </button>
        <button
          type="button"
          className="settings-btn settings-btn--primary"
          onClick={handleSave}
          disabled={saving}
        >
          {saving ? 'Saving...' : saveMsg ?? 'Save'}
        </button>
      </div>
    </section>
  );
}
