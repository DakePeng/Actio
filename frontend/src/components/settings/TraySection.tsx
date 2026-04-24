import { useStore } from '../../store/use-store';
import { useT } from '../../i18n';

export function TraySection() {
  const reminders = useStore((s) => s.reminders);
  const trayExpanded = useStore((s) => s.ui.trayExpanded);
  const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
  const t = useT();

  async function handleReset() {
    if (!isTauri) return;
    const { invoke } = await import('@tauri-apps/api/core');
    await invoke('reset_tray_position', {
      trayExpanded,
      reminderCount: reminders.length,
    });
  }

  return (
    <div className="settings-section">
      <h3 className="settings-section__title">{t('settings.tray.title')}</h3>
      <div className="settings-row">
        <span className="settings-row__label">{t('settings.tray.position')}</span>
        <button type="button" className="secondary-button" onClick={() => void handleReset()}>
          {t('settings.tray.reset')}
        </button>
      </div>
    </div>
  );
}
