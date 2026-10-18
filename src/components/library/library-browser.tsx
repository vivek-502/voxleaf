import * as React from 'react';
import { useNavigate, useSearchParams } from 'react-router-dom';
import { Library, SlidersHorizontal } from 'lucide-react';

import { BookGrid } from '@/components/book/book-card';
import { Pagination } from '@/components/common/pagination';
import { PageHeader } from '@/components/common/section-header';
import { BookGridSkeleton, EmptyState, ErrorState } from '@/components/common/states';
import { Button } from '@/components/ui/button';
import { Label } from '@/components/ui/inputs';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { useApiQuery } from '@/hooks/use-async';
import { usePlayer } from '@/components/providers/player-provider';
import { LIBRARY_FILTERS, LIBRARY_SORTS, PAGE_SIZE, type LibraryFilter, type LibrarySort } from '@/utils/constants';
import { pluralize } from '@/utils/format';
import type { LibraryBookInfo } from '@/types';

interface LibraryPayload {
  books: LibraryBookInfo[];
  total: number;
  page: number;
  pageSize: number;
  hasMore: boolean;
  stats: {
    bookCount: number;
    chaptersWithAudio: number;
    chapterCount: number;
  };
}

/**
 * The user's own shelf.
 *
 * Filters map to the state that actually matters in a hand-managed library:
 * what has audio, what is missing audio, and what is finished.
 */
export function LibraryBrowser() {
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const { playChapter } = usePlayer();

  const sort = (searchParams.get('sort') ?? 'recently-added') as LibrarySort;
  const filter = (searchParams.get('filter') ?? 'all') as LibraryFilter;
  const page = Math.max(1, Number(searchParams.get('page') ?? '1') || 1);

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
      navigate(search ? `/library?${search}` : '/library');
    },
    [navigate, searchParams],
  );

  const { data, error, loading, reload } = useApiQuery<LibraryPayload>('/api/library', {
    sort,
    filter: filter === 'all' ? undefined : filter,
    page,
    pageSize: PAGE_SIZE,
  });

  const resume = (entry: LibraryBookInfo) => {
    if (entry.lastPlayedChapterId) {
      void playChapter(entry.lastPlayedChapterId, entry.bookId);
    } else {
      void playChapter(entry.bookId);
    }
  };

  return (
    <div className="container-page py-8 sm:py-10">
      <PageHeader
        title="My library"
        description={
          loading && !data
            ? 'Loading your books…'
            : data
              ? `${pluralize(data.stats.bookCount, 'book')} · ${data.stats.chaptersWithAudio} of ${data.stats.chapterCount} chapters have audio`
              : undefined
        }
      />

      {/* Filters */}
      <div className="mb-6 flex flex-wrap items-center justify-between gap-3">
        <div
          role="group"
          aria-label="Filter library"
          className="no-scrollbar -mx-5 flex gap-2 overflow-x-auto px-5 pb-1 sm:mx-0 sm:flex-wrap sm:px-0"
        >
          {LIBRARY_FILTERS.map((option) => {
            const active = filter === option.value;
            return (
              <button
                key={option.value}
                type="button"
                onClick={() => push({ filter: option.value, page: undefined })}
                aria-pressed={active}
                className={[
                  'shrink-0 rounded-full border px-4 py-2 text-sm font-medium transition-colors duration-150',
                  active
                    ? 'border-accent-line bg-accent-wash text-accent-soft'
                    : 'border-hairline text-ink-muted hover:border-hairline-strong hover:text-ink',
                ].join(' ')}
              >
                {option.label}
              </button>
            );
          })}
        </div>

        <div className="flex items-center gap-2">
          <SlidersHorizontal className="size-4 shrink-0 text-ink-subtle" aria-hidden />
          <Label htmlFor="library-sort" className="sr-only-focusable">
            Sort library
          </Label>
          <Select value={sort} onValueChange={(value) => push({ sort: value, page: undefined })}>
            <SelectTrigger id="library-sort" className="w-48">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {LIBRARY_SORTS.map((option) => (
                <SelectItem key={option.value} value={option.value}>
                  {option.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      </div>

      {error && (
        <ErrorState title="Could not load your library" message={error} onRetry={reload} />
      )}

      {loading && !data ? (
        <BookGridSkeleton count={12} />
      ) : data && data.books.length === 0 ? (
        <EmptyState
          icon={Library}
          title={
            data.stats.bookCount === 0
              ? 'Your library is empty'
              : filter === 'all'
                ? 'Nothing to show'
                : `No books match “${LIBRARY_FILTERS.find((f) => f.value === filter)?.label}”`
          }
          description={
            data.stats.bookCount === 0
              ? 'Search the store and add the books you want to listen to.'
              : 'Try a different filter.'
          }
          action={
            data.stats.bookCount === 0 ? (
              <Button asChild>
                <a href="/store">Browse the store</a>
              </Button>
            ) : (
              <Button variant="outline" onClick={() => push({ filter: 'all' })}>
                Show all books
              </Button>
            )
          }
        />
      ) : data ? (
        <>
          <div className={loading ? 'opacity-55 transition-opacity duration-200' : 'transition-opacity duration-200'}>
            <BookGrid
              books={data.books.map((entry) => ({
                book: entry.book,
                library: entry,
                // Only offer playback when there is actually audio to play.
                onPlay: entry.chaptersWithAudio > 0 ? () => resume(entry) : undefined,
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
