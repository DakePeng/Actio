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
import { useT } from '../../i18n';

type SettingsTab = 'general' | 'board' | 'voice' | 'ai' | 'shortcuts';

const SECTION_TABS: { id: SettingsTab; labelKey: `settings.tab.${SettingsTab}` }[] = [
  { id: 'general', labelKey: 'settings.tab.general' },
  { id: 'board', labelKey: 'settings.tab.board' },
  { id: 'voice', labelKey: 'settings.tab.voice' },
  { id: 'ai', labelKey: 'settings.tab.ai' },
  { id: 'shortcuts', labelKey: 'settings.tab.shortcuts' },
];

const panelMotion = {
  initial: { opacity: 0, x: -12 },
  animate: { opacity: 1, x: 0 },
  exit: { opacity: 0, x: 12 },
  transition: { duration: 0.18 },
};

export function SettingsView() {
  const [tab, setTab] = useState<SettingsTab>('general');
  const t = useT();

  const panelContent: Record<SettingsTab, React.ReactNode> = {
    general: <><ProfileSection /><PreferencesSection /><TraySection /></>,
    board: <LabelManager />,
    voice: <><AudioSettings /><RecordingSection /><ModelSetup /></>,
    ai: <><LlmSettings /></>,
    shortcuts: <KeyboardSettings />,
  };

  return (
    <div className="settings-view">
      <div className="settings-view__section-tabs" role="tablist" aria-label="Settings sections">
        {SECTION_TABS.map(({ id, labelKey }) => {
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
              {t(labelKey)}
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
        <motion.div key={tab} className="settings-view__panel" {...panelMotion}>
          {panelContent[tab]}
        </motion.div>
      </AnimatePresence>
    </div>
  );
}
