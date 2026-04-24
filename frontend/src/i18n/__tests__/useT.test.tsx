import { describe, it, expect, afterEach, beforeEach } from 'vitest';
import { render, screen, act } from '@testing-library/react';
import { LanguageProvider, useLanguage, useT, type TKey } from '../index';

function Probe({ k, vars }: { k: TKey; vars?: Record<string, string | number> }) {
  const t = useT();
  const { lang, setLang } = useLanguage();
  return (
    <div>
      <span data-testid="lang">{lang}</span>
      <span data-testid="out">{t(k, vars)}</span>
      <button type="button" onClick={() => setLang('zh-CN')}>
        switch
      </button>
    </div>
  );
}

beforeEach(() => {
  // Freeze OS locale so bootLocale() is deterministic.
  localStorage.clear();
  Object.defineProperty(navigator, 'language', { value: 'en-US', configurable: true });
});

afterEach(() => {
  localStorage.clear();
});

describe('useT / useLanguage', () => {
  it('returns English by default', () => {
    render(
      <LanguageProvider>
        <Probe k="settings.tab.general" />
      </LanguageProvider>,
    );
    expect(screen.getByTestId('lang').textContent).toBe('en');
    expect(screen.getByTestId('out').textContent).toBe('General');
  });

  it('switches to zh-CN and re-renders translated strings', () => {
    render(
      <LanguageProvider>
        <Probe k="settings.tab.general" />
      </LanguageProvider>,
    );
    act(() => {
      screen.getByRole('button').click();
    });
    expect(screen.getByTestId('lang').textContent).toBe('zh-CN');
    expect(screen.getByTestId('out').textContent).toBe('常规');
  });

  it('interpolates {token} variables', () => {
    render(
      <LanguageProvider>
        <Probe k="voiceprint.title" vars={{ name: 'Dake' }} />
      </LanguageProvider>,
    );
    expect(screen.getByTestId('out').textContent).toBe('Record voiceprint for Dake');
  });

  it('falls back to the raw key when it is not in the dictionary', () => {
    render(
      <LanguageProvider>
        <Probe k={'not.a.real.key' as TKey} />
      </LanguageProvider>,
    );
    expect(screen.getByTestId('out').textContent).toBe('not.a.real.key');
  });

  it('picks zh-CN automatically when navigator.language starts with zh', () => {
    Object.defineProperty(navigator, 'language', { value: 'zh-CN', configurable: true });
    render(
      <LanguageProvider>
        <Probe k="settings.tab.general" />
      </LanguageProvider>,
    );
    expect(screen.getByTestId('lang').textContent).toBe('zh-CN');
    expect(screen.getByTestId('out').textContent).toBe('常规');
  });
});
