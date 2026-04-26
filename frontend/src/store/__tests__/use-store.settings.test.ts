import { beforeEach, describe, expect, it } from 'vitest';
import { useStore } from '../use-store';

describe('useStore settings actions', () => {
  beforeEach(() => {
    useStore.getState().reset();
    localStorage.clear();
  });

  it('defaults activeTab to board', () => {
    expect(useStore.getState().ui.activeTab).toBe('board');
  });

  it('sets activeTab and resets expandedCardId', () => {
    useStore.setState((s) => ({ ui: { ...s.ui, expandedCardId: 'r1' } }));
    useStore.getState().setActiveTab('archive');
    expect(useStore.getState().ui.activeTab).toBe('archive');
    expect(useStore.getState().ui.expandedCardId).toBeNull();
  });

  it('setActiveTab closes new reminder bar', () => {
    useStore.setState((s) => ({ ui: { ...s.ui, showNewReminderBar: true } }));
    useStore.getState().setActiveTab('settings');
    expect(useStore.getState().ui.showNewReminderBar).toBe(false);
  });

  it('setProfile merges patch into store state', () => {
    useStore.getState().setProfile({ display_name: 'Jane Doe' });
    expect(useStore.getState().profile.display_name).toBe('Jane Doe');
  });

  it('setPreferences merges patch and persists to localStorage', () => {
    useStore.getState().setPreferences({ theme: 'dark' });
    expect(useStore.getState().preferences.theme).toBe('dark');
    expect(JSON.parse(localStorage.getItem('actio-preferences') ?? '{}')).toMatchObject({ theme: 'dark' });
  });

  it('setPreferences does not overwrite unrelated fields', () => {
    useStore.getState().setPreferences({ notifications: false });
    useStore.getState().setPreferences({ theme: 'light' });
    expect(useStore.getState().preferences.notifications).toBe(false);
    expect(useStore.getState().preferences.theme).toBe('light');
  });
});
