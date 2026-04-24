import { useStore } from '../../store/use-store';
import { useT } from '../../i18n';

export function ProfileSection() {
  const profile = useStore((s) => s.profile);
  const setProfile = useStore((s) => s.setProfile);
  const t = useT();

  return (
    <section className="settings-section">
      <div className="settings-section__title">{t('settings.profile.title')}</div>
      <div className="settings-field">
        <label className="settings-field__label" htmlFor="profile-name">
          {t('settings.profile.name')}
        </label>
        <input
          id="profile-name"
          type="text"
          className="settings-input"
          value={profile.name}
          onChange={(e) => setProfile({ name: e.target.value })}
          placeholder={t('settings.profile.namePlaceholder')}
        />
      </div>
    </section>
  );
}
