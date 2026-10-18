import * as React from 'react';
import { Link } from 'react-router-dom';
import { useLocation } from 'react-router-dom';
import { AudioLines, LibraryBig } from 'lucide-react';

import { PRIMARY_NAV, SECONDARY_NAV, isNavActive } from './nav-items';
import { cn } from '@/utils/cn';

/**
 * Desktop navigation rail.
 *
 * A plain `<nav>` of links — deliberately not a client-side store, so it stays
 * keyboard-navigable and screen-reader friendly without extra ARIA plumbing.
 */
export function Sidebar() {
  const pathname = useLocation().pathname;

  return (
    <aside className="sticky top-0 hidden h-dvh w-[248px] shrink-0 flex-col border-r border-hairline bg-surface/60 px-3 py-5 lg:flex">
      <Link
        to="/"
        className="mb-7 flex items-center gap-2.5 rounded-xl px-2 py-1.5 transition-colors hover:bg-overlay"
      >
        <span className="grid size-9 place-items-center rounded-xl bg-accent text-white shadow-[0_6px_18px_-6px_var(--color-accent)]">
          <AudioLines className="size-5" aria-hidden />
        </span>
        <span className="min-w-0">
          <span className="block text-[15px] font-semibold leading-tight text-ink">VoxLeaf</span>
          <span className="block truncate text-[11px] leading-tight text-ink-subtle">
            Audiobook library
          </span>
        </span>
      </Link>

      <nav aria-label="Main" className="flex-1 overflow-y-auto no-scrollbar">
        <ul className="flex flex-col gap-0.5">
          {PRIMARY_NAV.map((item) => (
            <SidebarLink key={item.href} item={item} active={isNavActive(pathname, item.href)} />
          ))}
        </ul>

        <p className="mt-7 mb-2 px-3 text-[11px] font-semibold uppercase tracking-[0.14em] text-ink-subtle">
          Your collection
        </p>
        <ul className="flex flex-col gap-0.5">
          {SECONDARY_NAV.map((item) => (
            <SidebarLink key={item.href} item={item} active={isNavActive(pathname, item.href)} />
          ))}
        </ul>
      </nav>

      <p className="mt-4 px-3 text-[11px] leading-relaxed text-ink-subtle">
        Single-user app. No account, no sign-in — everything is stored locally.
      </p>
    </aside>
  );
}

function SidebarLink({
  item,
  active,
}: {
  item: (typeof PRIMARY_NAV)[number];
  active: boolean;
}) {
  const Icon = item.icon;
  return (
    <li>
      <Link
        to={item.href}
        aria-current={active ? 'page' : undefined}
        className={cn(
          'group relative flex items-center gap-3 rounded-xl px-3 py-2.5 text-sm transition-colors duration-150',
          active
            ? 'bg-accent-wash font-semibold text-accent-soft'
            : 'text-ink-muted hover:bg-overlay hover:text-ink',
        )}
      >
        <span
          aria-hidden
          className={cn(
            'absolute top-1/2 -left-3 h-5 w-[3px] -translate-y-1/2 rounded-r-full bg-accent transition-opacity duration-150',
            active ? 'opacity-100' : 'opacity-0',
          )}
        />
        <Icon className="size-[18px] shrink-0" aria-hidden />
        <span className="truncate">{item.label}</span>
      </Link>
    </li>
  );
}

/** Compact logo used by the mobile top bar. */
export function MobileLogo() {
  return (
    <Link to="/" className="flex items-center gap-2" aria-label="VoxLeaf home">
      <span className="grid size-8 place-items-center rounded-lg bg-accent text-white">
        <LibraryBig className="size-[18px]" aria-hidden />
      </span>
      <span className="text-[15px] font-semibold text-ink">VoxLeaf</span>
    </Link>
  );
}
