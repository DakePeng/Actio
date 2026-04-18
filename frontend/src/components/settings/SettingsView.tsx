import { useState } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { ProfileSection } from './ProfileSection';
import { PreferencesSection } from './PreferencesSection';
import { TraySection } from './TraySection';
import { LabelManager } from './LabelManager';
import { AudioSettings } from './AudioSettings';
import { RecordingSection } from './RecordingSection';
import { LlmSettings } from './LlmSettings';
import { ModelSetup } from './ModelSetup';
import { KeyboardSettings } from './KeyboardSettings';

type SettingsTab = 'general' | 'board' | 'voice' | 'ai' | 'shortcuts';

const SECTION_TABS: { id: SettingsTab; label: string }[] = [
  { id: 'general', label: 'General' },
  { id: 'board', label: 'Board' },
  { id: 'voice', label: 'Voice' },
  { id: 'ai', label: 'AI' },
  { id: 'shortcuts', label: 'Shortcuts' },
];

const panelMotion = {
  initial: { opacity: 0, x: -12 },
  animate: { opacity: 1, x: 0 },
  exit: { opacity: 0, x: 12 },
  transition: { duration: 0.18 },
};

export function SettingsView() {
  const [tab, setTab] = useState<SettingsTab>('general');

  return (
    <div className="settings-view">
      <div className="settings-view__section-tabs" role="tablist" aria-label="Settings sections">
        {SECTION_TABS.map(({ id, label }) => {
          const isActive = tab === id;
          return (
            <button
              key={id}
              type="button"
              role="tab"
              aria-selected={isActive}
              className={`settings-section-btn${isActive ? ' is-active' : ''}`}
              onClick={() => setTab(id)}
            >
              {label}
              {isActive && (
                <motion.div
                  layoutId="settingsSectionIndicator"
                  className="settings-section-btn__indicator"
                  initial={false}
                  transition={{ type: 'spring', stiffness: 500, damping: 32 }}
                />
              )}
            </button>
          );
        })}
      </div>

      <AnimatePresence mode="wait">
        {tab === 'general' && (
          <motion.div key="general" className="settings-view__panel" {...panelMotion}>
            <ProfileSection />
            <PreferencesSection />
            <TraySection />
          </motion.div>
        )}
        {tab === 'board' && (
          <motion.div key="board" className="settings-view__panel" {...panelMotion}>
            <LabelManager />
          </motion.div>
        )}
        {tab === 'voice' && (
          <motion.div key="voice" className="settings-view__panel" {...panelMotion}>
            <AudioSettings />
            <RecordingSection />
          </motion.div>
        )}
        {tab === 'ai' && (
          <motion.div key="ai" className="settings-view__panel" {...panelMotion}>
            <LlmSettings />
            <ModelSetup />
          </motion.div>
        )}
        {tab === 'shortcuts' && (
          <motion.div key="shortcuts" className="settings-view__panel" {...panelMotion}>
            <KeyboardSettings />
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
