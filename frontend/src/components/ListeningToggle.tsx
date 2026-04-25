import { useStore } from '../store/use-store';
import { useT } from '../i18n';

export interface ListeningToggleProps {
  /** Optional className applied alongside the base class. */
  className?: string;
  /** Render at this pixel size (square). Defaults to 28. */
  size?: number;
}

export function ListeningToggle({
  className,
  size = 28,
}: ListeningToggleProps) {
  const enabled = useStore((s) => s.ui.listeningEnabled);
  const setListening = useStore((s) => s.setListening);
  const t = useT();

  const isOn = enabled === true;
  const ariaLabel = enabled === null
    ? t('tray.tooltip.listening') // neutral while booting
    : isOn
      ? t('tray.aria.toggleListening.on')
      : t('tray.aria.toggleListening.off');
  const tooltip = enabled === null ? undefined : isOn ? t('tray.tooltip.listening') : t('tray.tooltip.muted');

  return (
    <button
      type="button"
      className={`listening-toggle${className ? ` ${className}` : ''}`}
      style={{ width: size, height: size }}
      aria-pressed={enabled === null ? undefined : isOn}
      aria-label={ariaLabel}
      title={tooltip}
      disabled={enabled === null}
      onClick={() => {
        if (enabled === null) return;
        void setListening(!isOn);
      }}
    >
      <svg
        viewBox="0 0 24 24"
        fill={isOn ? 'currentColor' : 'none'}
        stroke="currentColor"
        strokeWidth="1.8"
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden="true"
      >
        {/* Mic capsule */}
        <rect x="9" y="3" width="6" height="12" rx="3" />
        <path d="M5 11a7 7 0 0 0 14 0" fill="none" />
        <line x1="12" y1="18" x2="12" y2="22" fill="none" />
        {!isOn && enabled !== null && (
          <line
            x1="4"
            y1="4"
            x2="20"
            y2="20"
            stroke="currentColor"
            strokeWidth="1.8"
            strokeLinecap="round"
          />
        )}
      </svg>
    </button>
  );
}
