import * as React from 'react';
import { useNavigate, useSearchParams } from 'react-router-dom';
import { Search, SlidersHorizontal, Sparkles, X } from 'lucide-react';

import { BookGrid } from '@/components/book/book-card';
import { Pagination } from '@/components/common/pagination';
import { PageHeader } from '@/components/common/section-header';
import { BookGridSkeleton, EmptyState, NoResults } from '@/components/common/states';
import { Button } from '@/components/ui/button';
import { Input, Label } from '@/components/ui/inputs';
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
 * Store: search millions of books and add them to the library.
 *
 * The query lives in the URL, so results are linkable, survive a refresh and
 * work with browser back/forward. Typing updates the URL on a debounce and
 * keeps the previous page's results visible while the next request runs, which
 * avoids a flash of empty state on every keystroke.
 */
export function StoreBrowser() {
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();

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
      navigate(search ? `/store?${search}` : '/store');
    },
    [navigate, searchParams],
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

  React.useEffect(() => () => {
    if (debounceRef.current) clearTimeout(debounceRef.current);
  }, []);

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
    <div className="container-page py-8 sm:py-10">
      <PageHeader
        title="Store"
        description="Search books, then add them to your library and import chapter audio."
      />

      {/* Search. Typing updates the URL on a debounce; results stay put while it runs. */}
      <div className="relative mb-6">
        <Label htmlFor="store-search" className="sr-only-focusable">
          Search books by title, author or ISBN
        </Label>
        <Search
          className="pointer-events-none absolute top-1/2 left-4 size-4 -translate-y-1/2 text-ink-subtle"
          aria-hidden
        />
        <Input
          id="store-search"
          ref={inputRef}
          type="search"
          value={term}
          onChange={(event) => onTermChange(event.target.value)}
          onKeyDown={onSearchKeyDown}
          placeholder="Search by title, author or ISBN…"
          autoComplete="off"
          spellCheck={false}
          enterKeyHint="search"
          // `::-webkit-search-cancel-button` would clear the field without
          // clearing the URL, leaving results for a query the box no longer shows.
          className="pr-11 pl-11 [&::-webkit-search-cancel-button]:hidden"
        />
        {term && (
          <button
            type="button"
            onClick={clearSearch}
            aria-label="Clear search"
            className="absolute top-1/2 right-3 grid size-7 -translate-y-1/2 place-items-center rounded-full text-ink-subtle transition-colors hover:bg-overlay hover:text-ink focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-accent-soft"
          >
            <X className="size-4" aria-hidden />
          </button>
        )}
      </div>

      {/* Genre chips */}
      <div
        role="group"
        aria-label="Filter by category"
        className="no-scrollbar -mx-5 mb-6 flex gap-2 overflow-x-auto px-5 pb-1 sm:mx-0 sm:flex-wrap sm:px-0"
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
                'shrink-0 rounded-full border px-4 py-2 text-sm font-medium transition-colors duration-150',
                active
                  ? 'border-accent-line bg-accent-wash text-accent-soft'
                  : 'border-hairline text-ink-muted hover:border-hairline-strong hover:text-ink',
              )}
            >
              {filter.label}
            </button>
          );
        })}
      </div>

      {/* Sort */}
      <div className="mb-6 flex items-center justify-between gap-4">
        <p className="text-sm text-ink-muted" aria-live="polite">
          {loading && !data
            ? 'Searching…'
            : data
              ? `${data.total.toLocaleString()} ${data.total === 1 ? 'result' : 'results'}`
              : ''}
        </p>

        <div className="flex items-center gap-2">
          <SlidersHorizontal className="size-4 shrink-0 text-ink-subtle" aria-hidden />
          <Label htmlFor="store-sort" className="sr-only-focusable">
            Sort results
          </Label>
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
        <p className="mb-6 rounded-xl border border-caution/25 bg-caution/6 px-4 py-3 text-sm text-ink-muted">
          The book metadata service is unreachable, so these are local catalogue results. Search will
          be complete once it recovers.
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
            query={query}
            onClear={() => {
              setTerm('');
              navigate('/store');
            }}
          />
        ) : (
          <EmptyState
            icon={Sparkles}
            title="Start with a search"
            description="Type a title, author or ISBN above, or pick a category to browse."
          />
        )
      ) : data ? (
        <>
          {/* Dim results while a newer search is in flight, rather than blanking them. */}
          <div className={cn('transition-opacity duration-200', loading && 'opacity-55')}>
            <BookGrid
              books={data.books.map((book) => ({
                book,
                onPlay: undefined,
              }))}
            />
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
