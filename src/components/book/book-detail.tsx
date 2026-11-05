'use client';

import * as React from 'react';
import Link from 'next/link';
import { useRouter } from 'next/navigation';
import {
  ArrowLeft,
  BookOpen,
  Ellipsis,
  Library,
  Play,
  RotateCcw,
  Trash2,
} from 'lucide-react';

import { BookCover } from './book-cover';
import { ProgressBar, AddToLibraryButton, FavoriteToggle } from './book-card';
import { ChapterList } from '@/components/chapters/chapter-list';
import { EmptyState, ErrorState, ChapterListSkeleton } from '@/components/common/states';
import { Button } from '@/components/ui/button';
import { Eyebrow } from '@/components/ui/primitives';
import { ConfirmDialog } from '@/components/common/confirm-dialog';
import { EpubStatusPanel } from '@/components/reader/epub-status-panel';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { useApiQuery } from '@/hooks/use-async';
import { api, errorMessage } from '@/hooks/use-api';
import { toast } from 'sonner';
import { usePlayer } from '@/components/providers/player-provider';
import {
  formatDuration,
  formatPercent,
  formatRelativeTime,
  pluralize,
} from '@/utils/format';
import type { BookSummary, ChapterInfo, LibraryBookInfo, BookmarkInfo } from '@/types';

interface BookDetailPayload {
  book: BookSummary;
  libraryBook: LibraryBookInfo | null;
  chapters: ChapterInfo[];
  bookmarks: BookmarkInfo[];
}

/**
 * Book detail page, set like a title page.
 *
 * Metadata and the colophon sit beside the jacket; below the rule is the
 * working part of the app — the chapter list, where audio is uploaded,
 * narrated and reordered.
 */
export function BookDetail({ bookId }: { bookId: string }) {
  const router = useRouter();
  const { playChapter } = usePlayer();
  const { data, error, loading, reload } = useApiQuery<BookDetailPayload>(`/api/books/${bookId}`);

  const [removeOpen, setRemoveOpen] = React.useState(false);
  const [resetOpen, setResetOpen] = React.useState(false);

  const book = data?.book;
  const libraryBook = data?.libraryBook ?? null;
  const chapters = data?.chapters ?? [];
  const bookmarks = data?.bookmarks ?? [];

  const resume = () => {
    if (!libraryBook) return;
    if (libraryBook.lastPlayedChapterId) {
      void playChapter(libraryBook.lastPlayedChapterId, libraryBook.bookId);
      return;
    }
    const firstWithAudio = chapters.find((chapter) => chapter.audio);
    if (firstWithAudio) {
      void playChapter(firstWithAudio.id, libraryBook.bookId);
    }
  };

  // Both of these are handed straight to `ConfirmDialog.onConfirm`, which owns
  // the busy state and keeps the dialog open until the returned promise settles.
  const removeFromLibrary = async () => {
    if (!libraryBook) return;
    try {
      await api.delete(`/api/library/${libraryBook.libraryId}`);
      toast.success(`Removed “${book?.title}” from your library.`);
      router.push('/library');
    } catch (error) {
      toast.error(errorMessage(error, 'Could not remove that book.'));
    }
  };

  const resetProgress = async () => {
    if (!libraryBook) return;
    try {
      await api.delete('/api/progress/reset', { bookId: libraryBook.bookId });
      toast.success('Progress has been reset.');
      reload();
    } catch (error) {
      toast.error(errorMessage(error, 'Could not reset progress.'));
    }
  };

  if (error) {
    return (
      <div className="container-page py-10">
        <ErrorState title="Could not load this book" message={error} onRetry={reload} />
      </div>
    );
  }

  if (loading && !data) {
    return (
      <div className="container-page py-10">
        <div className="grid gap-8 sm:grid-cols-[14rem_1fr] lg:grid-cols-[18rem_1fr]">
          <div className="skeleton aspect-[2/3] w-48 rounded-[2px] sm:w-full" />
          <div className="space-y-3">
            <div className="skeleton h-3 w-24" />
            <div className="skeleton h-12 w-2/3" />
            <div className="skeleton h-4 w-1/3" />
            <div className="skeleton h-24 w-full max-w-xl" />
          </div>
        </div>
        <div className="mt-12">
          <ChapterListSkeleton count={6} />
        </div>
      </div>
    );
  }

  if (!book) return null;

  const inLibrary = Boolean(libraryBook);
  const started = Boolean(libraryBook && libraryBook.overallProgress > 0);
  const completed = libraryBook?.completed ?? false;
  const canPlay = chapters.some((chapter) => chapter.audio);

  return (
    <div className="container-page pb-16">
      <div className="mt-6 flex items-center justify-between border-b border-hairline pb-2">
        <Link
          href={inLibrary ? '/library' : '/store'}
          className="inline-flex items-center gap-1.5 font-mono text-[10px] uppercase tracking-[0.09em] text-ink-subtle transition-colors hover:text-ink"
        >
          <ArrowLeft className="size-3" aria-hidden />
          {inLibrary ? 'Library' : 'Discover'}
        </Link>
        <span className="eyebrow">{inLibrary ? 'On your shelf' : 'Catalogue'}</span>
      </div>

      <header className="grid gap-8 border-b border-ink py-8 sm:grid-cols-[14rem_1fr] sm:gap-10 lg:grid-cols-[18rem_1fr] lg:gap-14 lg:py-10">
        <div className="mx-auto w-48 sm:mx-0 sm:w-full">
          <div className="relative">
            <div aria-hidden className="absolute -right-2 -bottom-2 h-full w-full border border-hairline" />
            <BookCover
              src={book.coverUrl}
              title={book.title}
              size="hero"
              emphasis
              className="relative w-full"
            />
          </div>
          <p className="eyebrow mt-4 text-right">
            {book.publicationDate ? book.publicationDate.slice(0, 4) : 'Private edition'}
          </p>
        </div>

        <div className="min-w-0">
          <Eyebrow>{book.author}</Eyebrow>
          <h1 className="mt-2 max-w-[20ch] font-display text-[clamp(2.2rem,5vw,3.8rem)] leading-[0.98] font-medium tracking-[-0.03em] text-ink">
            {book.title}
          </h1>

          {book.subtitle && (
            <p className="mt-3 font-display text-[18px] italic leading-snug text-ink-muted">
              {book.subtitle}
            </p>
          )}

          <div className="mt-5 flex flex-wrap items-center gap-x-2.5 gap-y-1 font-mono text-[10px] uppercase tracking-[0.08em] text-ink-subtle">
            {[
              book.publicationDate,
              book.pageCount ? pluralize(book.pageCount, 'page') : null,
              book.language,
              book.rating !== null && book.rating > 0
                ? `★ ${book.rating.toFixed(1)}${book.ratingCount ? ` (${book.ratingCount})` : ''}`
                : null,
            ]
              .filter(Boolean)
              .map((item, index) => (
                <React.Fragment key={String(item)}>
                  {index > 0 && <span aria-hidden>·</span>}
                  <span>{item}</span>
                </React.Fragment>
              ))}
          </div>

          {book.description && (
            <p className="mt-5 max-w-2xl text-[13.5px] leading-relaxed text-ink-muted">
              {book.description}
            </p>
          )}

          {book.categories.length > 0 && (
            <p className="mt-3 max-w-2xl font-display text-[13px] italic leading-relaxed text-ink-muted">
              {book.categories.slice(0, 8).join(' · ')}
            </p>
          )}

          <div className="mt-7 flex flex-wrap items-center gap-2">
            {libraryBook ? (
              <>
                <Button
                  asChild
                  size="lg"
                  icon={<BookOpen className="size-4" aria-hidden />}
                >
                  <Link href={`/read/${book.id}`}>Read</Link>
                </Button>
                {canPlay && (
                  <Button
                    variant="outline"
                    size="lg"
                    onClick={resume}
                    icon={<Play className="size-4 fill-current" aria-hidden />}
                  >
                    {completed ? 'Listen again' : started ? 'Resume' : 'Start listening'}
                  </Button>
                )}
                <FavoriteToggle
                  libraryId={libraryBook.libraryId}
                  initial={book.favorite}
                  onToggled={reload}
                  className="size-12"
                />
                <DropdownMenu>
                  <DropdownMenuTrigger asChild>
                    <Button variant="ghost" size="icon-lg" aria-label="Manage this book">
                      <Ellipsis className="size-5" aria-hidden />
                    </Button>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent align="start" className="w-52">
                    <DropdownMenuItem
                      onSelect={() => setResetOpen(true)}
                      disabled={!started}
                    >
                      <RotateCcw aria-hidden /> Reset progress
                    </DropdownMenuItem>
                    <DropdownMenuSeparator />
                    <DropdownMenuItem destructive onSelect={() => setRemoveOpen(true)}>
                      <Trash2 aria-hidden /> Remove from library
                    </DropdownMenuItem>
                  </DropdownMenuContent>
                </DropdownMenu>
              </>
            ) : (
              <AddToLibraryButton
                book={book}
                onAdded={reload}
                className="h-12 px-6"
              />
            )}

            {inLibrary && <EpubStatusPanel bookId={book.id} />}
          </div>

          {inLibrary && libraryBook && (
            <div className="mt-7 max-w-md border-t border-hairline pt-4">
              {started ? (
                <>
                  <ProgressBar
                    value={libraryBook.overallProgress}
                    label={`${book.title} progress`}
                    showValue
                  />
                  <p className="mt-2 text-[11px] text-ink-subtle">
                    {completed
                      ? 'Finished'
                      : `${formatPercent(libraryBook.overallProgress)} · ${libraryBook.chaptersWithAudio} of ${libraryBook.chapterCount} chapters with audio`}
                    {libraryBook.lastPlayedAt && (
                      <> · Last played {formatRelativeTime(libraryBook.lastPlayedAt)}</>
                    )}
                  </p>
                </>
              ) : (
                <p className="text-[12px] leading-relaxed text-ink-muted">
                  {libraryBook.chaptersWithAudio === 0
                    ? 'No audio yet. Narrate a chapter or import audio below.'
                    : `${libraryBook.chaptersWithAudio} of ${libraryBook.chapterCount} chapters have audio.`}
                </p>
              )}
            </div>
          )}

          <Colophon book={book} />
        </div>
      </header>

      {inLibrary && bookmarks.length > 0 && (
        <section aria-labelledby="book-bookmarks" className="mb-3 pt-8">
          <div className="mb-2 flex items-end justify-between border-b border-hairline pb-2">
            <h2
              id="book-bookmarks"
              className="font-display text-[22px] font-medium tracking-[-0.02em] text-ink"
            >
              Saved moments
            </h2>
            <Link
              href="/bookmarks"
              className="font-mono text-[10px] uppercase tracking-[0.09em] text-accent hover:text-accent-deep"
            >
              All bookmarks
            </Link>
          </div>
          <ul>
            {bookmarks.slice(0, 5).map((bookmark) => (
              <li
                key={bookmark.id}
                className="flex items-center gap-4 border-b border-hairline py-3"
              >
                <span className="w-16 shrink-0 font-mono text-[13px] tabular-nums text-accent">
                  {formatDuration(bookmark.timestamp)}
                </span>
                <div className="min-w-0 flex-1">
                  <p className="truncate text-[13px] text-ink">
                    {bookmark.chapterNumber > 0
                      ? `Ch. ${bookmark.chapterNumber} · `
                      : ''}
                    {bookmark.chapterTitle}
                  </p>
                  {bookmark.note && (
                    <p className="mt-0.5 line-clamp-2-safe text-[12px] italic text-ink-muted">
                      {bookmark.note}
                    </p>
                  )}
                </div>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  onClick={() => void playChapter(bookmark.chapterId, book.id)}
                  aria-label={`Play from ${formatDuration(bookmark.timestamp)}`}
                >
                  <Play className="size-4 fill-current" aria-hidden />
                </Button>
              </li>
            ))}
          </ul>
        </section>
      )}

      {inLibrary && libraryBook ? (
        <div className="pt-8">
          <ChapterList
            bookId={book.id}
            libraryId={libraryBook.libraryId}
            bookTitle={book.title}
            chapters={chapters}
            loading={loading}
            error={error}
            onRetry={reload}
          />
        </div>
      ) : (
        <div className="pt-8">
          <EmptyState
            icon={Library}
            title="Not on your shelf yet"
            description="Add this book to your library to upload its EPUB, manage chapters and give it a voice."
          />
        </div>
      )}

      <ConfirmDialog
        open={removeOpen}
        onOpenChange={setRemoveOpen}
        title={`Remove “${book.title}”?`}
        description="This removes the book, all its chapters, audio files, bookmarks and listening progress. This cannot be undone."
        confirmLabel="Remove book"
        destructive
        onConfirm={removeFromLibrary}
      />

      <ConfirmDialog
        open={resetOpen}
        onOpenChange={setResetOpen}
        title="Reset listening progress?"
        description="All chapter positions and completion status for this book will be cleared. Your chapters and audio files are not affected."
        confirmLabel="Reset progress"
        onConfirm={resetProgress}
      />
    </div>
  );
}

/** Bibliographic details, set as a small colophon rather than badge pills. */
function Colophon({ book }: { book: BookSummary }) {
  const rows = [
    { label: 'Publisher', value: book.publisher },
    { label: 'Published', value: book.publicationDate },
    { label: 'Pages', value: book.pageCount ? String(book.pageCount) : null },
    { label: 'Language', value: book.language },
    { label: 'ISBN', value: book.isbn },
    {
      label: 'Rating',
      value:
        book.rating !== null && book.rating > 0
          ? `${book.rating.toFixed(1)} / 5${book.ratingCount ? ` · ${book.ratingCount} ratings` : ''}`
          : null,
    },
  ].filter((row) => row.value);

  if (rows.length === 0) return null;

  return (
    <dl className="mt-7 grid max-w-2xl grid-cols-2 gap-x-8 gap-y-3 border-t border-hairline pt-4 sm:grid-cols-3">
      {rows.map((row) => (
        <div key={row.label}>
          <dt className="eyebrow">{row.label}</dt>
          <dd className="mt-0.5 truncate text-[12px] text-ink-muted">{row.value}</dd>
        </div>
      ))}
    </dl>
  );
}
