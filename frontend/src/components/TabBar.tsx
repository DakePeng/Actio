import { useStore } from '../store/use-store';

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
      {TABS.map(({ id, label }) => (
        <button
          key={id}
          type="button"
          role="tab"
          aria-selected={activeTab === id}
          className={`tab-bar__tab${activeTab === id ? ' is-active' : ''}`}
          onClick={() => setActiveTab(id)}
        >
          {label}
        </button>
      ))}
    </div>
  );
}
