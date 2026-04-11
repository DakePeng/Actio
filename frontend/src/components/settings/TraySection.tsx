import { useStore } from '../../store/use-store';

export function TraySection() {
  const reminders = useStore((s) => s.reminders);
  const trayExpanded = useStore((s) => s.ui.trayExpanded);
  const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

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
      <h3 className="settings-section__title">Tray</h3>
      <div className="settings-row">
        <span className="settings-row__label">Tray position</span>
        <button type="button" className="secondary-button" onClick={() => void handleReset()}>
          Reset to default
        </button>
      </div>
    </div>
  );
}
