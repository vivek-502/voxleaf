import * as React from 'react';
import { Link } from 'react-router-dom';
import {
  BookPlus,
  Headphones,
  Library,
  LibraryBig,
  Play,
  Search,
} from 'lucide-react';

import { BookCover } from '@/components/book/book-cover';
import { BookRow, ProgressBar } from '@/components/book/book-card';
import { SectionHeader } from '@/components/common/section-header';
import { BookRowSkeleton, EmptyState, ErrorState } from '@/components/common/states';
import { Button } from '@/components/ui/button';
import { Eyebrow, Skeleton } from '@/components/ui/primitives';
import { useApiQuery } from '@/hooks/use-async';
import { api, errorMessage } from '@/hooks/use-api';
import { toast } from 'sonner';
import { usePlayer } from '@/components/providers/player-provider';
import { formatDuration, formatPercent, pluralize } from '@/utils/format';
import { cn } from '@/utils/cn';
import type { BookSummary, LibraryBookInfo } from '@/types';

interface HomeStats {
  bookCount: number;
  chapterCount: number;
  chaptersWithAudio: number;
  totalAudioSeconds: number;
  bookmarkCount: number;
  favoriteCount: number;
  completedCount: number;
  inProgressCount: number;
}

interface HomePayload {
  continueListening: LibraryBookInfo[];
  recentlyAdded: LibraryBookInfo[];
  library: LibraryBookInfo[];
  favorites: LibraryBookInfo[];
  stats: HomeStats;
  recommended: BookSummary[];
  recommendedDegraded: boolean;
}

/**
 * The dashboard.
 *
 * One aggregated `/api/home` request drives the whole page, so there is no
 * request waterfall on the most-used screen. Sections with nothing to show are
 * omitted rather than rendered empty.
 */
export function HomeDashboard() {
  const { playChapter } = usePlayer();
  const { data, error, loading, reload } = useApiQuery<HomePayload>('/api/home');

  const resume = (entry: LibraryBookInfo) => {
    // Prefer the exact position the listener stopped at; fall back to the
    // first chapter that actually has audio.
    if (entry.lastPlayedChapterId) {
      void playChapter(entry.lastPlayedChapterId, entry.bookId);
      return;
    }
    void playChapter(entry.bookId);
  };

  const hero = data?.continueListening[0] ?? data?.library.find((entry) => entry.overallProgress > 0);

  if (error) {
    return (
      <div className="container-page py-10">
        <ErrorState
          title="Could not load your dashboard"
          message={error}
          onRetry={reload}
          className="mt-6"
        />
      </div>
    );
  }

  return (
    <div className="container-page py-8 sm:py-10">
      <header className="mb-8">
        <Eyebrow>Welcome back</Eyebrow>
        <h1 className="mt-1.5 text-2xl font-semibold text-ink sm:text-3xl">Your listening</h1>
        <p className="mt-1.5 text-sm text-ink-muted">
          {loading
            ? 'Loading your library…'
            : data
              ? `${pluralize(data.stats.bookCount, 'book')} · ${formatDuration(
                  data.stats.totalAudioSeconds,
                )} of audio · ${data.stats.bookmarkCount} bookmarks`
              : ''}
        </p>
      </header>

      {loading ? (
        <div className="space-y-10">
          <div className="skeleton h-44 rounded-panel" />
          <section>
            <Skeleton className="h-6 w-40" />
            <div className="mt-4">
              <BookRowSkeleton count={6} />
            </div>
          </section>
        </div>
      ) : !data ? null : data.stats.bookCount === 0 ? (
        <FirstRun />
      ) : (
        <div className="flex flex-col gap-11">
          {hero && <HeroCard book={hero} onPlay={() => resume(hero)} />}

          {data.continueListening.length > 1 && (
            <section>
              <SectionHeader
                title="Continue listening"
                href="/continue"
                description="Pick up exactly where you left off."
              />
              <BookRow
                books={data.continueListening.slice(1, 13).map((entry) => ({
                  book: entry,
                  onPlay: () => resume(entry),
                }))}
              />
            </section>
          )}

          {data.recentlyAdded.length > 0 && (
            <section>
              <SectionHeader title="Recently added" href="/library" />
              <BookRow
                books={data.recentlyAdded.slice(0, 13).map((entry) => ({
                  book: entry,
                  onPlay: () => resume(entry),
                }))}
              />
            </section>
          )}

          {data.favorites.length > 0 && (
            <section>
              <SectionHeader title="Your favorites" href="/favorites" />
              <BookRow
                books={data.favorites.slice(0, 13).map((entry) => ({
                  book: entry,
                  onPlay: () => resume(entry),
                }))}
              />
            </section>
          )}

          <section>
            <SectionHeader
              title="Discover something new"
              href="/store"
              description="Search millions of books, then add the ones you want."
            />
            {data.recommended.length > 0 ? (
              <BookGridShelf books={data.recommended} degraded={data.recommendedDegraded} onChanged={reload} />
            ) : (
              <EmptyState
                icon={Search}
                title="No recommendations right now"
                description="Browse the store and add a few books — recommendations are based on what you already own."
                action={
                  <Button asChild variant="outline">
                    <Link to="/store">Browse the store</Link>
                  </Button>
                }
              />
            )}
          </section>

          {data.stats.bookmarkCount > 0 && (
            <section>
              <SectionHeader
                title="Saved moments"
                href="/bookmarks"
                description={`${pluralize(data.stats.bookmarkCount, 'bookmark', 'bookmarks')} across your library`}
              />
            </section>
          )}
        </div>
      )}
    </div>
  );
}

/** First-run guidance. There is no sign-up step, so this has to explain the flow. */
function FirstRun() {
  return (
    <EmptyState
      icon={Library}
      title="Your library is empty"
      description="VoxLeaf has no accounts and no sign-in — search for any book, add it to your library, then import audio for its chapters."
      action={
        <Button asChild size="lg" icon={<Search className="size-4" aria-hidden />}>
          <Link to="/store">Find a book</Link>
        </Button>
      }
      secondaryAction={
        <Button asChild variant="outline" icon={<LibraryBig className="size-4" aria-hidden />}>
          <Link to="/library">My library</Link>
        </Button>
      }
      className="mt-4"
    />
  );
}

/** Prominent "keep listening" card for the single most relevant book. */
function HeroCard({ book, onPlay }: { book: LibraryBookInfo; onPlay: () => void }) {
  const summary = book.book;
  const started = book.overallProgress > 0;

  return (
    <section
      aria-labelledby="hero-heading"
      className="card-surface relative overflow-hidden p-5 sm:p-7"
    >
      <div
        aria-hidden
        className="pointer-events-none absolute inset-0 bg-gradient-to-br from-accent-wash via-transparent to-transparent"
      />
      <div className="relative flex flex-col gap-5 sm:flex-row sm:items-center sm:gap-7">
        <div className="flex min-w-0 flex-1 flex-col items-start gap-3 sm:order-2">
          <Eyebrow className="flex items-center gap-1.5">
            <Headphones className="size-3.5" aria-hidden />
            {book.completed ? 'Finished — listen again' : started ? 'Continue listening' : 'Ready to start'}
          </Eyebrow>

          <h2 id="hero-heading" className="text-xl font-semibold text-ink sm:text-2xl">
            <Link to={`/book/${book.bookId}`} className="hover:text-accent-soft">
              {summary.title}
            </Link>
          </h2>

          <p className="text-sm text-ink-muted">{summary.author}</p>

          {book.lastChapterTitle && (
            <p className="text-sm text-ink-muted">
              {book.lastPlayedChapterId && (
                <>
                  Ch. {book.lastChapterNumber} · {book.lastChapterTitle}
                </>
              )}
              {book.lastPosition > 0 && (
                <>
                  {' '}
                  · {formatDuration(book.lastPosition)} in
                </>
              )}
            </p>
          )}

          {started && (
            <div className="w-full max-w-sm">
              <ProgressBar
                value={book.overallProgress}
                label={`${summary.title} progress`}
                showValue
              />
              <p className="mt-1.5 text-xs text-ink-subtle">
                {formatPercent(book.overallProgress)}% ·{' '}
                {book.chaptersWithAudio} of {book.chapterCount}{' '}
                {pluralize(book.chapterCount, 'chapter')} with audio
              </p>
            </div>
          )}

          <div className="mt-1 flex flex-wrap gap-2">
            <Button onClick={onPlay} icon={<Play className="size-4 fill-current" aria-hidden />}>
              {book.completed ? 'Listen again' : started ? 'Resume' : 'Play'}
            </Button>
            <Button asChild variant="outline">
              <Link to={`/book/${book.bookId}`}>View chapters</Link>
            </Button>
          </div>
        </div>
      </div>
    </section>
  );
}

/** Compact recommended-books strip, rendered as rows to keep the page short. */
function BookGridShelf({
  books,
  degraded,
  onChanged,
}: {
  books: BookSummary[];
  degraded: boolean;
  onChanged: () => void;
}) {
  return (
    <>
      {degraded && (
        <p className="mb-4 rounded-xl border border-caution/25 bg-caution/6 px-4 py-2.5 text-xs text-ink-muted">
          The book service is unreachable, so these are local catalogue results. Search will work
          again once it recovers.
        </p>
      )}
      <RecommendationShelf books={books} onChanged={onChanged} />
    </>
  );
}

/** Horizontal, scroll-snapping shelf of recommended books with inline add. */
function RecommendationShelf({
  books,
  onChanged,
}: {
  books: BookSummary[];
  onChanged: () => void;
}) {
  const [pendingId, setPendingId] = React.useState<string | null>(null);

  return (
    <ul className="no-scrollbar -mx-5 flex gap-4 overflow-x-auto px-5 pb-2 sm:mx-0 sm:px-0">
      {books.map((book) => (
        <li key={book.id} className="w-40 shrink-0 sm:w-44">
          <article className="group flex flex-col">
            <Link
              to={`/book/${book.id}`}
              className="block overflow-hidden rounded-lg focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent-soft"
            >
              <CoverLazy book={book} />
            </Link>
            <h3 className="mt-2.5 truncate text-[13px] leading-tight font-medium text-ink">
              <Link to={`/book/${book.id}`} className="hover:text-accent-soft">
                {book.title}
              </Link>
            </h3>
            <p className="mt-0.5 truncate text-xs text-ink-subtle">{book.author}</p>
            {book.inLibrary ? (
              <Link
                to={`/book/${book.libraryId}`}
                className={cn(
                  'mt-1.5 inline-flex w-fit items-center gap-1 text-[11px] font-medium text-accent-soft',
                  'hover:text-accent',
                )}
              >
                In your library
              </Link>
            ) : (
              <Button
                variant="ghost"
                size="sm"
                className="mt-1 w-fit px-0 text-[11px] text-ink-muted hover:bg-transparent hover:text-accent-soft"
                loading={pendingId === book.id}
                icon={<BookPlus className="size-3.5" aria-hidden />}
                onClick={async () => {
                  setPendingId(book.id);
                  try {
                    await api.post('/api/library', { bookId: book.id });
                    toast.success(`Added “${book.title}” to your library.`);
                    // Re-fetch so the card flips to "In your library" and the
                    // library counts in the header update, instead of leaving a
                    // stale grid the user could click again.
                    onChanged();
                  } catch (error) {
                    toast.error(errorMessage(error, 'Could not add this book.'));
                  } finally {
                    setPendingId(null);
                  }
                }}
              >
                Add
              </Button>
            )}
          </article>
        </li>
      ))}
    </ul>
  );
}

function CoverLazy({ book }: { book: BookSummary }) {
  return <BookCover src={book.coverUrl} title={book.title} size="md" className="w-full" />;
}
