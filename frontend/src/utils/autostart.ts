/** Launch-at-login control via tauri-plugin-autostart. The plugin handles
 *  per-OS plumbing — Windows registry Run key, macOS LaunchAgent .plist,
 *  Linux XDG autostart .desktop file — so the JS side only sees a boolean.
 *
 *  In dev/web mode (no Tauri runtime) every call is a no-op: the preference
 *  is still stored in localStorage by the caller, but we don't try to talk
 *  to a plugin that isn't loaded.
 *
 *  `@tauri-apps/api/core` is imported dynamically so the Tauri runtime
 *  surface code-splits out of the main chunk (see ISSUES.md #51). */
const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

export async function setAutostart(enabled: boolean): Promise<void> {
  if (!isTauri) return;
  const { invoke } = await import('@tauri-apps/api/core');
  if (enabled) {
    await invoke('plugin:autostart|enable');
  } else {
    await invoke('plugin:autostart|disable');
  }
}

export async function isAutostartEnabled(): Promise<boolean | null> {
  if (!isTauri) return null;
  try {
    const { invoke } = await import('@tauri-apps/api/core');
    return await invoke<boolean>('plugin:autostart|is_enabled');
  } catch (e) {
    console.error('[Actio] Failed to query autostart state:', e);
    return null;
  }
}
