'use client';

import * as React from 'react';
import Link from 'next/link';
import { usePathname } from 'next/navigation';

import { MOBILE_NAV, isNavActive } from './nav-items';
import { cn } from '@/utils/cn';

/**
 * Mobile bottom navigation.
 *
 * The bottom row of the shell's fixed stack (below the mini-player), and
 * hidden on `lg` where the masthead carries the section links. Home-indicator
 * inset is applied here because this is always the last painted thing at the
 * bottom edge.
 */
export function MobileNav() {
  const pathname = usePathname();

  return (
    <nav
      aria-label="Sections"
      className="border-t border-hairline bg-surface/95 backdrop-blur-md"
      style={{ paddingBottom: 'var(--safe-bottom)' }}
    >
      <ul className="mx-auto flex max-w-lg items-stretch">
        {MOBILE_NAV.map((item) => {
          const active = isNavActive(pathname, item.href);
          const Icon = item.icon;
          return (
            <li key={item.href} className="relative flex-1">
              <Link
                href={item.href}
                aria-current={active ? 'page' : undefined}
                aria-label={`${item.label} — ${item.description}`}
                className={cn(
                  'flex h-[60px] flex-col items-center justify-center gap-1.5 transition-colors duration-150',
                  active ? 'text-accent' : 'text-ink-subtle hover:text-ink',
                )}
              >
                {active && (
                  <span aria-hidden className="absolute inset-x-5 top-0 h-[2px] bg-accent" />
                )}
                <Icon className="size-[19px]" strokeWidth={active ? 1.9 : 1.6} aria-hidden />
                <span className="font-mono text-[9px] font-medium uppercase tracking-[0.1em]">
                  {item.label}
                </span>
              </Link>
            </li>
          );
        })}
      </ul>
    </nav>
  );
}
