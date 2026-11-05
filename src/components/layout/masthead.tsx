'use client';

import * as React from 'react';
import Link from 'next/link';
import { usePathname, useRouter, useSearchParams } from 'next/navigation';
import { Monitor, Moon, Search, Settings, Sun, X } from 'lucide-react';

import { PRIMARY_NAV, isNavActive } from './nav-items';
import { Button } from '@/components/ui/button';
import { TooltipWrap } from '@/components/ui/tooltip';
import { useTheme } from '@/components/providers/theme-provider';
import { cn } from '@/utils/cn';
import type { ThemeId } from '@/types';

const THEME_ORDER: ThemeId[] = ['light', 'dark', 'system'];

/**
 * The masthead.
 *
 * A single horizontal header rather than a sidebar: the app's identity is a
 * printed front page, and a masthead lets the wordmark, the section links and
 * the search sit together on one ruled line. On small screens the links move
 * to the bottom bar and search folds into a second row.
 */
export function Masthead() {
  const router = useRouter();
  const pathname = usePathname();
  const searchParams = useSearchParams();
  const { theme, resolved, setTheme } = useTheme();
  const inputRef = React.useRef<HTMLInputElement>(null);
  const mobileInputRef = React.useRef<HTMLInputElement>(null);
  const [searchOpen, setSearchOpen] = React.useState(false);

  const onStore = pathname === '/store';
  const urlQuery = onStore ? (searchParams.get('q') ?? '') : '';
  const [value, setValue] = React.useState(urlQuery);

  React.useEffect(() => {
    if (onStore) setValue(urlQuery);
  }, [onStore, urlQuery]);

  React.useEffect(() => {
    setSearchOpen(false);
  }, [pathname]);

  React.useEffect(() => {
    if (searchOpen) mobileInputRef.current?.focus();
  }, [searchOpen]);

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    const term = value.trim();
    router.push(term ? `/store?q=${encodeURIComponent(term)}` : '/store');
    setSearchOpen(false);
  };

  const ThemeIcon = theme === 'system' ? Monitor : resolved === 'light' ? Sun : Moon;
  const nextTheme = THEME_ORDER[(THEME_ORDER.indexOf(theme) + 1) % THEME_ORDER.length];

  const searchField = (id: string, ref: React.RefObject<HTMLInputElement | null>, compact = false) => (
    <form role="search" onSubmit={submit} className="relative min-w-0">
      <label htmlFor={id} className="sr-only-focusable">
        Search books and authors
      </label>
      <Search
        className="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-ink-subtle"
        aria-hidden
      />
      <input
        id={id}
        ref={ref}
        type="search"
        name="q"
        value={value}
        onChange={(event) => setValue(event.target.value)}
        placeholder={compact ? 'Search' : 'Search books, authors…'}
        autoComplete="off"
        enterKeyHint="search"
        className="h-9 w-full rounded-[3px] border border-hairline bg-transparent pr-8 pl-8 text-[13px] text-ink placeholder:text-ink-subtle transition-colors hover:border-hairline-strong focus:border-accent focus:bg-raised focus:outline-none"
      />
      {value.length > 0 && (
        <button
          type="button"
          onClick={() => {
            setValue('');
            ref.current?.focus();
          }}
          aria-label="Clear search"
          className="absolute top-1/2 right-1.5 grid size-6 -translate-y-1/2 place-items-center rounded-[2px] text-ink-subtle transition-colors hover:bg-overlay hover:text-ink"
        >
          <X className="size-3.5" aria-hidden />
        </button>
      )}
    </form>
  );

  return (
    <header className="sticky top-0 z-40 border-b border-hairline bg-canvas/92 backdrop-blur-md">
      <div className="container-page flex h-16 items-center gap-3 sm:gap-5">
        <Link href="/" aria-label="VoiceBook home" className="flex shrink-0 items-baseline gap-1.5">
          <span className="font-display text-[24px] leading-none font-medium tracking-[-0.025em] text-ink">
            VoiceBook
          </span>
          <span className="text-[24px] leading-none text-accent" aria-hidden>
            .
          </span>
        </Link>

        <nav aria-label="Sections" className="hidden h-16 items-stretch lg:ml-4 lg:flex">
          <ul className="flex items-stretch gap-1">
            {PRIMARY_NAV.map((item) => {
              const active = isNavActive(pathname, item.href);
              return (
                <li key={item.href} className="flex">
                  <Link
                    href={item.href}
                    aria-current={active ? 'page' : undefined}
                    className={cn(
                      'relative flex items-center px-2.5 font-mono text-[10.5px] uppercase tracking-[0.12em] transition-colors xl:px-3',
                      active ? 'text-ink' : 'text-ink-subtle hover:text-ink',
                    )}
                  >
                    {item.label}
                    <span
                      aria-hidden
                      className={cn(
                        'absolute inset-x-2 bottom-0 h-[2px] bg-accent transition-opacity xl:inset-x-3',
                        active ? 'opacity-100' : 'opacity-0',
                      )}
                    />
                  </Link>
                </li>
              );
            })}
          </ul>
        </nav>

        <div className="ml-auto flex items-center gap-1.5 sm:gap-2">
          <div className="hidden w-56 md:block xl:w-72">{searchField('global-search', inputRef)}</div>

          <Button
            variant="ghost"
            size="icon-sm"
            onClick={() => setSearchOpen((open) => !open)}
            aria-label={searchOpen ? 'Close search' : 'Search books'}
            aria-expanded={searchOpen}
            className="md:hidden"
          >
            {searchOpen ? <X className="size-4" aria-hidden /> : <Search className="size-4" aria-hidden />}
          </Button>

          <TooltipWrap label={`Theme: ${theme}`}>
            <Button
              variant="ghost"
              size="icon-sm"
              onClick={() => setTheme(nextTheme)}
              aria-label={`Switch theme, currently ${theme}`}
            >
              <ThemeIcon className="size-4" aria-hidden />
            </Button>
          </TooltipWrap>

          <TooltipWrap label="Settings">
            <Button variant="ghost" size="icon-sm" asChild>
              <Link href="/settings" aria-label="Settings">
                <Settings className="size-4" aria-hidden />
              </Link>
            </Button>
          </TooltipWrap>

        </div>
      </div>

      {searchOpen && (
        <div className="container-page pb-3 md:hidden">
          {searchField('mobile-search', mobileInputRef, true)}
        </div>
      )}
    </header>
  );
}
