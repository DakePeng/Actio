import { getApiUrl } from './backend-url';

export interface TranslateLineRequest {
  id: string;
  text: string;
}

export interface TranslateLineResponse {
  id: string;
  text: string;
}

export class LlmDisabledError extends Error {
  constructor() {
    super('llm_disabled');
    this.name = 'LlmDisabledError';
  }
}

/** POST /llm/translate. Throws `LlmDisabledError` on 503 so callers
 *  can flip the toggle off and surface a precise toast. */
export async function translateLines(
  targetLang: string,
  lines: TranslateLineRequest[],
): Promise<TranslateLineResponse[]> {
  const url = await getApiUrl('/llm/translate');
  const resp = await fetch(url, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ target_lang: targetLang, lines }),
  });
  if (resp.status === 503) {
    const body = await resp.json().catch(() => ({}));
    if (body?.error === 'llm_disabled') throw new LlmDisabledError();
    throw new Error(`translate failed: ${resp.status}`);
  }
  if (!resp.ok) {
    throw new Error(`translate failed: ${resp.status}`);
  }
  const data = (await resp.json()) as { translations: TranslateLineResponse[] };
  return data.translations;
}
