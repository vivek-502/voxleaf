import * as React from 'react';
import { Link } from 'react-router-dom';
import { ArrowRight } from 'lucide-react';

import { cn } from '@/utils/cn';

export interface SectionHeaderProps {
  title: string;
  description?: string;
  /** Shows a "See all →" link when provided. */
  href?: string;
  action?: React.ReactNode;
  className?: string;
}

/** Section heading with an optional trailing action. */
export function SectionHeader({
  title,
  description,
  href,
  action,
  className,
}: SectionHeaderProps) {
  return (
    <div className={cn('mb-4 flex items-end justify-between gap-4', className)}>
      <div className="min-w-0">
        <h2 className="text-lg font-semibold text-ink sm:text-xl">{title}</h2>
        {description && <p className="mt-1 text-sm text-ink-muted">{description}</p>}
      </div>

      {action ??
        (href && (
          <Link
            to={href}
            className="group flex shrink-0 items-center gap-1 text-sm font-medium text-accent-soft transition-colors hover:text-accent"
          >
            See all
            <ArrowRight
              className="size-4 transition-transform duration-200 group-hover:translate-x-0.5"
              aria-hidden
            />
          </Link>
        ))}
    </div>
  );
}

/* -------------------------------------------------------------------------- */
/* Page header                                                                 */
/* -------------------------------------------------------------------------- */

export interface PageHeaderProps {
  title: string;
  description?: React.ReactNode;
  actions?: React.ReactNode;
  className?: string;
}

/** Top-of-page title block, used by every non-home page. */
export function PageHeader({ title, description, actions, className }: PageHeaderProps) {
  return (
    <header className={cn('mb-7 flex flex-wrap items-start justify-between gap-4', className)}>
      <div className="min-w-0">
        <h1 className="text-2xl font-semibold text-ink sm:text-3xl">{title}</h1>
        {description && (
          <div className="mt-1.5 max-w-2xl text-sm leading-relaxed text-ink-muted">{description}</div>
        )}
      </div>
      {actions && <div className="flex shrink-0 flex-wrap items-center gap-2">{actions}</div>}
    </header>
  );
}
