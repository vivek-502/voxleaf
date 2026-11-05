'use client';

import * as React from 'react';
import Link from 'next/link';
import { useRouter, useSearchParams } from 'next/navigation';
import { Library } from 'lucide-react';

import { BookGrid } from '@/components/book/book-card';
import { Pagination } from '@/components/common/pagination';
import { PageHeader } from '@/components/common/section-header';
import { BookGridSkeleton, EmptyState, ErrorState } from '@/components/common/states';
import { Button } from '@/components/ui/button';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { useApiQuery } from '@/hooks/use-async';
import { usePlayer } from '@/components/providers/player-provider';
import {
  LIBRARY_FILTERS,
  LIBRARY_SORTS,
  PAGE_SIZE,
  type LibraryFilter,
  type LibrarySort,
} from '@/utils/constants';
import { pluralize } from '@/utils/format';
import { cn } from '@/utils/cn';
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
  const router = useRouter();
  const searchParams = useSearchParams();
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
      router.push(search ? `/library?${search}` : '/library');
    },
    [router, searchParams],
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
      return;
    }
    router.push(`/book/${entry.bookId}`);
  };

  return (
    <div className="container-page pb-16">
      <PageHeader
        className="mt-8 sm:mt-10"
        eyebrow="Your shelves"
        title="The library"
        description={
          loading && !data
            ? 'Loading your books…'
            : data
              ? `${pluralize(data.stats.bookCount, 'title')} shelved · ${data.stats.chaptersWithAudio} of ${data.stats.chapterCount} chapters narrated`
              : undefined
        }
        actions={
          <Button asChild variant="outline">
            <Link href="/store">Discover a book</Link>
          </Button>
        }
      />

      <div className="mt-6 mb-7 flex flex-wrap items-end justify-between gap-3 border-b border-hairline pb-1">
        <div
          role="group"
          aria-label="Filter library"
          className="no-scrollbar -mx-5 flex gap-x-4 overflow-x-auto px-5 sm:mx-0 sm:flex-wrap sm:px-0"
        >
          {LIBRARY_FILTERS.map((option) => {
            const active = filter === option.value;
            return (
              <button
                key={option.value}
                type="button"
                onClick={() => push({ filter: option.value, page: undefined })}
                aria-pressed={active}
                className={cn(
                  'inline-flex min-h-11 shrink-0 items-center border-b-2 px-2.5 pb-2 pt-1 font-mono text-[10px] uppercase tracking-[0.08em] transition-colors duration-150',
                  active
                    ? 'border-accent text-accent'
                    : 'border-transparent text-ink-subtle hover:border-hairline-strong hover:text-ink',
                )}
              >
                {option.label}
              </button>
            );
          })}
        </div>

        <div className="mb-1.5 flex items-center gap-2">
          <span className="eyebrow hidden sm:block">Sort</span>
          <Select value={sort} onValueChange={(value) => push({ sort: value, page: undefined })}>
            <SelectTrigger id="library-sort" className="w-44">
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
              ? 'Your shelves are empty'
              : filter === 'all'
                ? 'Nothing to show'
                : `No books match “${LIBRARY_FILTERS.find((f) => f.value === filter)?.label}”`
          }
          description={
            data.stats.bookCount === 0
              ? 'Search the catalogue and shelve the books you want to listen to.'
              : 'Try a different filter.'
          }
          action={
            data.stats.bookCount === 0 ? (
              <Button asChild>
                <Link href="/store">Browse the catalogue</Link>
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
          <div
            className={cn(
              'transition-opacity duration-200',
              loading && 'opacity-55',
            )}
          >
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
