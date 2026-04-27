import { useStore } from '../../store/use-store';
import type { Preferences } from '../../types';
import { useLanguage, useT } from '../../i18n';
import { setAutostart } from '../../utils/autostart';

export function PreferencesSection() {
  const preferences = useStore((s) => s.preferences);
  const setPreferences = useStore((s) => s.setPreferences);
  const { lang, setLang } = useLanguage();
  const t = useT();

  const handleLaunchAtLoginChange = async (next: boolean) => {
    // Optimistic: flip the toggle, then ask the OS plugin to register or
    // remove the auto-start entry. On failure (e.g. permission denied,
    // sandbox restriction), revert so the UI reflects reality.
    setPreferences({ launchAtLogin: next });
    try {
      await setAutostart(next);
    } catch (e) {
      console.error('[Actio] Could not update launch-at-login:', e);
      setPreferences({ launchAtLogin: !next });
    }
  };

  const themes: { id: Preferences['theme']; key: 'light' | 'system' | 'dark' }[] = [
    { id: 'light', key: 'light' },
    { id: 'system', key: 'system' },
    { id: 'dark', key: 'dark' },
  ];

  return (
    <section className="settings-section">
      <div className="settings-section__title">{t('settings.preferences.title')}</div>

      <div className="settings-field">
        <div className="settings-field__label">{t('settings.preferences.theme')}</div>
        <div className="theme-selector">
          {themes.map(({ id, key }) => (
            <button
              key={id}
              type="button"
              className={`theme-btn${preferences.theme === id ? ' is-active' : ''}`}
              onClick={() => setPreferences({ theme: id })}
            >
              {t(`settings.preferences.theme.${key}` as const)}
            </button>
          ))}
        </div>
      </div>

      <div className="settings-row">
        <div>
          <div className="settings-row__label">{t('settings.preferences.language')}</div>
          <div className="settings-row__sublabel">
            {t('settings.preferences.language.sub')}
          </div>
        </div>
        <select
          className="settings-row__select"
          value={lang}
          onChange={(e) => setLang(e.target.value as 'en' | 'zh-CN')}
        >
          <option value="en">{t('settings.preferences.language.en')}</option>
          <option value="zh-CN">{t('settings.preferences.language.zh')}</option>
        </select>
      </div>

      <div className="settings-row">
        <div>
          <div className="settings-row__label">{t('settings.preferences.notifications')}</div>
          <div className="settings-row__sublabel">
            {t('settings.preferences.notifications.sub')}
          </div>
        </div>
        <input
          type="checkbox"
          className="settings-check"
          role="switch"
          aria-checked={preferences.notifications}
          aria-label={t('settings.preferences.notifications')}
          checked={preferences.notifications}
          onChange={(e) => setPreferences({ notifications: e.target.checked })}
        />
      </div>

      <div className="settings-row">
        <div>
          <div className="settings-row__label">{t('settings.preferences.launchAtLogin')}</div>
          <div className="settings-row__sublabel">
            {t('settings.preferences.launchAtLogin.sub')}
          </div>
        </div>
        <input
          type="checkbox"
          className="settings-check"
          role="switch"
          aria-checked={preferences.launchAtLogin}
          aria-label={t('settings.preferences.launchAtLogin')}
          checked={preferences.launchAtLogin}
          onChange={(e) => {
            void handleLaunchAtLoginChange(e.target.checked);
          }}
        />
      </div>
    </section>
  );
}
