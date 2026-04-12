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
      {/* ── General ── */}
      <h2 className="settings-group-title">General</h2>
      <ProfileSection />
      <PreferencesSection />

      {/* ── Board ── */}
      <h2 className="settings-group-title">Board</h2>
      <LabelManager />

      {/* ── Transcription & AI ── */}
      <h2 className="settings-group-title">Transcription &amp; AI</h2>
      <AudioSettings />
      <RecordingSection />
      <LlmSettings />
      <ModelSetup />
      <TraySection />
    </div>
  );
}
