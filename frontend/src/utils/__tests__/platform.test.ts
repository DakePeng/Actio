import { describe, expect, it } from 'vitest';
import { isMac, primaryMod } from '../platform';

/** These tests document the contract — `isMac` and `primaryMod` are derived
 *  from `navigator` at module-load time and don't change during a session.
 *  jsdom (the vitest test environment) reports `navigator.platform === ''` on
 *  most setups, so the derived values reflect the host where tests run.
 *
 *  We can't dynamically toggle these in tests without esmodule reloading,
 *  but we CAN assert their internal consistency. */
describe('platform.ts', () => {
  it('isMac is a boolean', () => {
    expect(typeof isMac).toBe('boolean');
  });

  it('primaryMod is exactly "Super" on Mac and exactly "Ctrl" elsewhere', () => {
    if (isMac) {
      expect(primaryMod).toBe('Super');
    } else {
      expect(primaryMod).toBe('Ctrl');
    }
  });

  it('primaryMod is one of the two supported tokens', () => {
    expect(['Super', 'Ctrl']).toContain(primaryMod);
  });
});
