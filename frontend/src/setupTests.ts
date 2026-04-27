import '@testing-library/jest-dom';
import { beforeEach, vi } from 'vitest';

// jsdom's `fetch` reaches the host network, so on a dev box where the real
// Actio backend is running on :3000 the `LanguageProvider` mount-effect
// fetches `/settings`, sees the user's actual locale, and flips the rendered
// UI to that language mid-test (e.g., from English to zh-CN). Tests that
// assert against English copy then break in a way that depends on the dev
// machine's state. Stub `fetch` to reject by default so tests are
// deterministic; individual tests can override via `vi.spyOn(globalThis,
// 'fetch')` when they need a specific response.
beforeEach(() => {
  vi.spyOn(globalThis, 'fetch').mockRejectedValue(new Error('offline (test default)'));
});
