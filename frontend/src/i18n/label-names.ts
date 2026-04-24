import type { TKey } from './locales/en';

/** Seeded default label names from `backend/actio-core/src/repository/label.rs`.
 *  When a label's stored name matches one of these exactly, the UI renders
 *  the translated form so existing users see localized defaults without any
 *  DB migration. Custom labels — or defaults the user has renamed — stay
 *  verbatim. */
const DEFAULT_LABEL_KEYS: Record<string, TKey> = {
  Work: 'label.default.work',
  Personal: 'label.default.personal',
  Urgent: 'label.default.urgent',
  Idea: 'label.default.idea',
  'Follow-up': 'label.default.followUp',
  Meeting: 'label.default.meeting',
};

/** Look up the translation key for a default label. Returns null for
 *  unrecognised (user-created) names so the caller can fall back to the
 *  stored value. */
export function defaultLabelKey(name: string): TKey | null {
  return DEFAULT_LABEL_KEYS[name] ?? null;
}

/** Convenience wrapper: given a `t` function and a label name, return the
 *  translated default name if it matches one of the seeded defaults, else
 *  return the stored name verbatim. */
export function translateLabelName(
  t: (key: TKey) => string,
  name: string,
): string {
  const key = defaultLabelKey(name);
  return key ? t(key) : name;
}
