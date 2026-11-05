'use client';

import * as React from 'react';
import Link from 'next/link';

import { cn } from '@/utils/cn';

/** The two halves of "Saved", presented as one destination. */
export function SavedTabs({ active }: { active: 'bookmarks' | 'favorites' }) {
  const tabs = [
    { id: 'bookmarks' as const, href: '/bookmarks', label: 'Bookmarks' },
    { id: 'favorites' as const, href: '/favorites', label: 'Favorites' },
  ];

  return (
    <nav aria-label="Saved" className="mt-6 flex gap-6 border-b border-hairline sm:mt-8">
      {tabs.map((tab) => (
        <Link
          key={tab.id}
          href={tab.href}
          aria-current={active === tab.id ? 'page' : undefined}
          className={cn(
            '-mb-px inline-flex min-h-11 items-center border-b-2 pb-2.5 pt-1 font-mono text-[10.5px] uppercase tracking-[0.1em] transition-colors',
            active === tab.id
              ? 'border-accent text-accent'
              : 'border-transparent text-ink-subtle hover:border-hairline-strong hover:text-ink',
          )}
        >
          {tab.label}
        </Link>
      ))}
    </nav>
  );
}
