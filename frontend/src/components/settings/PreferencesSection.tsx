import { useStore } from '../../store/use-store';
import type { Preferences } from '../../types';

export function PreferencesSection() {
  const preferences = useStore((s) => s.preferences);
  const setPreferences = useStore((s) => s.setPreferences);

  const themes: { id: Preferences['theme']; label: string }[] = [
    { id: 'light', label: 'Light' },
    { id: 'system', label: 'System' },
    { id: 'dark', label: 'Dark' },
  ];

  return (
    <section className="settings-section">
      <div className="settings-section__title">Preferences</div>

      <div className="settings-field">
        <div className="settings-field__label">Theme</div>
        <div className="theme-selector">
          {themes.map(({ id, label }) => (
            <button
              key={id}
              type="button"
              className={`theme-btn${preferences.theme === id ? ' is-active' : ''}`}
              onClick={() => setPreferences({ theme: id })}
            >
              {label}
            </button>
          ))}
        </div>
      </div>

      <div className="settings-row">
        <div>
          <div className="settings-row__label">Notifications</div>
          <div className="settings-row__sublabel">Show alerts for new reminders</div>
        </div>
        <label className="toggle">
          <input
            type="checkbox"
            checked={preferences.notifications}
            onChange={(e) => setPreferences({ notifications: e.target.checked })}
          />
          <div className="toggle__track" />
          <div className="toggle__thumb" />
        </label>
      </div>

      <div className="settings-row">
        <div>
          <div className="settings-row__label">Launch at login</div>
          <div className="settings-row__sublabel">Start Actio automatically when you log in</div>
        </div>
        <label className="toggle">
          <input
            type="checkbox"
            checked={preferences.launchAtLogin}
            onChange={(e) => setPreferences({ launchAtLogin: e.target.checked })}
          />
          <div className="toggle__track" />
          <div className="toggle__thumb" />
        </label>
      </div>
    </section>
  );
}
