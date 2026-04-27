import type { Label } from '../types';

export function getLabelById(labels: Label[], id: string) {
  return labels.find((label) => label.id === id) ?? null;
}
