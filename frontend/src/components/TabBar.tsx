import { useStore } from '../store/use-store';
import { motion } from 'framer-motion';

type Tab = 'board' | 'archive' | 'settings';

const TABS: { id: Tab; label: string }[] = [
  { id: 'board', label: 'Board' },
  { id: 'archive', label: 'Archive' },
  { id: 'settings', label: 'Settings' },
];

export function TabBar() {
  const activeTab = useStore((s) => s.ui.activeTab);
  const setActiveTab = useStore((s) => s.setActiveTab);

  return (
    <div className="tab-bar" role="tablist" aria-label="Board navigation">
      {TABS.map(({ id, label }) => {
        const isActive = activeTab === id;
        return (
          <button
            key={id}
            type="button"
            role="tab"
            aria-selected={isActive}
            className={`tab-bar__tab${isActive ? ' is-active' : ''}`}
            onClick={() => setActiveTab(id)}
          >
            {label}
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
