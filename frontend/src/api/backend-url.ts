const ENV_API_BASE_URL = import.meta.env.VITE_ACTIO_API_BASE_URL?.replace(/\/$/, '');
const FALLBACK_PORTS = Array.from({ length: 10 }, (_, index) => 3000 + index);
const PROBE_TIMEOUT_MS = 800;

let cachedApiBaseUrl: string | null = null;
let pendingDiscovery: Promise<string> | null = null;

function joinUrl(baseUrl: string, path: string) {
  return `${baseUrl}${path.startsWith('/') ? path : `/${path}`}`;
}

async function canReachBackend(baseUrl: string) {
  const controller = new AbortController();
  const timeout = window.setTimeout(() => controller.abort(), PROBE_TIMEOUT_MS);

  try {
    const response = await fetch(joinUrl(baseUrl, '/health'), {
      method: 'GET',
      signal: controller.signal,
    });
    return response.ok;
  } catch {
    return false;
  } finally {
    window.clearTimeout(timeout);
  }
}

export function resetBackendUrlCache() {
  cachedApiBaseUrl = null;
  pendingDiscovery = null;
}

export async function getApiBaseUrl() {
  if (ENV_API_BASE_URL) {
    return ENV_API_BASE_URL;
  }

  if (cachedApiBaseUrl) {
    return cachedApiBaseUrl;
  }

  pendingDiscovery ??= (async () => {
    for (const port of FALLBACK_PORTS) {
      const baseUrl = `http://127.0.0.1:${port}`;
      if (await canReachBackend(baseUrl)) {
        cachedApiBaseUrl = baseUrl;
        return baseUrl;
      }
    }

    throw new Error('Actio backend is not reachable on ports 3000-3009');
  })().finally(() => {
    pendingDiscovery = null;
  });

  return pendingDiscovery;
}

export async function getApiUrl(path: string) {
  return joinUrl(await getApiBaseUrl(), path);
}

export async function getWsUrl(path: string) {
  const baseUrl = await getApiBaseUrl();
  return joinUrl(baseUrl.replace(/^http/, 'ws'), path);
}
