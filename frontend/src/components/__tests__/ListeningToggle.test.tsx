import { fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { ListeningToggle } from '../ListeningToggle';
import { LanguageProvider } from '../../i18n';
import { useStore } from '../../store/use-store';

function renderToggle() {
  return render(
    <LanguageProvider>
      <ListeningToggle />
    </LanguageProvider>,
  );
}

describe('ListeningToggle', () => {
  beforeEach(() => {
    Object.defineProperty(navigator, 'language', { value: 'en-US', configurable: true });
    useStore.setState((state) => ({
      ui: { ...state.ui, listeningEnabled: true, listeningStartedAt: 1 },
    }));
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true, json: () => Promise.resolve({}) }));
  });

  it('renders the on-state aria label when listening', () => {
    renderToggle();
    expect(screen.getByRole('button', { name: /click to mute/i })).toHaveAttribute(
      'aria-pressed',
      'true',
    );
  });

  it('renders the off-state aria label when muted', () => {
    useStore.setState((state) => ({
      ui: { ...state.ui, listeningEnabled: false, listeningStartedAt: null },
    }));
    renderToggle();
    expect(screen.getByRole('button', { name: /click to start listening/i })).toHaveAttribute(
      'aria-pressed',
      'false',
    );
  });

  it('disables itself while the toggle is null (boot)', () => {
    useStore.setState((state) => ({
      ui: { ...state.ui, listeningEnabled: null, listeningStartedAt: null },
    }));
    renderToggle();
    expect(screen.getByRole('button')).toBeDisabled();
  });

  it('clicking calls setListening with the inverted value', () => {
    const spy = vi.spyOn(useStore.getState(), 'setListening').mockResolvedValue();
    renderToggle();
    fireEvent.click(screen.getByRole('button'));
    expect(spy).toHaveBeenCalledWith(false);
  });
});
