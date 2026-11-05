'use client';

import * as React from 'react';
import Link from 'next/link';
import { ArrowRight } from 'lucide-react';

import { cn } from '@/utils/cn';

export interface SectionHeaderProps {
  title: string;
  description?: string;
  /** Two-digit folio number, shown to the left of the heading. */
  index?: string;
  /** Shows an "All …" link when provided. */
  href?: string;
  linkLabel?: string;
  action?: React.ReactNode;
  className?: string;
}

/** Folio-style section heading: number, rule, serif title, trailing link. */
export function SectionHeader({
  title,
  description,
  index,
  href,
  linkLabel = 'See all',
  action,
  className,
}: SectionHeaderProps) {
  return (
    <div className={cn('mb-5', className)}>
      <div className="flex flex-wrap items-end justify-between gap-3 border-b border-ink pb-2.5">
        <div className="flex min-w-0 items-baseline gap-3.5">
          {index && (
            <span className="font-mono text-[10px] tracking-[0.1em] text-ink-subtle tabular-nums">
              {index}
            </span>
          )}
          <h2 className="font-display text-[22px] leading-none font-medium tracking-[-0.02em] text-ink sm:text-[26px]">
            {title}
          </h2>
        </div>

        {action ??
          (href && (
            <Link
              href={href}
              className="group inline-flex shrink-0 items-center gap-1.5 font-mono text-[10px] uppercase tracking-[0.1em] text-accent transition-colors hover:text-accent-deep"
            >
              {linkLabel}
              <ArrowRight
                className="size-3 transition-transform duration-200 group-hover:translate-x-0.5"
                aria-hidden
              />
            </Link>
          ))}
      </div>
      {description && (
        <p className="mt-2 max-w-2xl text-[13px] leading-relaxed text-ink-muted">{description}</p>
      )}
    </div>
  );
}

/* -------------------------------------------------------------------------- */
/* Page header                                                                 */
/* -------------------------------------------------------------------------- */

export interface PageHeaderProps {
  title: string;
  description?: React.ReactNode;
  /** Small line above the title — a section name or folio marker. */
  eyebrow?: React.ReactNode;
  actions?: React.ReactNode;
  className?: string;
}

/** Top-of-page title block, used by every non-home page. */
export function PageHeader({ title, description, eyebrow, actions, className }: PageHeaderProps) {
  return (
    <header className={cn('border-b border-ink pb-6', className)}>
      {eyebrow && <div className="eyebrow mb-3">{eyebrow}</div>}
      <div className="flex flex-wrap items-end justify-between gap-4">
        <div className="min-w-0">
          <h1 className="font-display text-[34px] leading-[1.02] font-medium tracking-[-0.025em] text-ink sm:text-[44px]">
            {title}
          </h1>
          {description && (
            <div className="mt-3 max-w-2xl text-[13.5px] leading-relaxed text-ink-muted">
              {description}
            </div>
          )}
        </div>
        {actions && <div className="flex shrink-0 flex-wrap items-center gap-2">{actions}</div>}
      </div>
    </header>
  );
}
