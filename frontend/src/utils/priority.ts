import type { Priority } from '../types';

const priorityOrder: Record<Priority, number> = { high: 0, medium: 1, low: 2 };

export function sortByPriority<T extends { priority: Priority }>(a: T, b: T): number {
  return priorityOrder[a.priority] - priorityOrder[b.priority];
}

