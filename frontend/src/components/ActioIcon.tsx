export type ActioIconState = 'recording' | 'processing' | 'paused';

interface ActioIconProps {
  state: ActioIconState;
  size: number;
  className?: string;
}

export function ActioIcon({ state, size, className = '' }: ActioIconProps) {
  return (
    <span
      className={`actio-icon actio-icon--${state} ${className}`}
      style={{ fontSize: size }}
      aria-hidden="true"
    >
      A
    </span>
  );
}
