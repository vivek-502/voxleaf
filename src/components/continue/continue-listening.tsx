import * as React from 'react';
import { Link } from 'react-router-dom';
import { Clock, Play } from 'lucide-react';

import { BookCover } from '@/components/book/book-cover';
import { PageHeader } from '@/components/common/section-header';
import { BookRowSkeleton, EmptyState, ErrorState } from '@/components/common/states';
import { Button } from '@/components/ui/button';
import { useApiQuery } from '@/hooks/use-async';
import { usePlayer } from '@/components/providers/player-provider';
import { formatDuration, formatPercent, formatRelativeTime } from '@/utils/format';
import type { LibraryBookInfo } from '@/types';

/** `/api/library` is paginated; this page shows the whole (single-user) library. */
interface LibraryPayload {
  books: LibraryBookInfo[];
  total: number;
}

/**
 * "Continue" — every book with listening state, most recently played first.
 *
 * This is the page a returning listener wants: one tap resumes the exact
 * position, with the position shown so the choice is informed.
 */
export function ContinueListening() {
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
    } else {
      void playChapter(entry.bookId);
    }
  };

  return (
    <div className="container-page py-8 sm:py-10">
      <PageHeader
        title="Continue listening"
        description="Every book you have started, most recently played first."
      />

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
              <Link to="/store">Find something to listen to</Link>
            </Button>
          }
        />
      ) : (
        <ul className="grid grid-cols-2 gap-x-4 gap-y-7 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6">
          {books.map((entry) => {
            const summary = entry.book;
            return (
              <li key={entry.libraryId} className="flex">
                <article className="group flex w-full flex-col gap-2.5">
                  <Link
                    to={`/book/${entry.bookId}`}
                    className="relative block overflow-hidden rounded-lg focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent-soft"
                  >
                    <BookCover
                      src={summary.coverUrl}
                      title={summary.title}
                      size="md"
                      className="aspect-[2/3] w-full"
                    />
                    {/* Resume affordance sits on the artwork, matching the
                        store/library cards. */}
                    <span className="absolute inset-x-0 bottom-0 flex items-end justify-between gap-2 bg-gradient-to-t from-black/75 to-transparent p-2.5">
                      <span className="truncate text-[11px] leading-tight font-medium text-white/90">
                        {entry.lastChapterNumber
                          ? `Ch. ${entry.lastChapterNumber}`
                          : entry.lastChapterTitle || 'Not started'}
                      </span>
                      <span className="shrink-0 text-[10px] tabular-nums text-white/70">
                        {formatPercent(entry.overallProgress)}%
                      </span>
                    </span>
                  </Link>

                  <h2 className="truncate text-[13px] leading-tight font-medium text-ink">
                    <Link to={`/book/${entry.bookId}`} className="hover:text-accent-soft">
                      {summary.title}
                    </Link>
                  </h2>
                  <p className="-mt-1.5 truncate text-xs text-ink-subtle">{summary.author}</p>

                  <p className="text-[11px] text-ink-subtle">
                    {entry.lastPlayedAt
                      ? `${formatRelativeTime(entry.lastPlayedAt)} · ${formatDuration(entry.lastPosition)} in`
                      : 'Not started yet'}
                  </p>

                  <div className="mt-auto flex items-center gap-1.5 pt-1">
                    <Button
                      size="sm"
                      onClick={() => resume(entry)}
                      icon={<Play className="size-3.5 fill-current" aria-hidden />}
                    >
                      {entry.overallProgress > 0 ? 'Resume' : 'Start'}
                    </Button>
                    <Button asChild size="sm" variant="outline">
                      <Link to={`/book/${entry.bookId}`}>Chapters</Link>
                    </Button>
                  </div>
                </article>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}

