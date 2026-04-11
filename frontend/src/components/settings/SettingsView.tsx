import { ProfileSection } from './ProfileSection';
import { PreferencesSection } from './PreferencesSection';
import { LabelManager } from './LabelManager';
import { RecordingSection } from './RecordingSection';
import { TraySection } from './TraySection';
import { ModelSetup } from './ModelSetup';
import { LlmSettings } from './LlmSettings';
import { AudioSettings } from './AudioSettings';

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
      <div className="settings-divider" />
      <AudioSettings />
      <div className="settings-divider" />
      <LlmSettings />
      <div className="settings-divider" />
      <ModelSetup />
    </div>
  );
}
