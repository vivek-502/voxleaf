'use client';

import * as React from 'react';
import Link from 'next/link';
import { useRouter } from 'next/navigation';
import { Clock, Play } from 'lucide-react';

import { BookCover } from '@/components/book/book-cover';
import { ProgressBar } from '@/components/book/book-card';
import { PageHeader } from '@/components/common/section-header';
import { BookRowSkeleton, EmptyState, ErrorState } from '@/components/common/states';
import { Button } from '@/components/ui/button';
import { useApiQuery } from '@/hooks/use-async';
import { usePlayer } from '@/components/providers/player-provider';
import {
  formatDuration,
  formatPercent,
  formatRelativeTime,
  pluralize,
} from '@/utils/format';
import type { LibraryBookInfo } from '@/types';

/** `/api/library` is paginated; this page shows the whole (single-user) library. */
interface LibraryPayload {
  books: LibraryBookInfo[];
  total: number;
}

/**
 * Continue: every book with listening state, most recently played first.
 *
 * Rendered as a numbered ledger rather than a card grid — this page is about
 * deciding which thread to pick up, so position and recency matter more than
 * artwork.
 */
export function ContinueListening() {
  const router = useRouter();
  const { playChapter } = usePlayer();
  const { data, error, loading, reload } = useApiQuery<LibraryPayload>('/api/library', {
    sort: 'recently-listened',
    filter: 'in-progress',
    pageSize: 100,
  });
  const books = data?.books ?? [];

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
        eyebrow="In progress"
        title="Continue"
        description="Every book you have started, most recently played first. Resume from the exact second you stopped."
      />

      <div className="mt-7 mb-4 flex items-center justify-between border-b border-hairline pb-2">
        <p className="eyebrow">
          {loading && !data ? 'Reading the ledger…' : pluralize(books.length, 'title')}
        </p>
        <Link
          href="/library"
          className="font-mono text-[10px] uppercase tracking-[0.09em] text-accent hover:text-accent-deep"
        >
          The whole library
        </Link>
      </div>

      {error && <ErrorState title="Could not load your progress" message={error} onRetry={reload} />}

      {loading && !data ? (
        <BookRowSkeleton count={6} />
      ) : books.length === 0 ? (
        <EmptyState
          icon={Clock}
          title="Nothing in progress yet"
          description="Play a chapter and it will show up here, ready to resume from exactly where you stopped."
          action={
            <Button asChild>
              <Link href="/store">Find something to listen to</Link>
            </Button>
          }
        />
      ) : (
        <ol className="border-t border-ink">
          {books.map((entry, index) => {
            const summary = entry.book;
            return (
              <li key={entry.libraryId} className="border-b border-hairline">
                <div className="flex items-center gap-3 py-4 sm:gap-5">
                  <span className="hidden w-6 shrink-0 font-mono text-[10px] tabular-nums text-ink-subtle sm:block">
                    {String(index + 1).padStart(2, '0')}
                  </span>

                  <Link
                    href={`/book/${entry.bookId}`}
                    className="shrink-0 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent"
                    tabIndex={-1}
                    aria-hidden
                  >
                    <BookCover src={summary.coverUrl} title={summary.title} size="sm" />
                  </Link>

                  <div className="min-w-0 flex-1">
                    <Link
                      href={`/book/${entry.bookId}`}
                      className="block truncate text-[14px] font-medium text-ink hover:text-accent"
                    >
                      {summary.title}
                    </Link>
                    <p className="mt-0.5 truncate text-[11.5px] text-ink-subtle">
                      {summary.author}
                      {entry.lastChapterTitle && ` · ${entry.lastChapterTitle}`}
                    </p>
                    <p className="mt-1 text-[11px] text-ink-subtle sm:hidden">
                      {formatPercent(entry.overallProgress)} ·{' '}
                      {entry.lastPlayedAt ? formatRelativeTime(entry.lastPlayedAt) : 'Not started'}
                    </p>
                  </div>

                  <div className="hidden w-36 shrink-0 md:block lg:w-48">
                    <ProgressBar
                      value={entry.overallProgress}
                      label={`${summary.title} progress`}
                    />
                    <p className="mt-1.5 font-mono text-[9.5px] uppercase tracking-[0.06em] text-ink-subtle">
                      {formatPercent(entry.overallProgress)} ·{' '}
                      {entry.lastPosition > 0 ? `${formatDuration(entry.lastPosition)} in` : 'unstarted'}
                    </p>
                  </div>

                  <p className="hidden w-28 shrink-0 text-right text-[11px] text-ink-subtle lg:block">
                    {entry.lastPlayedAt ? formatRelativeTime(entry.lastPlayedAt) : 'Not started'}
                  </p>

                  <Button
                    size="sm"
                    onClick={() => resume(entry)}
                    icon={<Play className="size-3.5 fill-current" aria-hidden />}
                    className="shrink-0"
                  >
                    {entry.overallProgress > 0 ? 'Resume' : 'Start'}
                  </Button>
                </div>
              </li>
            );
          })}
        </ol>
      )}
    </div>
  );
}
