import { useStore } from '../store/use-store';
import { motion } from 'framer-motion';
import type { Tab } from '../types';
import { useT, type TKey } from '../i18n';

const TABS: { id: Tab; labelKey: TKey }[] = [
  { id: 'people', labelKey: 'tab.people' },
  { id: 'recording', labelKey: 'tab.recording' },
  { id: 'board', labelKey: 'tab.board' },
  { id: 'needs-review', labelKey: 'tab.needsReview' },
  { id: 'archive', labelKey: 'tab.archive' },
  { id: 'settings', labelKey: 'tab.settings' },
];

export function TabBar() {
  const activeTab = useStore((s) => s.ui.activeTab);
  const setActiveTab = useStore((s) => s.setActiveTab);
  // Count only the unarchived pending items so the badge matches what the
  // tab content will render. Keep this selector narrow — a full reminders
  // dependency would re-render the TabBar on every card edit.
  const pendingCount = useStore(
    (s) => s.reminders.filter((r) => r.status === 'pending' && r.archivedAt === null).length,
  );
  const t = useT();

  return (
    <div className="tab-bar" role="tablist" aria-label="Board navigation">
      {TABS.map(({ id, labelKey }) => {
        const isActive = activeTab === id;
        const isPrimary = id === 'board';
        const showBadge = id === 'needs-review' && pendingCount > 0;
        return (
          <button
            key={id}
            type="button"
            role="tab"
            aria-selected={isActive}
            className={`tab-bar__tab${isPrimary ? ' tab-bar__tab--primary' : ''}${isActive ? ' is-active' : ''}`}
            onClick={() => setActiveTab(id)}
          >
            {t(labelKey)}
            {showBadge && (
              <span className="tab-bar__badge" aria-label={String(pendingCount)}>
                {pendingCount > 9 ? '9+' : pendingCount}
              </span>
            )}
            {isActive && (
              <motion.div
                layoutId="tabBarIndicator"
                className="tab-bar__indicator"
                initial={false}
                transition={{ type: 'spring', stiffness: 500, damping: 30 }}
              />
            )}
          </button>
        );
      })}
    </div>
  );
}
