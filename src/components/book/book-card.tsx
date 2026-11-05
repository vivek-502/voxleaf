'use client';

import * as React from 'react';
import Link from 'next/link';
import { Check, Heart, Library, Play } from 'lucide-react';

import { toast } from 'sonner';

import { BookCover } from './book-cover';
import { Badge } from '@/components/ui/primitives';
import { Button } from '@/components/ui/button';
import { TooltipWrap } from '@/components/ui/tooltip';
import { api, errorMessage } from '@/hooks/use-api';
import { formatPercent, pluralize } from '@/utils/format';
import { cn } from '@/utils/cn';
import type { BookSummary, LibraryBookInfo } from '@/types';

/* -------------------------------------------------------------------------- */
/* Progress bar                                                                */
/* -------------------------------------------------------------------------- */

export interface ProgressBarProps {
  value: number;
  label?: string;
  showValue?: boolean;
  className?: string;
  size?: 'sm' | 'md';
}

/** Thin, ruled progress bar. `value` is a 0..1 fraction. */
export function ProgressBar({
  value,
  label,
  showValue = false,
  className,
  size = 'sm',
}: ProgressBarProps) {
  const clamped = Math.max(0, Math.min(1, value || 0));
  const percent = Math.round(clamped * 100);

  return (
    <div className={cn('w-full', className)}>
      {showValue && (
        <div className="mb-1.5 flex items-baseline justify-between">
          <span className="eyebrow">{label}</span>
          <span className="font-mono text-[10px] tabular-nums text-ink-subtle">{percent}%</span>
        </div>
      )}
      <div
        role="progressbar"
        aria-valuenow={percent}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-label={label ?? 'Progress'}
        className={cn('w-full overflow-hidden bg-hairline', size === 'sm' ? 'h-[3px]' : 'h-1')}
      >
        <div
          className="h-full bg-accent transition-[width] duration-300 ease-[var(--ease-out-quint)]"
          style={{ width: `${percent}%` }}
        />
      </div>
    </div>
  );
}

/* -------------------------------------------------------------------------- */
/* Book card                                                                   */
/* -------------------------------------------------------------------------- */

export interface BookCardProps {
  book: BookSummary;
  /** When present, the card shows library progress and a resume affordance. */
  library?: LibraryBookInfo | null;
  onPlay?: () => void;
  className?: string;
}

/**
 * Shelf card. Artwork first; the title is the primary link and the play
 * control is a separate sibling button, never nested inside the link.
 */
export function BookCard({ book, library, onPlay, className }: BookCardProps) {
  const started = Boolean(library && library.overallProgress > 0);
  const completed = Boolean(library?.completed);

  return (
    <article className={cn('group relative flex flex-col gap-2.5', className)}>
      <div className="relative">
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

        {book.favorite && (
          <span className="absolute top-2 left-2 grid size-6 place-items-center border border-hairline bg-canvas/90 text-accent">
            <Heart className="size-3 fill-current" aria-hidden />
            <span className="sr-only-focusable">Favorite</span>
          </span>
        )}

        {book.inLibrary && (
          <span className="absolute top-2 right-2">
            <Badge variant="accent" className="bg-canvas/90 backdrop-blur-[2px]">
              <Check className="size-2.5" aria-hidden />
              Shelved
            </Badge>
          </span>
        )}

        {onPlay && (
          <button
            type="button"
            onClick={onPlay}
            aria-label={completed ? `Replay ${book.title}` : `Resume ${book.title}`}
            className="absolute right-2 bottom-2 grid size-9 place-items-center rounded-[2px] bg-ink text-canvas opacity-0 transition-opacity duration-150 group-hover:opacity-100 focus-visible:opacity-100"
          >
            <Play className="size-3.5 translate-x-px fill-current" aria-hidden />
          </button>
        )}
      </div>

      <div className="min-w-0">
        <h3 className="truncate text-[13.5px] leading-tight font-medium text-ink">
          <Link href={`/book/${book.id}`} className="hover:text-accent">
            {book.title}
          </Link>
        </h3>
        <p className="mt-1 truncate font-mono text-[9.5px] uppercase tracking-[0.08em] text-ink-subtle">
          {book.author}
        </p>

        {library && started && (
          <div className="mt-2.5">
            <ProgressBar value={library.overallProgress} label={`${book.title} progress`} />
            <p className="mt-1.5 truncate text-[11px] text-ink-subtle">
              {completed
                ? 'Finished'
                : `${formatPercent(library.overallProgress)} · ${library.chaptersWithAudio} of ${library.chapterCount} ${pluralize(library.chapterCount, 'chapter')} with audio`}
            </p>
          </div>
        )}

        {library && !started && (
          <p className="mt-1.5 truncate text-[11px] text-ink-subtle">
            {library.chaptersWithAudio === 0
              ? 'No audio yet'
              : `${pluralize(library.chaptersWithAudio, 'chapter')} ready`}
          </p>
        )}
      </div>
    </article>
  );
}

/* -------------------------------------------------------------------------- */
/* Book grid                                                                   */
/* -------------------------------------------------------------------------- */

export function BookGrid({ books, className }: { books: BookCardProps[]; className?: string }) {
  return (
    <ul
      className={cn(
        'grid grid-cols-2 gap-x-5 gap-y-8 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6',
        className,
      )}
    >
      {books.map((entry, index) => (
        <li key={`${entry.book.id}-${index}`} className="flex">
          <BookCard {...entry} className="w-full" />
        </li>
      ))}
    </ul>
  );
}

/* -------------------------------------------------------------------------- */
/* Horizontal row                                                              */
/* -------------------------------------------------------------------------- */

export interface BookRowProps {
  book: LibraryBookInfo;
  onPlay?: () => void;
}

/** Wide card used in Continue / Recently-listened shelves. */
export function BookRowCard({ book, onPlay }: BookRowProps) {
  const summary = book.book;
  const label = book.completed
    ? `Replay ${summary.title}`
    : book.overallProgress > 0
      ? `Resume ${summary.title}`
      : `Play ${summary.title}`;

  return (
    <article className="group relative w-36 shrink-0 sm:w-40">
      <div className="relative">
        <Link
          href={`/book/${book.bookId}`}
          className="block overflow-hidden rounded-[2px] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent"
        >
          <BookCover
            src={summary.coverUrl}
            title={summary.title}
            size="md"
            className="w-full transition-opacity duration-200 group-hover:opacity-92"
          />
        </Link>

        {book.favorite && (
          <span className="absolute top-2 left-2 grid size-6 place-items-center border border-hairline bg-canvas/90 text-accent">
            <Heart className="size-3 fill-current" aria-hidden />
            <span className="sr-only-focusable">Favorite</span>
          </span>
        )}

        {onPlay && (
          <button
            type="button"
            onClick={onPlay}
            aria-label={label}
            className="absolute right-2 bottom-2 grid size-9 place-items-center rounded-[2px] bg-ink text-canvas opacity-0 transition-opacity duration-150 group-hover:opacity-100 focus-visible:opacity-100"
          >
            <Play className="size-3.5 translate-x-px fill-current" aria-hidden />
          </button>
        )}
      </div>

      <div className="mt-2.5">
        <h3 className="truncate text-[13.5px] leading-tight font-medium text-ink">
          <Link href={`/book/${book.bookId}`} className="hover:text-accent">
            {summary.title}
          </Link>
        </h3>
        <p className="mt-1 truncate font-mono text-[9.5px] uppercase tracking-[0.08em] text-ink-subtle">
          {summary.author}
        </p>

        {book.overallProgress > 0 ? (
          <div className="mt-2.5">
            <ProgressBar value={book.overallProgress} label={`${summary.title} progress`} />
            <p className="mt-1.5 truncate text-[11px] text-ink-subtle">
              {book.completed ? 'Finished' : `${formatPercent(book.overallProgress)} complete`}
            </p>
          </div>
        ) : (
          <p className="mt-1.5 truncate text-[11px] text-ink-subtle">
            {book.chaptersWithAudio === 0 ? 'No audio yet' : 'Ready to start'}
          </p>
        )}
      </div>
    </article>
  );
}

/** Horizontally scrollable shelf. Scrollbar is hidden but scroll stays keyboard-reachable. */
export function BookRow({ books, className }: { books: BookRowProps[]; className?: string }) {
  return (
    <ul
      className={cn(
        'no-scrollbar -mx-5 flex gap-5 overflow-x-auto scroll-smooth px-5 pb-2 sm:mx-0 sm:px-0',
        className,
      )}
    >
      {books.map((entry) => (
        <li key={entry.book.libraryId} className="flex">
          <BookRowCard {...entry} />
        </li>
      ))}
    </ul>
  );
}

/* -------------------------------------------------------------------------- */
/* Add-to-library control                                                      */
/* -------------------------------------------------------------------------- */

/**
 * Primary store action. Reflects all three states of a book relative to the
 * library so the user never has to guess whether adding worked.
 */
export function AddToLibraryButton({
  book,
  onAdded,
  className,
}: {
  book: BookSummary;
  onAdded?: () => void;
  className?: string;
}) {
  const [state, setState] = React.useState<'idle' | 'working'>('idle');
  const [added, setAdded] = React.useState(book.inLibrary);
  const inLibrary = added || book.inLibrary;

  const add = async () => {
    setState('working');
    try {
      await api.post('/api/library', { bookId: book.id });
      setAdded(true);
      onAdded?.();
    } catch (error) {
      toast.error(errorMessage(error, 'Could not add this book.'));
    } finally {
      setState('idle');
    }
  };

  return (
    <Button
      variant={inLibrary ? 'secondary' : 'primary'}
      onClick={inLibrary ? undefined : () => void add()}
      loading={state === 'working'}
      disabled={inLibrary}
      icon={inLibrary ? <Check className="size-4" aria-hidden /> : <Library className="size-4" aria-hidden />}
      className={className}
    >
      {inLibrary ? 'On the shelf' : 'Add to library'}
    </Button>
  );
}

/** Small favourite toggle for a book on the shelf. */
export function FavoriteToggle({
  libraryId,
  initial,
  className,
  size = 'icon' as const,
  onToggled,
}: {
  libraryId: string;
  initial: boolean;
  className?: string;
  size?: 'icon' | 'sm';
  onToggled?: (favorite: boolean) => void;
}) {
  const [favorite, setFavorite] = React.useState(initial);
  const [busy, setBusy] = React.useState(false);
  const last = React.useRef(initial);
  React.useEffect(() => setFavorite(initial), [initial]);

  const toggle = async () => {
    const next = !favorite;
    setFavorite(next); // optimistic
    setBusy(true);
    try {
      await api.patch(`/api/library/${libraryId}`, { favorite: next });
      last.current = next;
      onToggled?.(next);
    } catch (error) {
      setFavorite(last.current); // roll back
      toast.error(errorMessage(error, 'Could not update favorites.'));
    } finally {
      setBusy(false);
    }
  };

  return (
    <TooltipWrap label={favorite ? 'Remove from favorites' : 'Add to favorites'}>
      <Button
        variant="outline"
        size={size}
        onClick={() => void toggle()}
        disabled={busy}
        aria-pressed={favorite}
        aria-label={favorite ? 'Remove from favorites' : 'Add to favorites'}
        className={cn(favorite && 'border-accent-line text-accent', className)}
      >
        <Heart className={cn('size-4', favorite && 'fill-current')} aria-hidden />
        {size === 'sm' && (favorite ? 'Favorited' : 'Favorite')}
      </Button>
    </TooltipWrap>
  );
}
