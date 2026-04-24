import { describe, it, expect } from 'vitest';
import { en } from '../locales/en';
import { zhCN } from '../locales/zh-CN';

describe('i18n parity', () => {
  it('zh-CN has exactly the same keys as en', () => {
    const enKeys = Object.keys(en).sort();
    const zhKeys = Object.keys(zhCN).sort();
    expect(zhKeys).toEqual(enKeys);
  });

  it('every translated string is non-empty', () => {
    for (const [key, value] of Object.entries(zhCN)) {
      expect(value, `zh-CN entry "${key}" is empty`).toBeTruthy();
    }
  });
});
