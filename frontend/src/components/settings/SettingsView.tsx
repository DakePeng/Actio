import { ProfileSection } from './ProfileSection';
import { PreferencesSection } from './PreferencesSection';
import { LabelManager } from './LabelManager';

export function SettingsView() {
  return (
    <div className="settings-view">
      <ProfileSection />
      <div className="settings-divider" />
      <LabelManager />
      <div className="settings-divider" />
      <PreferencesSection />
    </div>
  );
}
