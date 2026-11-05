'use client';

import * as React from 'react';
import { useRouter, useSearchParams } from 'next/navigation';
import { Search, Sparkles, X } from 'lucide-react';

import { BookGrid } from '@/components/book/book-card';
import { Pagination } from '@/components/common/pagination';
import { PageHeader } from '@/components/common/section-header';
import { BookGridSkeleton, EmptyState, NoResults } from '@/components/common/states';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/inputs';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { useApiQuery } from '@/hooks/use-async';
import { GENRE_FILTERS, PAGE_SIZE, STORE_SORTS, type StoreSort } from '@/utils/constants';
import { cn } from '@/utils/cn';
import type { BookSearchResult } from '@/types';

/**
 * Discover: search the public catalogue and shelve what you want.
 *
 * The query lives in the URL, so results are linkable, survive a refresh and
 * work with browser back/forward. Typing updates the URL on a debounce and
 * keeps the previous page's results visible while the next request runs.
 */
export function StoreBrowser() {
  const router = useRouter();
  const searchParams = useSearchParams();

  const query = searchParams.get('q') ?? '';
  const genre = searchParams.get('genre') ?? 'all';
  const sort = (searchParams.get('sort') ?? 'relevance') as StoreSort;
  const page = Math.max(1, Number(searchParams.get('page') ?? '1') || 1);

  // Local mirror of the search box so typing feels instant.
  const [term, setTerm] = React.useState(query);
  const debounceRef = React.useRef<ReturnType<typeof setTimeout> | null>(null);
  const inputRef = React.useRef<HTMLInputElement>(null);

  React.useEffect(() => setTerm(query), [query]);

  const push = React.useCallback(
    (next: Record<string, string | number | undefined>) => {
      const params = new URLSearchParams(searchParams.toString());
      for (const [key, value] of Object.entries(next)) {
        if (value === undefined || value === '' || value === 'all' || value === 1) {
          if (key === 'page') params.delete('page');
          else params.delete(key);
        } else {
          params.set(key, String(value));
        }
      }
      const search = params.toString();
      router.push(search ? `/store?${search}` : '/store');
    },
    [router, searchParams],
  );

  // Debounced: 350ms is long enough to skip intermediate keystrokes and short
  // enough that the results feel connected to typing.
  const onTermChange = React.useCallback(
    (value: string) => {
      setTerm(value);
      if (debounceRef.current) clearTimeout(debounceRef.current);
      debounceRef.current = setTimeout(() => {
        push({ q: value, page: undefined });
      }, 350);
    },
    [push],
  );

  React.useEffect(
    () => () => {
      if (debounceRef.current) clearTimeout(debounceRef.current);
    },
    [],
  );

  // Escape clears the box, matching the platform convention for search fields.
  const onSearchKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'Escape' && term) {
      event.preventDefault();
      setTerm('');
      push({ q: undefined, page: undefined });
    }
  };

  const clearSearch = () => {
    setTerm('');
    push({ q: undefined, page: undefined });
    inputRef.current?.focus();
  };

  const { data, error, loading, reload } = useApiQuery<BookSearchResult>('/api/books/search', {
    q: query,
    genre: genre === 'all' ? undefined : genre,
    sort,
    page,
    pageSize: PAGE_SIZE,
  });

  const hasQuery = Boolean(query.trim()) || genre !== 'all';
  const showSkeleton = loading && !data;

  return (
    <div className="container-page pb-16">
      <PageHeader
        className="mt-8 sm:mt-10"
        eyebrow="The catalogue"
        title="Discover"
        description="Search the public catalogue, then shelve a book and give it a voice — either by narrating its chapters or by importing audio you already have."
      />

      {/* The search is the page's centrepiece, set like a title, not a filter bar. */}
      <section aria-labelledby="discover-search-label" className="border-b border-ink py-6">
        <label id="discover-search-label" htmlFor="store-search" className="eyebrow">
          Search the catalogue
        </label>
        <div className="relative mt-2">
          <Search
            className="pointer-events-none absolute top-1/2 left-0 size-5 -translate-y-1/2 text-ink-subtle"
            aria-hidden
          />
          <Input
            id="store-search"
            ref={inputRef}
            type="search"
            value={term}
            onChange={(event) => onTermChange(event.target.value)}
            onKeyDown={onSearchKeyDown}
            placeholder="Title, author or ISBN"
            autoComplete="off"
            spellCheck={false}
            enterKeyHint="search"
            // `::-webkit-search-cancel-button` would clear the field without
            // clearing the URL, leaving results for a query the box no longer shows.
            className="h-14 rounded-none border-0 border-b border-hairline bg-transparent pr-10 pl-9 font-display text-[clamp(1.35rem,3.4vw,2rem)] tracking-[-0.02em] placeholder:text-ink-subtle focus:border-ink focus:bg-transparent focus-visible:outline-offset-4 [&::-webkit-search-cancel-button]:hidden"
          />
          {term && (
            <button
              type="button"
              onClick={clearSearch}
              aria-label="Clear search"
              className="absolute top-1/2 right-0 grid size-8 -translate-y-1/2 place-items-center rounded-[2px] text-ink-subtle transition-colors hover:bg-overlay hover:text-ink focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-accent"
            >
              <X className="size-4" aria-hidden />
            </button>
          )}
        </div>

        <div
          role="group"
          aria-label="Filter by category"
          className="no-scrollbar -mx-5 mt-5 flex gap-x-4 gap-y-1 overflow-x-auto px-5 sm:mx-0 sm:flex-wrap sm:px-0"
        >
          {GENRE_FILTERS.map((filter) => {
            const active = genre === filter.slug;
            return (
              <button
                key={filter.slug}
                type="button"
                onClick={() => push({ genre: filter.slug, page: undefined })}
                aria-pressed={active}
                className={cn(
                  'inline-flex min-h-11 shrink-0 items-center border-b-2 px-2.5 pb-1.5 pt-1 font-mono text-[10px] uppercase tracking-[0.08em] transition-colors duration-150',
                  active
                    ? 'border-accent text-accent'
                    : 'border-transparent text-ink-subtle hover:border-hairline-strong hover:text-ink',
                )}
              >
                {filter.label}
              </button>
            );
          })}
        </div>
      </section>

      {/* Results toolbar */}
      <div className="mt-7 mb-6 flex flex-wrap items-end justify-between gap-3 border-b border-hairline pb-2">
        <p className="font-mono text-[10px] uppercase tracking-[0.09em] text-ink-subtle" aria-live="polite">
          {loading && !data
            ? 'Searching the catalogue…'
            : data
              ? `${data.total.toLocaleString()} ${data.total === 1 ? 'volume' : 'volumes'}`
              : ''}
        </p>

        <div className="flex items-center gap-2">
          <span className="eyebrow hidden sm:block">Sort</span>
          <Select value={sort} onValueChange={(value) => push({ sort: value, page: undefined })}>
            <SelectTrigger id="store-sort" className="w-40">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {STORE_SORTS.map((option) => (
                <SelectItem key={option.value} value={option.value}>
                  {option.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      </div>

      {data?.degraded && (
        <p className="mb-6 border-l-2 border-caution pl-3 text-[12px] leading-relaxed text-ink-muted">
          The book metadata service is unreachable, so these are local catalogue results. Search
          will be complete once it recovers.
        </p>
      )}

      {error && (
        <EmptyState
          title="Search failed"
          description={error}
          action={<Button onClick={reload}>Try again</Button>}
        />
      )}

      {showSkeleton ? (
        <BookGridSkeleton count={12} />
      ) : data && data.books.length === 0 ? (
        hasQuery ? (
          <NoResults
            query={query || GENRE_FILTERS.find((f) => f.slug === genre)?.label || genre}
            onClear={() => {
              setTerm('');
              router.push('/store');
            }}
          />
        ) : (
          <EmptyState
            icon={Sparkles}
            title="The catalogue is at your fingertips"
            description="Type a title, author or ISBN in the field above, or choose a category to browse."
          />
        )
      ) : data ? (
        <>
          {/* Dim results while a newer search is in flight, rather than blanking them. */}
          <div className={cn('transition-opacity duration-200', loading && 'opacity-55')}>
            <BookGrid books={data.books.map((book) => ({ book }))} />
          </div>

          <Pagination
            page={page}
            hasMore={data.hasMore}
            onPageChange={(next) => {
              push({ page: next });
              window.scrollTo({ top: 0, behavior: 'smooth' });
            }}
            total={data.total}
            pageSize={data.pageSize}
          />
        </>
      ) : null}
    </div>
  );
}
