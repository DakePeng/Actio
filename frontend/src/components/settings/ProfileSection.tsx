import { useStore } from '../../store/use-store';

export function ProfileSection() {
  const profile = useStore((s) => s.profile);
  const setProfile = useStore((s) => s.setProfile);

  return (
    <section className="settings-section">
      <div className="settings-section__title">Profile</div>
      <div className="settings-field">
        <label className="settings-field__label" htmlFor="profile-name">Name</label>
        <input
          id="profile-name"
          type="text"
          className="settings-input"
          value={profile.name}
          onChange={(e) => setProfile({ name: e.target.value })}
          placeholder="Your name"
        />
      </div>
    </section>
  );
}
