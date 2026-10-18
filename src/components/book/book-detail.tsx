import * as React from 'react';
import { Link } from 'react-router-dom';
import {
  BookOpen,
  Bookmark,
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
import { Badge, Eyebrow } from '@/components/ui/primitives';
import { ConfirmDialog } from '@/components/common/confirm-dialog';
import { EpubStatusPanel } from '@/components/reader/epub-status-panel';
import { useApiQuery } from '@/hooks/use-async';
import { api, errorMessage } from '@/hooks/use-api';
import { toast } from 'sonner';
import { usePlayer } from '@/components/providers/player-provider';
import { formatDuration, formatPercent, formatRelativeTime, pluralize } from '@/utils/format';
import type { BookSummary, ChapterInfo, LibraryBookInfo, BookmarkInfo } from '@/types';

interface BookDetailPayload {
  book: BookSummary;
  libraryBook: LibraryBookInfo | null;
  chapters: ChapterInfo[];
  bookmarks: BookmarkInfo[];
}

/**
 * Book detail page.
 *
 * Shows metadata, the chapter list with drag-drop reorder, and all the
 * management actions (add to library, favorite, remove, reset progress).
 * The chapter list is the heart of the app — most books have no audio yet,
 * so the UI makes that state visible and actionable.
 */
export function BookDetail({ bookId }: { bookId: string }) {
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
    const firstWithAudio = chapters.find((c) => c.audio);
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
      toast.success(`Removed "${book?.title}" from your library.`);
      window.location.href = '/library';
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
        <div className="flex flex-col gap-6 sm:flex-row">
          <div className="skeleton size-48 rounded-2xl" />
          <div className="flex-1 space-y-3">
            <div className="skeleton h-8 w-2/3" />
            <div className="skeleton h-4 w-1/3" />
            <div className="skeleton h-24 w-full" />
          </div>
        </div>
        <div className="mt-10">
          <ChapterListSkeleton count={6} />
        </div>
      </div>
    );
  }

  if (!book) return null;

  const inLibrary = Boolean(libraryBook);
  const started = libraryBook && libraryBook.overallProgress > 0;
  const completed = libraryBook?.completed ?? false;

  return (
    <div className="container-page py-8 sm:py-10">
      {/* Back link */}
      <Link
        to={inLibrary ? '/library' : '/store'}
        className="mb-6 inline-flex items-center gap-1.5 text-sm text-ink-muted transition-colors hover:text-ink"
      >
        ← Back to {inLibrary ? 'library' : 'store'}
      </Link>

      {/* Header */}
      <header className="mb-8 flex flex-col gap-6 sm:flex-row sm:items-start sm:gap-8">
        <BookCover
          src={book.coverUrl}
          title={book.title}
          size="hero"
          emphasis
          className="mx-auto sm:mx-0"
        />

        <div className="min-w-0 flex-1">
          <Eyebrow>{book.author}</Eyebrow>
          <h1 className="mt-1.5 text-2xl font-semibold text-ink sm:text-3xl">{book.title}</h1>

          {book.subtitle && (
            <p className="mt-1 text-base text-ink-muted">{book.subtitle}</p>
          )}

          {/* Metadata chips */}
          <div className="mt-4 flex flex-wrap items-center gap-2">
            {book.publicationDate && (
              <Badge variant="outline">{book.publicationDate}</Badge>
            )}
            {book.pageCount && (
              <Badge variant="outline">{pluralize(book.pageCount, 'page')}</Badge>
            )}
            {book.language && (
              <Badge variant="outline">{book.language}</Badge>
            )}
            {book.rating !== null && book.rating > 0 && (
              <Badge variant="accent">★ {book.rating.toFixed(1)}</Badge>
            )}
            {book.isbn && (
              <Badge variant="outline">ISBN {book.isbn}</Badge>
            )}
          </div>

          {/* Description */}
          {book.description && (
            <p className="mt-4 max-w-2xl text-sm leading-relaxed text-ink-muted">
              {book.description}
            </p>
          )}

          {/* Categories */}
          {book.categories.length > 0 && (
            <div className="mt-4 flex flex-wrap gap-1.5">
              {book.categories.slice(0, 8).map((category) => (
                <span
                  key={category}
                  className="rounded-full bg-overlay px-2.5 py-1 text-[11px] text-ink-subtle"
                >
                  {category}
                </span>
              ))}
            </div>
          )}

          {/* Actions */}
          <div className="mt-6 flex flex-wrap items-center gap-2">
            {inLibrary ? (
              <>
                {/* Reading is the primary action for a book that has text;
                    listening stays right beside it so both are one click. */}
                <Button
                  asChild
                  size="lg"
                  icon={<BookOpen className="size-4" aria-hidden />}
                >
                  <Link to={`/read/${book.id}`}>Read</Link>
                </Button>
                {started && (
                  <Button
                    onClick={resume}
                    icon={<Play className="size-4 fill-current" aria-hidden />}
                    size="lg"
                  >
                    {completed ? 'Listen again' : 'Resume'}
                  </Button>
                )}
                {!started && chapters.some((c) => c.audio) && (
                  <Button
                    onClick={resume}
                    icon={<Play className="size-4 fill-current" aria-hidden />}
                    size="lg"
                  >
                    Start listening
                  </Button>
                )}
                <FavoriteToggle
                  bookId={book.id}
                  initial={book.favorite}
                />
                {started && (
                  <Button
                    variant="outline"
                    icon={<RotateCcw className="size-4" aria-hidden />}
                    onClick={() => setResetOpen(true)}
                  >
                    Reset progress
                  </Button>
                )}
                <Button
                  variant="danger"
                  icon={<Trash2 className="size-4" aria-hidden />}
                  onClick={() => setRemoveOpen(true)}
                >
                  Remove
                </Button>
              </>
            ) : (
              <AddToLibraryButton book={book} onAdded={reload} className="h-12 px-7 text-[15px]" />
            )}

            {/* The book-file control lives with the actions. With no file it is a
                full-width card that wraps onto its own line below them; once the
                text is ready it collapses to a compact menu, so it never repeats
                the Read button above it. */}
            {inLibrary && <EpubStatusPanel bookId={book.id} />}
          </div>

          {/* Progress summary */}
          {inLibrary && libraryBook && (
            <div className="mt-6 max-w-md">
              {started ? (
                <>
                  <ProgressBar
                    value={libraryBook.overallProgress}
                    label={`${book.title} progress`}
                    showValue
                  />
                  <p className="mt-1.5 text-xs text-ink-subtle">
                    {completed
                      ? 'Completed'
                      : `${formatPercent(libraryBook.overallProgress)}% · ${libraryBook.chaptersWithAudio} of ${libraryBook.chapterCount} chapters with audio`}
                    {libraryBook.lastPlayedAt && (
                      <> · Last played {formatRelativeTime(libraryBook.lastPlayedAt)}</>
                    )}
                  </p>
                </>
              ) : (
                <p className="text-sm text-ink-muted">
                  {libraryBook.chaptersWithAudio === 0
                    ? 'No audio generated or imported yet. Generate TTS or import audio from the reader or chapters list.'
                    : `${libraryBook.chaptersWithAudio} of ${libraryBook.chapterCount} chapters have audio.`}
                </p>
              )}
            </div>
          )}
        </div>
      </header>

      {/* Bookmarks for this book */}
      {inLibrary && bookmarks.length > 0 && (
        <section aria-labelledby="book-bookmarks" className="mb-8">
          <h2 id="book-bookmarks" className="mb-3 text-base font-semibold text-ink">
            Bookmarks in this book
          </h2>
          <ul className="flex flex-col gap-2">
            {bookmarks.slice(0, 5).map((bookmark) => (
              <li
                key={bookmark.id}
                className="card-surface flex items-center gap-3 p-3"
              >
                <Bookmark className="size-4 shrink-0 text-accent-soft" aria-hidden />
                <div className="min-w-0 flex-1">
                  <p className="truncate text-sm text-ink">
                    {bookmark.chapterNumber > 0 ? `Ch. ${bookmark.chapterNumber} · ` : ''}
                    {bookmark.chapterTitle}
                  </p>
                  <p className="text-xs text-ink-subtle">
                    {formatDuration(bookmark.timestamp)}
                    {bookmark.note && ` · ${bookmark.note}`}
                  </p>
                </div>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => void playChapter(bookmark.chapterId, book.id)}
                  icon={<Play className="size-3.5 fill-current" aria-hidden />}
                >
                  Play
                </Button>
              </li>
            ))}
          </ul>
        </section>
      )}

      {/* Chapter list */}
      {inLibrary && libraryBook ? (
        <ChapterList
          bookId={book.id}
          libraryId={libraryBook.libraryId}
          bookTitle={book.title}
          chapters={chapters}
          loading={loading}
          error={error}
          onRetry={reload}
        />
      ) : (
        <EmptyState
          icon={Library}
          title="Not in your library"
          description="Add this book to your library to upload its EPUB, manage chapters and import audio."
        />
      )}

      {/* Remove confirmation */}
      <ConfirmDialog
        open={removeOpen}
        onOpenChange={setRemoveOpen}
        title={`Remove "${book.title}"?`}
        description="This removes the book, all its chapters, audio files, bookmarks and listening progress. This cannot be undone."
        confirmLabel="Remove book"
        destructive
        onConfirm={removeFromLibrary}
      />

      {/* Reset progress confirmation */}
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
