import { render } from '@testing-library/react';
import { act } from 'react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { StandbyTray } from '../StandbyTray';
import { LanguageProvider } from '../../i18n';
import { useStore } from '../../store/use-store';
import { useActioState } from '../../hooks/useActioState';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: vi.fn() }));
vi.mock('../../hooks/useActioState');
const mockedUseActioState = vi.mocked(useActioState);

function renderTray() {
  return render(
    <LanguageProvider>
      <StandbyTray />
    </LanguageProvider>,
  );
}

/** Pins ISS-080: the dictation transcript region used to combine
 *  `role="status"` + `aria-live="polite"` + `aria-label` on a single div,
 *  which silenced the inner streaming text for screen readers. The fix
 *  splits responsibilities: the label span owns the live region, the
 *  streaming viewport is `aria-hidden`. */
describe('StandbyTray dictation transcript ARIA (ISS-080)', () => {
  beforeEach(() => {
    localStorage.clear();
    Object.defineProperty(navigator, 'language', { value: 'en-US', configurable: true });
    mockedUseActioState.mockReturnValue('transcribing');
    useStore.setState((state) => ({
      reminders: [],
      ui: {
        ...state.ui,
        showBoardWindow: false,
        trayExpanded: false,
        isDictating: true,
        isDictationTranscribing: false,
        dictationTranscript: '',
        feedback: null,
      },
    }));
  });

  it('outer .tray-transcript carries no role / aria-live / aria-label', () => {
    const { container } = renderTray();
    const outer = container.querySelector('.tray-transcript');
    expect(outer).toBeTruthy();
    expect(outer!.getAttribute('role')).toBeNull();
    expect(outer!.getAttribute('aria-live')).toBeNull();
    expect(outer!.getAttribute('aria-label')).toBeNull();
  });

  it('label span owns the live region (role=status + aria-live=polite)', () => {
    const { container } = renderTray();
    const label = container.querySelector('.tray-transcript__label');
    expect(label).toBeTruthy();
    expect(label!.getAttribute('role')).toBe('status');
    expect(label!.getAttribute('aria-live')).toBe('polite');
  });

  it('streaming viewport is aria-hidden so partials do not flood', () => {
    useStore.setState((state) => ({
      ui: {
        ...state.ui,
        dictationTranscript: 'Schedule the design review for tomorrow morning.',
      },
    }));
    const { container } = renderTray();
    const viewport = container.querySelector('.tray-transcript__viewport');
    expect(viewport).toBeTruthy();
    expect(viewport!.getAttribute('aria-hidden')).toBe('true');
    // Sanity: the visible streaming text is still rendered for sighted users.
    expect(viewport!.textContent).toBe(
      'Schedule the design review for tomorrow morning.',
    );
  });

  it('label text changes from "Listening" to "Transcribing" when phase advances', async () => {
    const { container } = renderTray();
    const label = () => container.querySelector('.tray-transcript__label')!;

    expect(label().textContent).toMatch(/Listening/);

    await act(async () => {
      useStore.setState((state) => ({
        ui: { ...state.ui, isDictationTranscribing: true },
      }));
    });

    expect(label().textContent).toMatch(/Transcribing/);
  });
});
