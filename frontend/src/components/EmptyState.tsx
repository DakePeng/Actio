import React from 'react';
import { useT } from '../i18n';

type EmptyStateProps = {
  hasFilters?: boolean;
  title?: string;
  description?: string;
  eyebrow?: string;
  icon?: React.ReactNode;
};

export function EmptyState({ hasFilters, title, description, eyebrow, icon }: EmptyStateProps) {
  const t = useT();

  if (hasFilters) {
    return (
      <div className="empty-shell">
        <div className="empty-shell__inner">
          <h2 className="empty-shell__title">{t('empty.noResults.title')}</h2>
          <p className="empty-shell__copy">{t('empty.noResults.copy')}</p>
        </div>
      </div>
    );
  }

  return (
    <div className="empty-shell">
      <div className="empty-shell__inner">
        <div className="empty-shell__mark" aria-hidden="true">
          {icon || <div className="empty-pulse" />}
        </div>
        <div className="empty-shell__eyebrow">{eyebrow || t('empty.default.eyebrow')}</div>
        <h2 className="empty-shell__title">{title || t('empty.default.title')}</h2>
        <p className="empty-shell__copy">{description || t('empty.default.copy')}</p>
      </div>
    </div>
  );
}
