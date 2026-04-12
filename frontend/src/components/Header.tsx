import { ActioIcon } from './ActioIcon';
import { useActioIconState } from '../hooks/useActioIconState';

export function Header() {
  const iconState = useActioIconState();

  return (
    <header className="topbar">
      <div className="topbar__inner">
        <div className="brand-lockup">
          <div className="brand-mark" aria-hidden="true">
            <ActioIcon state={iconState} size={18} />
          </div>
          <div className="brand-copy">
            <div className="brand-title">actio</div>
            <div className="brand-subtitle">Voice-captured reminders, organized for action.</div>
          </div>
        </div>

        <div className="topbar-actions">
          <div className="topbar-avatar" aria-label="Current user">
            JD
          </div>
        </div>
      </div>
    </header>
  );
}
