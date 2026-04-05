import { useContext } from 'react';
import { useStore } from '../store/use-store';
import { StandbyTrayContext } from './StandbyTray';

export function Fab() {
  const setNewReminderBar = useStore((s) => s.setNewReminderBar);
  const { expanded } = useContext(StandbyTrayContext);

  return (
    <button
      type="button"
      className="floating-fab"
      style={{
        bottom: expanded ? '96px' : '24px',
        right: expanded ? '360px' : '24px',
      }}
      onClick={() => setNewReminderBar(true)}
      title="Add manually"
    >
      <span className="floating-fab__inner">+</span>
    </button>
  );
}
