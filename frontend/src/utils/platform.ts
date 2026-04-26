/** Platform detection — used by keyboard-shortcut code to pick the right
 *  primary modifier (Cmd on macOS, Ctrl elsewhere). `navigator.userAgentData`
 *  is the future-proof API; `navigator.platform` is the legacy fallback that
 *  still works in WebView2, WKWebView, and WebKitGTK. Either reading the
 *  platform string at module load is fine — Tauri windows don't migrate
 *  between OSes mid-session. */
export const isMac: boolean =
  typeof navigator !== 'undefined' &&
  (
    // @ts-expect-error — userAgentData is non-standard in the type defs
    (navigator.userAgentData?.platform &&
      // @ts-expect-error
      /mac/i.test(navigator.userAgentData.platform)) ||
    /mac/i.test(navigator.platform || '') ||
    /mac/i.test(navigator.userAgent || '')
  );

/** Modifier string used in shortcut definitions for the primary OS modifier.
 *  `Super` on macOS (parsed by tauri-plugin-global-shortcut as Cmd / by the
 *  global-hotkey crate as the Meta key) and `Ctrl` everywhere else.
 *  When written into a string like "Super+\\", both the Tauri global shortcut
 *  parser AND the in-process DOM matcher treat the corresponding key as the
 *  bound trigger. */
export const primaryMod: 'Super' | 'Ctrl' = isMac ? 'Super' : 'Ctrl';
