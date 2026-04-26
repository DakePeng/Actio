import { describe, expect, it } from 'vitest';
import { matchesShortcut, normalizeKey } from '../useKeyboardShortcuts';

/** Build a KeyboardEvent with just the modifier flags + key set. jsdom's
 *  KeyboardEvent supports the standard properties so we don't need a full
 *  spec-compliant event. */
function ev(opts: {
  key: string;
  ctrlKey?: boolean;
  metaKey?: boolean;
  shiftKey?: boolean;
  altKey?: boolean;
}): KeyboardEvent {
  return new KeyboardEvent('keydown', {
    key: opts.key,
    ctrlKey: opts.ctrlKey ?? false,
    metaKey: opts.metaKey ?? false,
    shiftKey: opts.shiftKey ?? false,
    altKey: opts.altKey ?? false,
  });
}

describe('normalizeKey', () => {
  it('translates the literal space character to "space"', () => {
    expect(normalizeKey(' ')).toBe('space');
  });

  it('lowercases letter keys', () => {
    expect(normalizeKey('A')).toBe('a');
    expect(normalizeKey('M')).toBe('m');
  });

  it('passes named keys through (already canonical)', () => {
    expect(normalizeKey('Enter')).toBe('enter');
    expect(normalizeKey('ArrowUp')).toBe('arrowup');
    expect(normalizeKey('Delete')).toBe('delete');
  });

  it('keeps backslash unchanged', () => {
    expect(normalizeKey('\\')).toBe('\\');
  });
});

describe('matchesShortcut — Ctrl combos', () => {
  it('matches "Ctrl+1" with ctrlKey + key=1', () => {
    expect(matchesShortcut(ev({ key: '1', ctrlKey: true }), 'Ctrl+1')).toBe(true);
  });

  it('does not match "Ctrl+1" without ctrlKey', () => {
    expect(matchesShortcut(ev({ key: '1' }), 'Ctrl+1')).toBe(false);
  });

  it('does not match "Ctrl+1" when only metaKey is pressed', () => {
    expect(matchesShortcut(ev({ key: '1', metaKey: true }), 'Ctrl+1')).toBe(false);
  });

  it('matches multi-modifier "Ctrl+Shift+M"', () => {
    expect(
      matchesShortcut(ev({ key: 'M', ctrlKey: true, shiftKey: true }), 'Ctrl+Shift+M'),
    ).toBe(true);
  });
});

describe('matchesShortcut — Meta/Cmd/Super aliases (macOS)', () => {
  // The recorder in KeyboardSettings.tsx writes "Meta+1" when the user presses
  // Cmd+1 on macOS. tauri-plugin-global-shortcut also accepts Super and Cmd.
  // The matcher needs to treat all of these as the same metaKey condition.
  it('matches "Meta+1" with metaKey pressed', () => {
    expect(matchesShortcut(ev({ key: '1', metaKey: true }), 'Meta+1')).toBe(true);
  });

  it('matches "Super+1" with metaKey pressed', () => {
    expect(matchesShortcut(ev({ key: '1', metaKey: true }), 'Super+1')).toBe(true);
  });

  it('matches "Cmd+1" with metaKey pressed', () => {
    expect(matchesShortcut(ev({ key: '1', metaKey: true }), 'Cmd+1')).toBe(true);
  });

  it('matches "Command+1" with metaKey pressed', () => {
    expect(matchesShortcut(ev({ key: '1', metaKey: true }), 'Command+1')).toBe(true);
  });

  it('does not match Meta combo without metaKey', () => {
    expect(matchesShortcut(ev({ key: '1' }), 'Meta+1')).toBe(false);
    expect(matchesShortcut(ev({ key: '1', ctrlKey: true }), 'Meta+1')).toBe(false);
  });
});

describe('matchesShortcut — Space normalization (#36)', () => {
  it('matches "Ctrl+Space" with ctrlKey + literal space character', () => {
    expect(matchesShortcut(ev({ key: ' ', ctrlKey: true }), 'Ctrl+Space')).toBe(true);
  });

  it('case-insensitive on the modifier names', () => {
    expect(matchesShortcut(ev({ key: '1', ctrlKey: true }), 'ctrl+1')).toBe(true);
    expect(matchesShortcut(ev({ key: '1', ctrlKey: true }), 'CTRL+1')).toBe(true);
  });
});

describe('matchesShortcut — extra modifier rejected', () => {
  it('does not match "Ctrl+1" when shiftKey is also pressed', () => {
    expect(
      matchesShortcut(ev({ key: '1', ctrlKey: true, shiftKey: true }), 'Ctrl+1'),
    ).toBe(false);
  });

  it('does not match "Ctrl+1" when altKey is also pressed', () => {
    expect(
      matchesShortcut(ev({ key: '1', ctrlKey: true, altKey: true }), 'Ctrl+1'),
    ).toBe(false);
  });
});

describe('matchesShortcut — option/control aliases', () => {
  it('treats "Control+M" as equivalent to "Ctrl+M"', () => {
    expect(matchesShortcut(ev({ key: 'M', ctrlKey: true }), 'Control+M')).toBe(true);
  });

  it('treats "Option+A" as equivalent to "Alt+A"', () => {
    expect(matchesShortcut(ev({ key: 'A', altKey: true }), 'Option+A')).toBe(true);
  });
});
