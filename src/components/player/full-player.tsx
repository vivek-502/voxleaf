import * as React from 'react';
import {
  ArrowRight,
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
import { Button } from '@/components/ui/button';
import { TooltipWrap } from '@/components/ui/tooltip';
import { usePlayer } from '@/components/providers/player-provider';
import { useSettings } from '@/components/providers/settings-provider';
import { describeSleepTimer } from '@/hooks/use-sleep-timer';
import { cn } from '@/utils/cn';

/**
 * Full-screen player.
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
  } = usePlayer();
  const { skipForwardSeconds, skipBackwardSeconds } = useSettings();
  const panelRef = React.useRef<HTMLDivElement>(null);

  // Escape to close, and lock the page behind the sheet from scrolling.
  React.useEffect(() => {
    if (!isExpanded) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault();
        collapse();
      }
    };
    document.addEventListener('keydown', onKeyDown);
    const previousOverflow = document.body.style.overflow;
    document.body.style.overflow = 'hidden';
    panelRef.current?.focus();
    return () => {
      document.removeEventListener('keydown', onKeyDown);
      document.body.style.overflow = previousOverflow;
    };
  }, [isExpanded, collapse]);

  if (!isExpanded || !track) return null;

  // "End of chapter" / "12:30 left" — null when the timer is off.
  const sleepLabel = describeSleepTimer(sleepTimer);

  return (
    <div className="fixed inset-0 z-100 flex items-end justify-center sm:items-center sm:p-6">
      <button
        type="button"
        onClick={collapse}
        aria-label="Close player"
        className="absolute inset-0 cursor-default bg-black/70 backdrop-blur-sm"
        style={{ animation: 'fade-in 180ms var(--ease-out-quint)' }}
      />

      <div
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-label={`Now playing ${track.bookTitle}`}
        tabIndex={-1}
        className={cn(
          'relative flex max-h-dvh w-full max-w-2xl flex-col outline-none',
          'rounded-t-panel bg-canvas/95 shadow-[var(--shadow-lift)] sm:rounded-panel',
        )}
        style={{ animation: 'player-in 260ms var(--ease-out-quint)' }}
      >
        {/* Drag handle (visual affordance for the mobile sheet). */}
        <div className="mx-auto mt-3 h-1 w-10 shrink-0 rounded-full bg-hairline-strong sm:hidden" />

        <header className="flex items-center justify-between gap-3 px-4 pt-3 pb-2 sm:px-6">
          <TooltipWrap label="Minimise">
            <Button variant="ghost" size="icon" onClick={collapse} aria-label="Minimise player">
              <ChevronDown className="size-5" aria-hidden />
            </Button>
          </TooltipWrap>
          <p className="min-w-0 flex-1 truncate text-center text-xs font-medium tracking-wide text-ink-subtle uppercase">
            {track.author}
          </p>
          <TooltipWrap label={track.favorite ? 'Remove from favorites' : 'Add to favorites'}>
            <Button
              variant="ghost"
              size="icon"
              onClick={() => void setFavorite(!track.favorite)}
              aria-pressed={track.favorite}
              aria-label={track.favorite ? 'Remove from favorites' : 'Add to favorites'}
              className={cn(track.favorite && 'text-accent-soft')}
            >
              <Heart className={cn('size-5', track.favorite && 'fill-current')} aria-hidden />
            </Button>
          </TooltipWrap>
        </header>

        <div className="min-h-0 flex-1 overflow-y-auto px-4 pb-6 sm:px-8">
          <div className="flex flex-col items-center">
            <BookCover
              src={track.coverUrl}
              title={track.bookTitle}
              size="hero"
              emphasis
              className="mt-2 mb-6 shadow-[var(--shadow-lift)]"
            />

            <h2 className="text-center text-xl font-semibold text-ink sm:text-2xl">
              {track.bookTitle}
            </h2>
            {track.chapterNumber > 0 && (
              <p className="mt-2 text-center text-sm text-ink-muted">
                Chapter {track.chapterNumber} · {track.chapterTitle}
              </p>
            )}

            {status === 'error' && error && (
              <p role="alert" className="mt-4 text-center text-sm text-danger">
                {error}
              </p>
            )}

            <div className="mt-7 w-full">
              <Timeline />
            </div>

            {/* Transport */}
            <div className="mt-5 flex items-center gap-2 sm:gap-3">
              <TooltipWrap label={`Back ${skipBackwardSeconds} seconds`}>
                <Button
                  variant="ghost"
                  size="icon-lg"
                  onClick={skipBackward}
                  aria-label={`Back ${skipBackwardSeconds} seconds`}
                >
                  <RotateCcw className="size-6" aria-hidden />
                </Button>
              </TooltipWrap>

              <TooltipWrap label={hasPrevious ? 'Previous chapter' : undefined}>
                <Button
                  variant="ghost"
                  size="icon-lg"
                  onClick={() => void playPrevious()}
                  disabled={!hasPrevious}
                  aria-label="Previous chapter"
                >
                  <SkipBack className="size-6" aria-hidden />
                </Button>
              </TooltipWrap>

              <Button
                variant="primary"
                size="icon-lg"
                onClick={() => void toggle()}
                disabled={status === 'error'}
                aria-label={isPlaying ? 'Pause' : 'Play'}
                className="size-16 shadow-[0_10px_30px_-8px_var(--color-accent)]"
              >
                {isLoading ? (
                  <Loader2 className="size-7 animate-spin" aria-hidden />
                ) : isPlaying ? (
                  <Pause className="size-7 fill-current" aria-hidden />
                ) : (
                  <Play className="size-7 translate-x-0.5 fill-current" aria-hidden />
                )}
              </Button>

              <TooltipWrap label={hasNext ? 'Next chapter' : undefined}>
                <Button
                  variant="ghost"
                  size="icon-lg"
                  onClick={() => void playNext()}
                  disabled={!hasNext}
                  aria-label="Next chapter"
                >
                  <SkipForward className="size-6" aria-hidden />
                </Button>
              </TooltipWrap>

              <TooltipWrap label={`Forward ${skipForwardSeconds} seconds`}>
                <Button
                  variant="ghost"
                  size="icon-lg"
                  onClick={skipForward}
                  aria-label={`Forward ${skipForwardSeconds} seconds`}
                >
                  <RotateCw className="size-6" aria-hidden />
                </Button>
              </TooltipWrap>
            </div>

            {sleepLabel && (
              <p className="mt-4 flex items-center justify-center gap-1.5 text-xs text-ink-muted">
                <Moon className="size-3.5" aria-hidden />
                Sleep timer: {sleepLabel}
              </p>
            )}

            {/* Secondary row */}
            <div className="mt-6 flex w-full max-w-md items-center justify-center gap-2 sm:gap-3">
              <SpeedMenu variant="outline" />
              <SleepTimerMenu variant="outline" />
              <VolumeControl className="flex-1 justify-start" />
            </div>

            <BlockedAdvanceNotice />
          </div>
        </div>
      </div>
    </div>
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
      className="mt-6 w-full max-w-md rounded-card border border-caution/30 bg-caution/8 p-4 text-left"
    >
      <div className="flex items-start gap-3">
        <FileAudio className="mt-0.5 size-5 shrink-0 text-caution" aria-hidden />
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
                  new CustomEvent('voxleaf:import-audio', {
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
