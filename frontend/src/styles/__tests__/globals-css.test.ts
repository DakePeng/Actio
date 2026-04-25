import { describe, expect, it } from 'vitest';

// @ts-expect-error Vitest executes this static stylesheet regression in Node; the app tsconfig intentionally omits Node types.
import { readFileSync } from 'node:fs';

const globalsCss = readFileSync('src/styles/globals.css', 'utf8');

function getKeyframesBody(name: string) {
  const start = globalsCss.indexOf(`@keyframes ${name}`);
  expect(start).toBeGreaterThanOrEqual(0);

  const openBrace = globalsCss.indexOf('{', start);
  expect(openBrace).toBeGreaterThanOrEqual(0);

  let depth = 0;
  for (let index = openBrace; index < globalsCss.length; index += 1) {
    const char = globalsCss[index];
    if (char === '{') depth += 1;
    if (char === '}') depth -= 1;
    if (depth === 0) {
      return globalsCss.slice(openBrace + 1, index);
    }
  }

  throw new Error(`Unable to parse @keyframes ${name}`);
}

describe('global CSS regressions', () => {
  it('keeps tray-launched card spotlight focused on green borders', () => {
    const spotlight = getKeyframesBody('cardSpotlight');

    expect(spotlight).toContain('border-color: var(--color-accent)');
    expect(spotlight).not.toMatch(/\bbackground\s*:/);
  });

  it('keeps the standby tray controls centered in an accessible compact header', () => {
    expect(globalsCss).toContain('--tray-collapsed-height: 78px');
    expect(globalsCss).toContain('min-height: var(--tray-collapsed-height)');
    expect(globalsCss).toContain('width: 44px');
    expect(globalsCss).toContain('height: 44px');
  });

  it('keeps live tray transcript text in a scrolling viewport', () => {
    expect(globalsCss).toContain('.tray-toggle--live');
    expect(globalsCss).toContain('.tray-transcript__viewport');
    expect(globalsCss).toContain('overflow-y: auto');
    expect(globalsCss).toContain('white-space: pre-wrap');
  });
});
