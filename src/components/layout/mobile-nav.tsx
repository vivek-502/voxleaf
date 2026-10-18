import * as React from 'react';
import { Link } from 'react-router-dom';
import { useLocation } from 'react-router-dom';

import { MOBILE_NAV, isNavActive } from './nav-items';
import { cn } from '@/utils/cn';

/**
 * Mobile bottom navigation.
 *
 * The bottom row of the shell's fixed stack (below the mini-player), and
 * hidden on `lg` where the sidebar takes over. Home-indicator inset is applied
 * here because this is always the last painted thing at the bottom edge.
 */
export function MobileNav() {
  const pathname = useLocation().pathname;

  return (
    <nav
      aria-label="Main"
      className="glass-surface border-t"
      style={{ paddingBottom: 'var(--safe-bottom)' }}
    >
      <ul className="mx-auto flex max-w-lg items-stretch">
        {MOBILE_NAV.map((item) => {
          const active = isNavActive(pathname, item.href);
          const Icon = item.icon;
          return (
            <li key={item.href} className="flex-1">
              <Link
                to={item.href}
                aria-current={active ? 'page' : undefined}
                className={cn(
                  'flex h-[58px] flex-col items-center justify-center gap-1 text-[10px] font-medium transition-colors duration-150',
                  active ? 'text-accent-soft' : 'text-ink-subtle hover:text-ink-muted',
                )}
              >
                <Icon className="size-[21px]" aria-hidden />
                <span>{item.label}</span>
              </Link>
            </li>
          );
        })}
      </ul>
    </nav>
  );
}
