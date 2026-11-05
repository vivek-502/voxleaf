'use client';

import * as React from 'react';
import Link from 'next/link';
import { useRouter } from 'next/navigation';
import { BookPlus, Headphones, Play, Search } from 'lucide-react';

import { BookCover } from '@/components/book/book-cover';
import { BookRow, ProgressBar } from '@/components/book/book-card';
import { SectionHeader } from '@/components/common/section-header';
import { ErrorState } from '@/components/common/states';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/primitives';
import { useApiQuery } from '@/hooks/use-async';
import { api, errorMessage } from '@/hooks/use-api';
import { toast } from 'sonner';
import { usePlayer } from '@/components/providers/player-provider';
import {
  formatDuration,
  formatDurationShort,
  formatPercent,
  formatRelativeTime,
  pluralize,
} from '@/utils/format';
import { cn } from '@/utils/cn';
import type { BookmarkInfo, BookSummary, LibraryBookInfo } from '@/types';

interface HomeStats {
  bookCount: number;
  chapterCount: number;
  chaptersWithAudio: number;
  totalAudioDuration: number;
  bookmarkCount: number;
  completedCount: number;
}

interface HomePayload {
  continueListening: LibraryBookInfo[];
  recentlyAdded: LibraryBookInfo[];
  library: LibraryBookInfo[];
  favorites: LibraryBookInfo[];
  stats: HomeStats;
  bookmarks: BookmarkInfo[];
  recommended: BookSummary[];
  recommendedDegraded: boolean;
}

/**
 * The front page.
 *
 * One aggregated `/api/home` request drives the whole page. The layout is
 * editorial rather than dashboard-like: a title-page hero for the book in
 * hand, a ledger of the collection, then ruled sections.
 */
export function HomeDashboard() {
  const router = useRouter();
  const { playChapter } = usePlayer();
  const { data, error, loading, reload } = useApiQuery<HomePayload>('/api/home');

  const resume = (entry: LibraryBookInfo) => {
    if (entry.lastPlayedChapterId) {
      void playChapter(entry.lastPlayedChapterId, entry.bookId);
      return;
    }
    router.push(`/book/${entry.bookId}`);
  };

  const hero =
    data?.continueListening[0] ??
    data?.library.find((entry) => entry.overallProgress > 0) ??
    data?.library[0];

  if (error) {
    return (
      <div className="container-page py-12">
        <ErrorState
          title="Could not load your listening room"
          message={error}
          onRetry={reload}
          className="mt-6"
        />
      </div>
    );
  }

  if (loading && !data) return <HomeSkeleton />;
  if (!data) return null;

  if (data.stats.bookCount === 0) {
    return <FirstRun />;
  }

  const rest = data.continueListening.slice(1);

  return (
    <div className="container-page pb-16">
      <div className="flex items-center justify-between border-b border-hairline py-3">
        <p className="eyebrow">The listening room</p>
        <p className="eyebrow tabular-nums">
          Vol. I · {pluralize(data.stats.bookCount, 'title')}
        </p>
      </div>

      {hero && <Hero book={hero} onResume={() => resume(hero)} />}

      <StatsLedger stats={data.stats} favorites={data.favorites.length} />

      {rest.length > 0 && (
        <section className="mt-12">
          <SectionHeader
            index="01"
            title="Continue listening"
            href="/continue"
            linkLabel="All in progress"
          />
          <ol className="border-t border-hairline">
            {rest.map((entry, index) => {
              const summary = entry.book;
              return (
                <li key={entry.libraryId} className="border-b border-hairline">
                  <div className="flex items-center gap-3 py-3 sm:gap-5">
                    <span className="hidden w-5 shrink-0 font-mono text-[10px] tabular-nums text-ink-subtle sm:block">
                      {String(index + 2).padStart(2, '0')}
                    </span>

                    <Link
                      href={`/book/${entry.bookId}`}
                      className="shrink-0 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent"
                      tabIndex={-1}
                      aria-hidden
                    >
                      <BookCover src={summary.coverUrl} title={summary.title} size="xs" />
                    </Link>

                    <Link href={`/book/${entry.bookId}`} className="min-w-0 flex-1">
                      <p className="truncate text-[13.5px] font-medium text-ink hover:text-accent">
                        {summary.title}
                      </p>
                      <p className="mt-0.5 truncate text-[11.5px] text-ink-subtle">
                        {summary.author}
                        {entry.lastChapterTitle && ` · ${entry.lastChapterTitle}`}
                        {entry.lastPosition > 0 && ` · ${formatDuration(entry.lastPosition)} in`}
                      </p>
                    </Link>

                    <div className="hidden w-32 shrink-0 sm:block md:w-40">
                      <ProgressBar
                        value={entry.overallProgress}
                        label={`${summary.title} progress`}
                      />
                      <p className="mt-1.5 font-mono text-[9.5px] uppercase tracking-[0.06em] text-ink-subtle">
                        {formatPercent(entry.overallProgress)} read
                      </p>
                    </div>

                    <Button
                      variant="ghost"
                      size="icon-sm"
                      onClick={() => resume(entry)}
                      aria-label={`Resume ${summary.title}`}
                      className="shrink-0"
                    >
                      <Play className="size-4 fill-current" aria-hidden />
                    </Button>
                  </div>
                </li>
              );
            })}
          </ol>
        </section>
      )}

      {data.recentlyAdded.length > 0 && (
        <section className="mt-12">
          <SectionHeader
            index={rest.length > 0 ? '02' : '01'}
            title="On the shelf"
            href="/library"
            linkLabel="The whole library"
          />
          <BookRow
            books={data.recentlyAdded.slice(0, 12).map((entry) => ({
              book: entry,
              onPlay: entry.lastPlayedChapterId ? () => resume(entry) : undefined,
            }))}
          />
        </section>
      )}

      {data.bookmarks.length > 0 && (
        <section className="mt-12">
          <SectionHeader
            index={rest.length > 0 ? '03' : '02'}
            title="Saved moments"
            href="/bookmarks"
            linkLabel={`All ${data.stats.bookmarkCount}`}
            description="Notes and markers you left while listening."
          />
          <ul className="grid gap-x-8 gap-y-6 sm:grid-cols-2 lg:grid-cols-4">
            {data.bookmarks.map((bookmark) => (
              <li key={bookmark.id} className="border-t border-hairline pt-3">
                <p className="line-clamp-2-safe font-display text-[16px] italic leading-snug text-ink">
                  {bookmark.note ?? bookmark.chapterTitle}
                </p>
                <p className="mt-3 font-mono text-[9.5px] uppercase leading-relaxed tracking-[0.08em] text-ink-subtle">
                  {bookmark.bookTitle}
                  <br />
                  {formatDuration(bookmark.timestamp)} ·{' '}
                  {formatRelativeTime(bookmark.createdAt)}
                </p>
                <Link
                  href={`/book/${bookmark.bookId}`}
                  className="mt-2 inline-block font-mono text-[10px] uppercase tracking-[0.08em] text-accent hover:text-accent-deep"
                >
                  Return to book
                </Link>
              </li>
            ))}
          </ul>
        </section>
      )}

      {data.favorites.length > 0 && (
        <section className="mt-12">
          <SectionHeader
            index={data.bookmarks.length > 0 ? '04' : '03'}
            title="Starred"
            href="/favorites"
            linkLabel="All favorites"
          />
          <BookRow
            books={data.favorites.slice(0, 12).map((entry) => ({
              book: entry,
              onPlay: entry.lastPlayedChapterId ? () => resume(entry) : undefined,
            }))}
          />
        </section>
      )}

      <section className="mt-12">
        <SectionHeader
          index={
            data.favorites.length > 0
              ? data.bookmarks.length > 0
                ? '05'
                : '04'
              : data.bookmarks.length > 0
                ? '04'
                : rest.length > 0
                  ? '03'
                  : '02'
          }
          title="Beyond the shelf"
          href="/store"
          linkLabel="Browse all"
          description="Books you do not own yet, drawn from the genres already on your shelf."
        />
        {data.recommendedDegraded && (
          <p className="mb-5 border-l-2 border-caution pl-3 text-[12px] leading-relaxed text-ink-muted">
            The book service is unreachable, so these are local catalogue results. Search will work
            again once it recovers.
          </p>
        )}
        {data.recommended.length > 0 ? (
          <RecommendationShelf books={data.recommended} onChanged={reload} />
        ) : (
          <p className="text-[13px] text-ink-muted">
            No recommendations right now.{' '}
            <Link href="/store" className="text-accent underline underline-offset-4">
              Search the catalogue
            </Link>{' '}
            to add a few more books.
          </p>
        )}
      </section>
    </div>
  );
}

/* -------------------------------------------------------------------------- */
/* Hero                                                                        */
/* -------------------------------------------------------------------------- */

function Hero({ book, onResume }: { book: LibraryBookInfo; onResume: () => void }) {
  const summary = book.book;
  const started = book.overallProgress > 0;
  const completed = book.completed;
  const canResume = Boolean(book.lastPlayedChapterId);

  return (
    <section
      aria-labelledby="hero-heading"
      className="grid gap-8 border-b border-ink py-8 sm:py-10 lg:grid-cols-12 lg:items-end lg:gap-12"
    >
      <div className="order-2 lg:order-1 lg:col-span-7">
        <p className="eyebrow flex items-center gap-2">
          <span aria-hidden className="size-1.5 rounded-full bg-accent" />
          {completed ? 'Finished — listen again' : started ? 'Continue listening' : 'From the shelf'}
        </p>

        <h1
          id="hero-heading"
          className="mt-4 max-w-[18ch] font-display text-[clamp(2.6rem,6.2vw,4.6rem)] leading-[0.94] font-medium tracking-[-0.035em] text-ink"
        >
          <Link href={`/book/${book.bookId}`} className="transition-colors hover:text-accent">
            {summary.title}
          </Link>
        </h1>

        <p className="mt-4 font-display text-[17px] italic leading-snug text-ink-muted">
          {summary.author}
        </p>

        {summary.description && (
          <p className="mt-4 line-clamp-2-safe max-w-xl text-[13.5px] leading-relaxed text-ink-muted">
            {summary.description}
          </p>
        )}

        <div className="mt-7 max-w-md">
          {started ? (
            <>
              <div className="flex items-baseline justify-between gap-4">
                <span className="eyebrow truncate">
                  {book.lastChapterTitle
                    ? `${book.lastChapterNumber ? `Ch. ${book.lastChapterNumber} · ` : ''}${book.lastChapterTitle}`
                    : 'Progress'}
                </span>
                <span className="font-mono text-[10px] tabular-nums text-ink-subtle">
                  {formatPercent(book.overallProgress)}
                </span>
              </div>
              <ProgressBar
                value={book.overallProgress}
                label={`${summary.title} progress`}
                size="md"
                className="mt-2.5"
              />
              <p className="mt-2 text-[11px] text-ink-subtle">
                {book.lastPosition > 0 && `${formatDuration(book.lastPosition)} in · `}
                {book.chaptersWithAudio} of {book.chapterCount} chapters narrated
              </p>
            </>
          ) : (
            <p className="text-[12.5px] leading-relaxed text-ink-muted">
              {book.chaptersWithAudio > 0
                ? `${book.chaptersWithAudio} of ${book.chapterCount} chapters are narrated and ready to play.`
                : 'No audio yet — open the book to upload audio or narrate a chapter.'}
            </p>
          )}
        </div>

        <div className="mt-6 flex flex-wrap items-center gap-2.5">
          {canResume ? (
            <Button
              onClick={onResume}
              icon={<Play className="size-3.5 fill-current" aria-hidden />}
            >
              {completed ? 'Listen again' : started ? 'Resume' : 'Start listening'}
            </Button>
          ) : (
            <Button
              asChild
              icon={<Headphones className="size-3.5" aria-hidden />}
            >
              <Link href={`/book/${book.bookId}`}>Open the book</Link>
            </Button>
          )}
          <Button asChild variant="outline">
            <Link href={`/book/${book.bookId}`}>Chapters</Link>
          </Button>
        </div>
      </div>

      <div className="order-1 flex justify-start lg:order-2 lg:col-span-5 lg:justify-end">
        <div className="relative w-48 sm:w-56 lg:w-[17rem]">
          <div aria-hidden className="absolute -right-2 -bottom-2 h-full w-full border border-hairline" />
          <BookCover
            src={summary.coverUrl}
            title={summary.title}
            size="hero"
            className="relative w-full shadow-[0_22px_44px_-30px_rgb(28_25_20_/_0.7)]"
          />
          <p className="eyebrow mt-4 text-right">
            Narrated · {formatDurationShort(book.totalAudioDuration)}
          </p>
        </div>
      </div>
    </section>
  );
}

/* -------------------------------------------------------------------------- */
/* Ledger                                                                      */
/* -------------------------------------------------------------------------- */

function StatsLedger({ stats, favorites }: { stats: HomeStats; favorites: number }) {
  const entries = [
    { value: stats.bookCount, label: 'Titles', note: `${favorites} starred` },
    {
      value: stats.chapterCount,
      label: 'Chapters',
      note: `${stats.chaptersWithAudio} narrated`,
    },
    {
      value: formatDurationShort(stats.totalAudioDuration),
      label: 'Listening',
      note: 'of audio on file',
    },
    { value: stats.completedCount, label: 'Finished', note: pluralize(stats.bookmarkCount, 'bookmark') },
  ];

  return (
    <dl className="grid grid-cols-2 border-b border-hairline md:grid-cols-4">
      {entries.map((entry, index) => (
        <div
          key={entry.label}
          className={cn(
            'flex flex-col gap-1.5 py-5 md:px-6',
            index % 2 === 1 && 'border-l border-hairline',
            index >= 2 && 'border-t border-hairline md:border-t-0',
            index === 2 && 'md:border-l',
            index === 0 && 'md:pl-0',
          )}
        >
          <dd className="order-1 font-display text-[30px] leading-none font-medium tabular-nums tracking-[-0.02em] text-ink">
            {entry.value}
          </dd>
          <dt className="order-2 eyebrow">{entry.label}</dt>
          <p className="order-3 text-[11px] text-ink-subtle">{entry.note}</p>
        </div>
      ))}
    </dl>
  );
}

/* -------------------------------------------------------------------------- */
/* First run                                                                   */
/* -------------------------------------------------------------------------- */

/** First-run guidance. There is no sign-up step, so this has to explain the flow. */
function FirstRun() {
  return (
    <div className="container-page pb-16">
      <div className="flex items-center justify-between border-b border-hairline py-3">
        <p className="eyebrow">The listening room</p>
        <p className="eyebrow">Vol. I · No. 00</p>
      </div>

      <section className="border-b border-ink py-12 sm:py-16">
        <p className="eyebrow flex items-center gap-2">
          <span aria-hidden className="size-1.5 rounded-full bg-accent" />
          The shelf is empty
        </p>
        <h1 className="mt-4 max-w-[16ch] font-display text-[clamp(2.6rem,7vw,5rem)] leading-[0.94] font-medium tracking-[-0.035em]">
          Your library begins with a search.
        </h1>
        <p className="mt-5 max-w-xl text-[14px] leading-relaxed text-ink-muted">
          VoiceBook has no accounts and no sign-in. Find a book in the public catalogue, shelve it,
          then add audio or let the narrator read it to you.
        </p>
        <div className="mt-7 flex flex-wrap gap-2.5">
          <Button asChild size="lg" icon={<Search className="size-4" aria-hidden />}>
            <Link href="/store">Find a book</Link>
          </Button>
          <Button asChild variant="outline" size="lg">
            <Link href="/library">See the library</Link>
          </Button>
        </div>

        <ol className="mt-12 grid gap-6 border-t border-hairline pt-6 sm:grid-cols-3 sm:gap-10">
          {[
            ['01', 'Find', 'Search millions of titles by name or author.'],
            ['02', 'Shelve', 'Add a book to your own private library.'],
            ['03', 'Listen', 'Narrate a chapter or import your own audio.'],
          ].map(([number, title, body]) => (
            <li key={number} className="flex gap-4">
              <span className="font-mono text-[10px] tabular-nums text-accent">{number}</span>
              <div>
                <p className="font-display text-[19px] leading-tight text-ink">{title}</p>
                <p className="mt-1.5 text-[12.5px] leading-relaxed text-ink-muted">{body}</p>
              </div>
            </li>
          ))}
        </ol>
      </section>
    </div>
  );
}

/* -------------------------------------------------------------------------- */
/* Recommendations                                                             */
/* -------------------------------------------------------------------------- */

function RecommendationShelf({
  books,
  onChanged,
}: {
  books: BookSummary[];
  onChanged: () => void;
}) {
  const [pendingId, setPendingId] = React.useState<string | null>(null);

  return (
    <ul className="no-scrollbar -mx-5 flex gap-5 overflow-x-auto px-5 pb-2 sm:mx-0 sm:px-0">
      {books.map((book) => (
        <li key={book.id} className="w-36 shrink-0 sm:w-40">
          <article className="group flex flex-col">
            <Link
              href={`/book/${book.id}`}
              className="block overflow-hidden rounded-[2px] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent"
            >
              <BookCover
                src={book.coverUrl}
                title={book.title}
                size="md"
                className="w-full transition-opacity duration-200 group-hover:opacity-92"
              />
            </Link>
            <h3 className="mt-2.5 line-clamp-2-safe text-[13px] leading-tight font-medium text-ink">
              <Link href={`/book/${book.id}`} className="hover:text-accent">
                {book.title}
              </Link>
            </h3>
            <p className="mt-1 truncate font-mono text-[9.5px] uppercase tracking-[0.08em] text-ink-subtle">
              {book.author}
            </p>
            {book.inLibrary ? (
              <Link
                href={`/book/${book.id}`}
                className="mt-2 inline-flex w-fit items-center font-mono text-[10px] uppercase tracking-[0.08em] text-accent hover:text-accent-deep"
              >
                On the shelf
              </Link>
            ) : (
              <Button
                variant="ghost"
                size="sm"
                className="mt-1.5 w-fit px-0 text-[10px] text-ink-muted hover:bg-transparent hover:text-accent"
                loading={pendingId === book.id}
                icon={<BookPlus className="size-3.5" aria-hidden />}
                onClick={async () => {
                  setPendingId(book.id);
                  try {
                    await api.post('/api/library', { bookId: book.id });
                    toast.success(`Added “${book.title}” to your library.`);
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

/* -------------------------------------------------------------------------- */
/* Loading                                                                     */
/* -------------------------------------------------------------------------- */

function HomeSkeleton() {
  return (
    <div className="container-page pb-16" role="status" aria-label="Loading your listening room">
      <div className="flex items-center justify-between border-b border-hairline py-3">
        <Skeleton className="h-2.5 w-28" />
        <Skeleton className="h-2.5 w-20" />
      </div>
      <div className="grid gap-8 border-b border-ink py-10 lg:grid-cols-12">
        <div className="lg:col-span-7">
          <Skeleton className="h-2.5 w-32" />
          <Skeleton className="mt-5 h-12 w-4/5" />
          <Skeleton className="mt-4 h-4 w-40" />
          <Skeleton className="mt-5 h-3 w-2/3" />
          <Skeleton className="mt-8 h-1 w-80 max-w-full" />
          <Skeleton className="mt-6 h-9 w-32" />
        </div>
        <div className="lg:col-span-5 lg:justify-self-end">
          <Skeleton className="aspect-[2/3] w-48 sm:w-56 lg:w-64" />
        </div>
      </div>
      <div className="grid grid-cols-2 border-b border-hairline md:grid-cols-4">
        {Array.from({ length: 4 }, (_, index) => (
          <div key={index} className="py-5 md:px-6">
            <Skeleton className="h-7 w-16" />
            <Skeleton className="mt-2 h-2.5 w-12" />
          </div>
        ))}
      </div>
      <div className="mt-12">
        <Skeleton className="h-6 w-48" />
        <div className="mt-5 flex gap-5">
          {Array.from({ length: 6 }, (_, index) => (
            <div key={index} className="w-36 shrink-0 sm:w-40">
              <Skeleton className="aspect-[2/3] w-full" />
              <Skeleton className="mt-2.5 h-3 w-4/5" />
              <Skeleton className="mt-2 h-2.5 w-3/5" />
            </div>
          ))}
        </div>
      </div>
      <span className="sr-only-focusable">Loading…</span>
    </div>
  );
}
