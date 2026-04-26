import { useStore } from '../../store/use-store';
import { useT } from '../../i18n';
import { useState } from 'react';

export function ProfileSection() {
  const profile = useStore((s) => s.profile);
  const setProfile = useStore((s) => s.setProfile);
  const saveProfile = useStore((s) => s.saveProfile);
  const t = useT();
  const [draftAlias, setDraftAlias] = useState('');

  const addAlias = () => {
    const v = draftAlias.trim();
    if (!v) return;
    const isAscii = (s: string) => /^[\x00-\x7F]*$/.test(s);
    const norm = (s: string) => (isAscii(s) ? s.toLowerCase() : s);
    if (profile.aliases.some((a) => norm(a) === norm(v))) {
      setDraftAlias('');
      return;
    }
    setProfile({ aliases: [...profile.aliases, v] });
    setDraftAlias('');
  };

  const removeAlias = (a: string) =>
    setProfile({ aliases: profile.aliases.filter((x) => x !== a) });

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
          value={profile.display_name}
          onChange={(e) => setProfile({ display_name: e.target.value })}
          placeholder={t('settings.profile.namePlaceholder')}
        />
      </div>

      <div className="settings-field">
        <label className="settings-field__label" htmlFor="profile-aliases">
          {t('settings.profile.aliases')}
        </label>
        <div className="alias-chips">
          {profile.aliases.map((a) => (
            <span key={a} className="alias-chip">
              {a}
              <button
                type="button"
                onClick={() => removeAlias(a)}
                aria-label={t('settings.profile.removeAlias')}
              >×</button>
            </span>
          ))}
        </div>
        <input
          id="profile-aliases"
          type="text"
          className="settings-input"
          value={draftAlias}
          onChange={(e) => setDraftAlias(e.target.value)}
          onKeyDown={(e) => { if (e.key === 'Enter') { e.preventDefault(); addAlias(); } }}
          placeholder={t('settings.profile.aliasesPlaceholder')}
        />
      </div>

      <div className="settings-field">
        <label className="settings-field__label" htmlFor="profile-bio">
          {t('settings.profile.bio')}
        </label>
        <textarea
          id="profile-bio"
          className="settings-input"
          rows={4}
          value={profile.bio}
          onChange={(e) => setProfile({ bio: e.target.value })}
          placeholder={t('settings.profile.bioPlaceholder')}
        />
      </div>

      <button type="button" className="settings-button" onClick={saveProfile}>
        {t('settings.profile.save')}
      </button>
    </section>
  );
}
