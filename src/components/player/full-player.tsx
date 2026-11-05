'use client';

import * as React from 'react';
import { toast } from 'sonner';
import {
  ArrowRight,
  BookmarkPlus,
  ChevronDown,
  FileAudio,
  Heart,
  Loader2,
  Moon,
  Pause,
  Play,
  RotateCcw,
  RotateCw,
  SkipBack,
  SkipForward,
} from 'lucide-react';

import { Timeline } from './timeline';
import { SpeedMenu } from './speed-menu';
import { SleepTimerMenu } from './sleep-timer-menu';
import { VolumeControl } from './volume-control';
import { BookCover } from '@/components/book/book-cover';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Label, Textarea } from '@/components/ui/inputs';
import { TooltipWrap } from '@/components/ui/tooltip';
import { usePlayer } from '@/components/providers/player-provider';
import { useSettings } from '@/components/providers/settings-provider';
import { api, errorMessage } from '@/hooks/use-api';
import { describeSleepTimer } from '@/hooks/use-sleep-timer';
import { formatDuration } from '@/utils/format';
import { cn } from '@/utils/cn';

/**
 * Full-screen player — the listening room.
 *
 * Opens over the app without unmounting anything, so audio continues
 * uninterrupted. On `sm` and up it is a centred panel; on phones it is a
 * bottom sheet. Escape and the backdrop both close it.
 */
export function FullPlayer() {
  const {
    track,
    isPlaying,
    isLoading,
    status,
    error,
    toggle,
    skipForward,
    skipBackward,
    hasNext,
    hasPrevious,
    playNext,
    playPrevious,
    isExpanded,
    collapse,
    setFavorite,
    sleepTimer,
    position,
  } = usePlayer();
  const { skipForwardSeconds, skipBackwardSeconds } = useSettings();
  const panelRef = React.useRef<HTMLDivElement>(null);
  const [bookmarkOpen, setBookmarkOpen] = React.useState(false);

  // Escape to close, and lock the page behind the sheet from scrolling.
  React.useEffect(() => {
    if (!isExpanded) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape' || bookmarkOpen) return;
      // Menus and popovers portal their own content and handle Escape
      // themselves; without this check the same press also closes the player.
      const path = event.composedPath();
      const inMenu = path.some(
        (node) =>
          node instanceof HTMLElement &&
          (node.getAttribute('role') === 'menu' ||
            node.hasAttribute('data-radix-popper-content-wrapper')),
      );
      if (inMenu || document.querySelector('[role="menu"][data-state="open"]')) return;
      event.preventDefault();
      collapse();
    };
    // Capture phase: the menu's own handler removes the popper before a
    // bubble-phase listener here could see that it owned the press.
    document.addEventListener('keydown', onKeyDown, true);
    const previousOverflow = document.body.style.overflow;
    document.body.style.overflow = 'hidden';
    panelRef.current?.focus();
    return () => {
      document.removeEventListener('keydown', onKeyDown, true);
      document.body.style.overflow = previousOverflow;
    };
  }, [isExpanded, collapse, bookmarkOpen]);

  if (!isExpanded || !track) return null;

  // "End of chapter" / "12:30 left" — null when the timer is off.
  const sleepLabel = describeSleepTimer(sleepTimer);

  return (
    <div className="fixed inset-0 z-100 flex items-end justify-center sm:items-center sm:p-6">
      <button
        type="button"
        onClick={collapse}
        aria-label="Close player"
        className="absolute inset-0 cursor-default bg-ink/70 backdrop-blur-sm"
        style={{ animation: 'fade-in 180ms var(--ease-out-quint)' }}
      />

      <div
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-label={`Now playing ${track.bookTitle}`}
        tabIndex={-1}
        className={cn(
          'relative flex max-h-[calc(100dvh-0.75rem)] w-full max-w-2xl flex-col overflow-hidden outline-none',
          'border border-hairline-strong bg-surface shadow-[var(--shadow-lift)]',
          'sm:max-h-[calc(100dvh-3rem)]',
          'max-sm:border-x-0 max-sm:border-b-0',
        )}
        style={{ animation: 'player-in 260ms var(--ease-out-quint)' }}
      >
        <div className="mx-auto mt-3 h-1 w-10 shrink-0 bg-hairline-strong sm:hidden" />

        <header className="flex items-center justify-between gap-3 border-b border-hairline px-4 py-2.5 sm:px-6">
          <TooltipWrap label="Minimise">
            <Button variant="ghost" size="icon-sm" onClick={collapse} aria-label="Minimise player">
              <ChevronDown className="size-5" aria-hidden />
            </Button>
          </TooltipWrap>
          <p className="min-w-0 flex-1 truncate text-center font-mono text-[10px] uppercase tracking-[0.16em] text-ink-subtle">
            Now playing
          </p>
          <TooltipWrap label={track.favorite ? 'Remove from favorites' : 'Add to favorites'}>
            <Button
              variant="ghost"
              size="icon-sm"
              onClick={() => void setFavorite(!track.favorite)}
              aria-pressed={track.favorite}
              aria-label={track.favorite ? 'Remove from favorites' : 'Add to favorites'}
              className={cn(track.favorite && 'text-accent')}
            >
              <Heart className={cn('size-[18px]', track.favorite && 'fill-current')} aria-hidden />
            </Button>
          </TooltipWrap>
        </header>

        <div className="flex min-h-0 flex-1 flex-col overflow-y-auto px-5 pt-4 pb-5 sm:px-8 sm:pt-5 sm:pb-6">
          <div className="flex flex-col items-center gap-4 sm:flex-row sm:items-center sm:gap-7">
            <div className="relative aspect-[2/3] h-[clamp(7.5rem,25dvh,12.5rem)] shrink-0 sm:h-[clamp(10rem,36dvh,20rem)]">
              <span
                aria-hidden
                className="absolute -right-2 -bottom-2 inset-y-0 left-2 border border-ink/25"
              />
              <BookCover
                src={track.coverUrl}
                title={track.bookTitle}
                size="hero"
                emphasis
                className="relative"
                style={{ width: '100%', height: '100%' }}
              />
            </div>

            <div className="min-w-0 flex-1 text-center sm:text-left">
              <p className="font-mono text-[10px] uppercase tracking-[0.16em] text-accent">
                {track.author}
              </p>
              <h2 className="line-clamp-2-safe mt-1.5 font-display text-[22px] leading-[1.1] tracking-tight text-ink sm:text-[26px]">
                {track.bookTitle}
              </h2>
              {track.chapterNumber > 0 && (
                <p className="mt-1.5 text-[12.5px] text-ink-muted sm:text-[13px]">
                  <span className="font-mono text-[10.5px] uppercase tracking-[0.08em] text-ink-subtle">
                    Chapter {track.chapterNumber}
                  </span>
                  <span className="mx-2 text-hairline-strong">/</span>
                  {track.chapterTitle}
                </p>
              )}

              {status === 'error' && error && (
                <p role="alert" className="mt-2.5 text-sm text-danger">
                  {error}
                </p>
              )}
            </div>
          </div>

          <div className="mt-4 shrink-0 sm:mt-5">
            <Timeline />

            {/* Transport */}
            <div className="mt-3 flex items-center justify-center gap-1.5 sm:gap-3">
              <TooltipWrap label={`Back ${skipBackwardSeconds} seconds`}>
                <Button
                  variant="ghost"
                  size="icon"
                  onClick={skipBackward}
                  aria-label={`Back ${skipBackwardSeconds} seconds`}
                >
                  <RotateCcw className="size-5" aria-hidden />
                </Button>
              </TooltipWrap>

              <TooltipWrap label={hasPrevious ? 'Previous chapter' : undefined}>
                <Button
                  variant="ghost"
                  size="icon"
                  onClick={() => void playPrevious()}
                  disabled={!hasPrevious}
                  aria-label="Previous chapter"
                >
                  <SkipBack className="size-5" aria-hidden />
                </Button>
              </TooltipWrap>

              <Button
                variant="primary"
                size="icon-lg"
                onClick={() => void toggle()}
                disabled={status === 'error'}
                aria-label={isPlaying ? 'Pause' : 'Play'}
                className="mx-1 size-14"
              >
                {isLoading ? (
                  <Loader2 className="size-6 animate-spin" aria-hidden />
                ) : isPlaying ? (
                  <Pause className="size-6 fill-current" aria-hidden />
                ) : (
                  <Play className="size-6 translate-x-0.5 fill-current" aria-hidden />
                )}
              </Button>

              <TooltipWrap label={hasNext ? 'Next chapter' : undefined}>
                <Button
                  variant="ghost"
                  size="icon"
                  onClick={() => void playNext()}
                  disabled={!hasNext}
                  aria-label="Next chapter"
                >
                  <SkipForward className="size-5" aria-hidden />
                </Button>
              </TooltipWrap>

              <TooltipWrap label={`Forward ${skipForwardSeconds} seconds`}>
                <Button
                  variant="ghost"
                  size="icon"
                  onClick={skipForward}
                  aria-label={`Forward ${skipForwardSeconds} seconds`}
                >
                  <RotateCw className="size-5" aria-hidden />
                </Button>
              </TooltipWrap>
            </div>

            {sleepLabel && (
              <p className="mt-2 flex items-center justify-center gap-1.5 font-mono text-[10px] uppercase tracking-[0.1em] text-ink-subtle">
                <Moon className="size-3.5" aria-hidden />
                Sleeps in {sleepLabel}
              </p>
            )}

            {/* Secondary row */}
            <div className="mx-auto mt-4 flex w-full max-w-md items-center justify-between gap-2 border-t border-hairline pt-3">
              <SpeedMenu variant="ghost" />
              <SleepTimerMenu variant="ghost" />
              <TooltipWrap label="Bookmark this moment">
                <Button
                  variant="ghost"
                  size="icon"
                  onClick={() => setBookmarkOpen(true)}
                  aria-label={`Bookmark this moment at ${formatDuration(position)}`}
                >
                  <BookmarkPlus className="size-[18px]" aria-hidden />
                </Button>
              </TooltipWrap>
              <VolumeControl className="justify-end" />
            </div>

            <BlockedAdvanceNotice />
          </div>
        </div>
      </div>

      <BookmarkMomentDialog open={bookmarkOpen} onOpenChange={setBookmarkOpen} />
    </div>
  );
}

/**
 * Capture the current position as a bookmark, optionally with a note.
 * The saved moment appears on the Saved page and in the book's own list.
 */
function BookmarkMomentDialog({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const { track, position } = usePlayer();
  const [note, setNote] = React.useState('');
  const [busy, setBusy] = React.useState(false);

  React.useEffect(() => {
    if (open) setNote('');
  }, [open]);

  const save = async (event: React.FormEvent) => {
    event.preventDefault();
    if (!track) return;
    setBusy(true);
    try {
      await api.post('/api/bookmarks', {
        chapterId: track.chapterId,
        timestamp: Number(position.toFixed(2)),
        note: note.trim() || undefined,
      });
      toast.success(`Bookmarked ${formatDuration(position)} in “${track.chapterTitle}”.`);
      onOpenChange(false);
    } catch (caught) {
      toast.error(errorMessage(caught, 'Could not save that bookmark.'));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(next) => (!busy ? onOpenChange(next) : undefined)}>
      <DialogContent className="max-w-sm">
        <DialogHeader>
          <DialogTitle>Mark this moment</DialogTitle>
          <DialogDescription>
            {track ? (
              <>
                Pinned at <span className="font-mono tabular-nums">{formatDuration(position)}</span>{' '}
                in “{track.chapterTitle}”.
              </>
            ) : (
              'Pinned at the current position.'
            )}
          </DialogDescription>
        </DialogHeader>

        <form onSubmit={(event) => void save(event)}>
          <Label htmlFor="bookmark-note">Note (optional)</Label>
          <Textarea
            id="bookmark-note"
            value={note}
            onChange={(event) => setNote(event.target.value)}
            placeholder="What is worth coming back to?"
            rows={3}
            maxLength={500}
            className="mt-1.5"
          />
          <DialogFooter>
            <Button
              type="button"
              variant="ghost"
              onClick={() => onOpenChange(false)}
              disabled={busy}
            >
              Cancel
            </Button>
            <Button
              type="submit"
              loading={busy}
              icon={<BookmarkPlus className="size-4" aria-hidden />}
            >
              Save bookmark
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

/**
 * The interstitial for "the next chapter exists but has no audio yet".
 *
 * Auto-advance stops here rather than silently skipping or failing, and offers
 * the two useful actions: attach an audio file, or stay on this chapter.
 */
function BlockedAdvanceNotice() {
  const { blocked, dismissBlocked } = usePlayer();
  if (!blocked) return null;

  return (
    <section
      role="alertdialog"
      aria-labelledby="blocked-title"
      className="mt-6 w-full border-l-2 border-caution pl-4 text-left"
    >
      <div className="flex items-start gap-3">
        <FileAudio className="mt-0.5 size-4 shrink-0 text-caution" aria-hidden />
        <div className="min-w-0 flex-1">
          <h3 id="blocked-title" className="text-sm font-semibold text-ink">
            {blocked.message}
          </h3>
          <p className="mt-1 text-xs leading-relaxed text-ink-muted">
            Chapter {blocked.nextChapterNumber} · {blocked.nextChapterTitle} — import an audio
            file to continue, or stay here and finish this chapter.
          </p>
          <div className="mt-3 flex flex-wrap gap-2">
            <Button
              variant="primary"
              size="sm"
              icon={<ArrowRight className="size-4" aria-hidden />}
              onClick={() => {
                dismissBlocked();
                window.dispatchEvent(
                  new CustomEvent('voicebook:import-audio', {
                    detail: {
                      chapterId: blocked.nextChapterId,
                      bookId: blocked.bookId,
                      libraryId: blocked.libraryId,
                    },
                  }),
                );
              }}
            >
              Import audio
            </Button>
            <Button variant="outline" size="sm" onClick={dismissBlocked}>
              Stay here
            </Button>
          </div>
        </div>
      </div>
    </section>
  );
}
