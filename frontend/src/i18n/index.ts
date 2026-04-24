import {
  createContext,
  createElement,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from 'react';
import { en, type TKey, type Translations } from './locales/en';
import { zhCN } from './locales/zh-CN';

export type Locale = 'en' | 'zh-CN';

const DICTIONARIES: Record<Locale, Translations> = {
  en,
  'zh-CN': zhCN,
};

const STORAGE_KEY = 'actio-language';

// Boot cache: used for the first render before the backend settings fetch
// resolves. Falls back to OS locale when nothing is cached.
function bootLocale(): Locale {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw === 'en' || raw === 'zh-CN') return raw;
  } catch {
    /* localStorage unavailable (SSR, privacy mode) */
  }
  if (typeof navigator !== 'undefined' && navigator.language?.toLowerCase().startsWith('zh')) {
    return 'zh-CN';
  }
  return 'en';
}

function interpolate(template: string, vars?: Record<string, string | number>): string {
  if (!vars) return template;
  return template.replace(/\{(\w+)\}/g, (_, key: string) =>
    key in vars ? String(vars[key]) : `{${key}}`,
  );
}

interface LanguageContextValue {
  lang: Locale;
  setLang: (next: Locale) => void;
  t: (key: TKey, vars?: Record<string, string | number>) => string;
  /** Lookup that returns `null` when the key is absent from both locale
   *  dictionaries. Useful when falling back to a backend-supplied string
   *  (e.g., model descriptions where only some ids have translations). */
  tMaybe: (key: string, vars?: Record<string, string | number>) => string | null;
}

const LanguageContext = createContext<LanguageContextValue | null>(null);

export function LanguageProvider({ children }: { children: ReactNode }) {
  const [lang, setLangState] = useState<Locale>(() => bootLocale());

  // Pull the authoritative value from the backend once on mount. If it
  // differs from the boot cache, swap to it. LocalStorage is a boot-only
  // cache — the backend is the source of truth.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const res = await fetch('http://127.0.0.1:3000/settings');
        if (!res.ok) return;
        const settings = await res.json();
        const remote: unknown = settings?.language;
        if (remote === 'en' || remote === 'zh-CN') {
          if (!cancelled && remote !== lang) {
            setLangState(remote);
            try {
              localStorage.setItem(STORAGE_KEY, remote);
            } catch {
              /* ignore */
            }
          }
        }
      } catch {
        /* backend unreachable during boot — boot cache stays in effect */
      }
    })();
    return () => {
      cancelled = true;
    };
    // Deliberately run only once on mount.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const setLang = useCallback((next: Locale) => {
    setLangState(next);
    try {
      localStorage.setItem(STORAGE_KEY, next);
    } catch {
      /* ignore */
    }
    // Fire-and-forget the backend write; settings persistence survives a
    // failed request because the boot cache picks up the last value.
    void fetch('http://127.0.0.1:3000/settings', {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ language: next }),
    }).catch(() => {});
  }, []);

  const t = useCallback<LanguageContextValue['t']>(
    (key, vars) => {
      const dict = DICTIONARIES[lang];
      const template = dict[key] ?? en[key] ?? key;
      return interpolate(template, vars);
    },
    [lang],
  );

  const tMaybe = useCallback<LanguageContextValue['tMaybe']>(
    (key, vars) => {
      const dict = DICTIONARIES[lang] as Record<string, string | undefined>;
      const fallback = en as unknown as Record<string, string | undefined>;
      const template = dict[key] ?? fallback[key];
      if (template === undefined) return null;
      return interpolate(template, vars);
    },
    [lang],
  );

  const value = useMemo<LanguageContextValue>(
    () => ({ lang, setLang, t, tMaybe }),
    [lang, setLang, t, tMaybe],
  );

  return createElement(LanguageContext.Provider, { value }, children);
}

export function useLanguage() {
  const ctx = useContext(LanguageContext);
  if (!ctx) throw new Error('useLanguage must be used inside <LanguageProvider>');
  return { lang: ctx.lang, setLang: ctx.setLang };
}

export function useT() {
  const ctx = useContext(LanguageContext);
  if (!ctx) throw new Error('useT must be used inside <LanguageProvider>');
  return ctx.t;
}

export function useTMaybe() {
  const ctx = useContext(LanguageContext);
  if (!ctx) throw new Error('useTMaybe must be used inside <LanguageProvider>');
  return ctx.tMaybe;
}

export type { TKey };
