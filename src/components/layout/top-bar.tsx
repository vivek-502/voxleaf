import * as React from 'react';
import { useLocation, useNavigate, useSearchParams } from 'react-router-dom';
import { Monitor, Moon, Search, Sun, X } from 'lucide-react';

import { MobileLogo } from './sidebar';
import { Button } from '@/components/ui/button';
import { TooltipWrap } from '@/components/ui/tooltip';
import { useTheme } from '@/components/providers/theme-provider';
import type { ThemeId } from '@/types';

const THEME_ORDER: ThemeId[] = ['light', 'dark', 'system'];

/**
 * Sticky page header: branding (mobile), search, and theme switch.
 *
 * Search lives here rather than on the Store page alone so it is reachable
 * from anywhere. Enter navigates to `/store`; the field stays uncontrolled
 * except when the route is already the store, where it mirrors the URL query.
 */
export function TopBar() {
  const navigate = useNavigate();
  const pathname = useLocation().pathname;
  const [searchParams] = useSearchParams();
  const { theme, resolved, setTheme } = useTheme();
  const inputRef = React.useRef<HTMLInputElement>(null);

  const onStore = pathname === '/store';
  const urlQuery = onStore ? (searchParams.get('q') ?? '') : '';
  const [value, setValue] = React.useState(urlQuery);

  // Keep in sync when the store page changes the query (chips, clear, back/forward).
  React.useEffect(() => {
    if (onStore) setValue(urlQuery);
  }, [onStore, urlQuery]);

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    const term = value.trim();
    navigate(term ? `/store?q=${encodeURIComponent(term)}` : '/store');
  };

  const ThemeIcon = theme === 'system' ? Monitor : resolved === 'light' ? Sun : Moon;
  const nextTheme = THEME_ORDER[(THEME_ORDER.indexOf(theme) + 1) % THEME_ORDER.length];

  return (
    <header className="sticky top-0 z-30 glass-surface border-b border-b-transparent border-x-0 border-t-0">
      <div className="container-page flex h-16 items-center gap-3">
        <div className="lg:hidden">
          <MobileLogo />
        </div>

        <form role="search" onSubmit={submit} className="relative min-w-0 flex-1 max-w-2xl">
          <label htmlFor="global-search" className="sr-only-focusable">
            Search books
          </label>
          <Search
            className="pointer-events-none absolute top-1/2 left-3.5 size-4 -translate-y-1/2 text-ink-subtle"
            aria-hidden
          />
          <input
            id="global-search"
            ref={inputRef}
            type="search"
            name="q"
            value={value}
            onChange={(event) => setValue(event.target.value)}
            placeholder="Search books, authors…"
            autoComplete="off"
            enterKeyHint="search"
            className="h-10 w-full rounded-full border border-hairline bg-raised pr-9 pl-10 text-sm text-ink placeholder:text-ink-subtle focus:border-accent-line focus:outline-none"
          />
          {value.length > 0 && (
            <button
              type="button"
              onClick={() => {
                setValue('');
                inputRef.current?.focus();
              }}
              aria-label="Clear search"
              className="absolute top-1/2 right-2.5 grid size-6 -translate-y-1/2 place-items-center rounded-full text-ink-subtle transition-colors hover:bg-overlay hover:text-ink"
            >
              <X className="size-3.5" aria-hidden />
            </button>
          )}
        </form>

        <TooltipWrap label={`Theme: ${theme}`}>
          <Button
            variant="ghost"
            size="icon"
            onClick={() => setTheme(nextTheme)}
            aria-label={`Switch theme, currently ${theme}`}
            className="ml-auto shrink-0"
          >
            <ThemeIcon className="size-[18px]" aria-hidden />
          </Button>
        </TooltipWrap>
      </div>
    </header>
  );
}
