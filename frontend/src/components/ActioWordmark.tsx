import { memo } from 'react';

export type WordmarkState =
  | 'standby'
  | 'listening'
  | 'transcribing'
  | 'processing'
  | 'success'
  | 'error';

export interface ActioWordmarkProps {
  state?: WordmarkState;
  height?: number;
  ariaLabel?: string;
  className?: string;
  /** Tray/compact view — only the primary 'a' glyph renders; the secondary
   *  'ctio' marks, the standby cursor, and the processing scan-beam are all
   *  omitted, and the viewBox tightens around the primary mark. */
  compact?: boolean;
}

// Geometric wordmark (a + c + t + i + o). State animations live in globals.css
// under .actio-wm, scoped by the .actio-wm--<state> modifier on the root.
export const ActioWordmark = memo(function ActioWordmark({
  state = 'standby',
  height = 24,
  ariaLabel,
  className,
  compact = false,
}: ActioWordmarkProps) {
  // Compact viewBox frames the primary 'a' and leaves room for the ring /
  // spin-arc overlays without clipping them.
  const viewBox = compact ? '0 16 50 48' : '0 0 260 70';
  const [, , vbW, vbH] = viewBox.split(' ').map(Number);
  const width = Math.round((height * vbW) / vbH);
  const rootClass = [
    'actio-wm',
    `actio-wm--${state}`,
    compact ? 'actio-wm--compact' : null,
    className,
  ]
    .filter(Boolean)
    .join(' ');

  return (
    <svg
      className={rootClass}
      width={width}
      height={height}
      viewBox={viewBox}
      fill="none"
      stroke="currentColor"
      strokeWidth="3"
      strokeLinecap="round"
      strokeLinejoin="round"
      role={ariaLabel ? 'img' : undefined}
      aria-label={ariaLabel}
      aria-hidden={ariaLabel ? undefined : true}
    >
      {state === 'listening' && (
        <g fill="none" stroke="currentColor" strokeWidth="1">
          <circle className="actio-wm__ring actio-wm__ring--1" cx="22" cy="40" r="10" />
          <circle className="actio-wm__ring actio-wm__ring--2" cx="22" cy="40" r="10" />
          <circle className="actio-wm__ring actio-wm__ring--3" cx="22" cy="40" r="10" />
        </g>
      )}

      {state === 'transcribing' && (
        <>
          <g
            className="actio-wm__target"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.4"
            strokeDasharray="2 3"
            strokeLinecap="round"
          >
            <circle cx="22" cy="40" r="10" />
            <line x1="22" y1="26" x2="22" y2="32" />
            <line x1="22" y1="48" x2="22" y2="54" />
            <line x1="8" y1="40" x2="14" y2="40" />
            <line x1="30" y1="40" x2="36" y2="40" />
          </g>
          <circle cx="22" cy="40" r="2" fill="currentColor" stroke="none" />
        </>
      )}

      {state === 'processing' && (
        <>
          <path
            className="actio-wm__spin-arc"
            d="M 22 23 A 17 17 0 0 1 39 40"
            fill="none"
            stroke="currentColor"
            strokeWidth="2.8"
            strokeLinecap="round"
          />
          {/* Scan beam only makes sense across the full wordmark — in
           *  compact view the spin-arc is the sole processing indicator. */}
          {!compact && (
            <rect
              className="actio-wm__scan-beam"
              x="-6"
              y="24"
              width="6"
              height="28"
              rx="3"
              fill="currentColor"
              opacity="0.12"
            />
          )}
        </>
      )}

      {state === 'error' && (
        <g fill="none" strokeWidth="1">
          <circle className="actio-wm__err-ring actio-wm__err-ring--1" cx="22" cy="40" r="10" />
          <circle className="actio-wm__err-ring actio-wm__err-ring--2" cx="22" cy="40" r="10" />
          <circle className="actio-wm__err-ring actio-wm__err-ring--3" cx="22" cy="40" r="10" />
        </g>
      )}

      <g className="actio-wm__glyph-group">
        {state === 'listening' && (
          <circle
            className="actio-wm__center-dot"
            cx="22"
            cy="40"
            r="3.4"
            fill="currentColor"
            stroke="none"
          />
        )}

        {/* primary 'a' glyph — gets the error/success accent color */}
        <g className="actio-wm__primary-mark">
          <circle cx="22" cy="40" r="12" />
          <line x1="34" y1="28" x2="34" y2="52" />
        </g>

        {/* secondary 'c', 't', 'i', 'o' glyphs — omitted in compact mode */}
        {!compact && (
          <g
            className={
              state === 'processing'
                ? 'actio-wm__secondary-mark actio-wm__dashed'
                : 'actio-wm__secondary-mark'
            }
          >
            <path d="M80 30 a12 12 0 1 0 0 20" />
            <path d="M110 22 v30 a4 4 0 0 0 4 4 h2" />
            <line x1="104" y1="32" x2="120" y2="32" />
            <circle cx="145" cy="24" r="1.6" fill="currentColor" stroke="none" />
            <line x1="145" y1="32" x2="145" y2="52" />
            <circle cx="180" cy="40" r="12" />
            {state === 'standby' && (
              <line
                className="actio-wm__cursor"
                x1="210"
                y1="28"
                x2="210"
                y2="52"
                strokeWidth="3.2"
              />
            )}
            {state === 'transcribing' && (
              <>
                <line
                  className="actio-wm__tick-mark"
                  x1="50"
                  y1="56"
                  x2="210"
                  y2="56"
                  stroke="currentColor"
                  strokeWidth="1"
                  strokeDasharray="2 4"
                />
                <line
                  className="actio-wm__caret"
                  x1="50"
                  y1="28"
                  x2="50"
                  y2="54"
                  stroke="currentColor"
                  strokeWidth="1.6"
                />
              </>
            )}
          </g>
        )}
      </g>
    </svg>
  );
});
