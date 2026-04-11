import { ProfileSection } from './ProfileSection';
import { PreferencesSection } from './PreferencesSection';
import { LabelManager } from './LabelManager';
import { RecordingSection } from './RecordingSection';
import { TraySection } from './TraySection';

export function SettingsView() {
  return (
    <div className="settings-view">
      <ProfileSection />
      <div className="settings-divider" />
      <LabelManager />
      <div className="settings-divider" />
      <PreferencesSection />
      <div className="settings-divider" />
      <RecordingSection />
      <div className="settings-divider" />
      <TraySection />
    </div>
  );
}
