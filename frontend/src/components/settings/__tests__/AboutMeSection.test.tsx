import { render, screen, fireEvent } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('../../../api/profile', () => ({
  fetchProfile: vi.fn().mockResolvedValue(null),
  updateProfile: vi.fn().mockResolvedValue({
    tenant_id: '00000000-0000-0000-0000-000000000000',
    display_name: '',
    aliases: [],
    bio: null,
  }),
}));

import { updateProfile } from '../../../api/profile';
import { useStore } from '../../../store/use-store';
import { ProfileSection } from '../ProfileSection';
import { LanguageProvider } from '../../../i18n';

beforeEach(() => {
  vi.clearAllMocks();
  localStorage.clear();
  Object.defineProperty(navigator, 'language', { value: 'en-US', configurable: true });
  useStore.setState({ profile: { display_name: '', aliases: [], bio: '', loaded: false } });
});

function renderSection() {
  return render(
    <LanguageProvider>
      <ProfileSection />
    </LanguageProvider>,
  );
}

describe('ProfileSection (About me)', () => {
  it('adds an alias on Enter and dedupes case-insensitively for ASCII', () => {
    renderSection();
    const input = screen.getByPlaceholderText(/Type and press Enter/i) as HTMLInputElement;

    fireEvent.change(input, { target: { value: 'Dake' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(screen.getByText('Dake')).toBeInTheDocument();

    fireEvent.change(input, { target: { value: 'dake' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    // Only one chip with that text (case-insensitive dedup)
    expect(screen.getAllByText(/^dake$/i).length).toBe(1);
  });

  it('preserves CJK alias verbatim', () => {
    renderSection();
    const input = screen.getByPlaceholderText(/Type and press Enter/i) as HTMLInputElement;

    fireEvent.change(input, { target: { value: '彭大可' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(screen.getByText('彭大可')).toBeInTheDocument();
  });

  it('Save triggers updateProfile exactly once', () => {
    renderSection();
    fireEvent.click(screen.getByText(/^Save$/i));
    expect(updateProfile).toHaveBeenCalledTimes(1);
  });
});
