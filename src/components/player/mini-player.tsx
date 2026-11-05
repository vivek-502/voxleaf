'use client';

import * as React from 'react';
import Link from 'next/link';
import {
  ChevronUp,
  Heart,
  Loader2,
  Pause,
  Play,
  RotateCcw,
  RotateCw,
  SkipForward,
  X,
} from 'lucide-react';

import { Timeline } from './timeline';
import { BookCover } from '@/components/book/book-cover';
import { Button } from '@/components/ui/button';
import { TooltipWrap } from '@/components/ui/tooltip';
import { usePlayer } from '@/components/providers/player-provider';
import { useSettings } from '@/components/providers/settings-provider';
import { cn } from '@/utils/cn';

/**
 * Persistent bottom player bar.
 *
 * Rendered by the app shell for the whole session, so playback survives
 * navigation. Renders nothing when there is no track — the shell drops its
 * reserved height at the same time.
 *
 * It is *not* `fixed` itself: it is the first row of the shell's bottom stack,
 * so the mobile nav can never end up underneath it.
 */
export function MiniPlayer() {
  const {
    track,
    isPlaying,
    isLoading,
    status,
    toggle,
    skipForward,
    skipBackward,
    hasNext,
    hasPrevious,
    playNext,
    playPrevious,
    isExpanded,
    toggleExpand,
    collapse,
    setFavorite,
    stop,
    error,
    position,
    duration,
  } = usePlayer();
  const { skipForwardSeconds, skipBackwardSeconds } = useSettings();

  if (!track || isExpanded) return null;

  const progress = duration > 0 ? Math.min(100, (position / duration) * 100) : 0;

  return (
    <div
      className="relative border-t border-ink bg-surface/97 backdrop-blur-md"
      role="region"
      aria-label="Now playing"
    >
      {/* A hairline of progress across the very top of the bar, legible at a glance. */}
      <span aria-hidden className="absolute inset-x-0 -top-px h-[2px] bg-hairline">
        <span
          className="block h-full bg-accent transition-[width] duration-200"
          style={{ width: `${progress}%` }}
        />
      </span>

      {status === 'error' && error && (
        <p className="flex items-center justify-center gap-2 border-b border-danger/25 bg-danger/10 px-4 py-1.5 text-center text-xs text-danger">
          <span className="truncate">{error}</span>
        </p>
      )}

      <div className="container-page flex h-[72px] items-center gap-2 sm:gap-3">
        <div className="flex min-w-0 flex-1 items-center gap-2 sm:flex-none sm:basis-[260px]">
          <BookCover
            src={track.coverUrl}
            title={track.bookTitle}
            size="sm"
            className="h-11 w-11 shrink-0 sm:h-12 sm:w-12"
          />

          <Link
            href={`/book/${track.bookId}`}
            onClick={collapse}
            className="min-w-0 flex-1 py-1 outline-none focus-visible:underline"
            aria-label={`Go to ${track.bookTitle}`}
          >
            <span className="block truncate text-[13px] font-medium leading-tight text-ink sm:text-sm">
              {track.bookTitle}
            </span>
            <span className="mt-0.5 block truncate font-mono text-[10px] uppercase tracking-[0.04em] text-ink-subtle sm:text-[10.5px]">
              {track.chapterNumber > 0 ? `Ch. ${track.chapterNumber} · ` : ''}
              {track.chapterTitle}
            </span>
          </Link>

          <TooltipWrap label={isExpanded ? 'Minimise' : 'Open player'}>
            <Button
              variant="ghost"
              size="icon-sm"
              onClick={toggleExpand}
              aria-label="Open the full player"
            >
              <ChevronUp className="size-[18px]" aria-hidden />
            </Button>
          </TooltipWrap>
        </div>

        <div className="flex shrink-0 items-center gap-0.5">
          <TooltipWrap label={`Back ${skipBackwardSeconds}s`}>
            <Button
              variant="ghost"
              size="icon-sm"
              onClick={skipBackward}
              aria-label={`Back ${skipBackwardSeconds} seconds`}
              className="hidden sm:inline-flex"
            >
              <RotateCcw className="size-[18px]" aria-hidden />
            </Button>
          </TooltipWrap>

          <TooltipWrap label={hasPrevious ? 'Previous chapter' : undefined}>
            <Button
              variant="ghost"
              size="icon-sm"
              onClick={() => void playPrevious()}
              disabled={!hasPrevious}
              aria-label="Previous chapter"
              className="hidden sm:inline-flex"
            >
              <SkipForward className="size-[18px] rotate-180" aria-hidden />
            </Button>
          </TooltipWrap>

          <Button
            variant="primary"
            size="icon"
            onClick={() => void toggle()}
            disabled={status === 'error'}
            aria-label={isPlaying ? 'Pause' : 'Play'}
            className="mx-1"
          >
            {isLoading ? (
              <Loader2 className="size-4 animate-spin" aria-hidden />
            ) : isPlaying ? (
              <Pause className="size-4 fill-current" aria-hidden />
            ) : (
              <Play className="size-4 translate-x-px fill-current" aria-hidden />
            )}
          </Button>

          <TooltipWrap label={hasNext ? 'Next chapter' : undefined}>
            <Button
              variant="ghost"
              size="icon-sm"
              onClick={() => void playNext()}
              disabled={!hasNext}
              aria-label="Next chapter"
              className="hidden sm:inline-flex"
            >
              <SkipForward className="size-[18px]" aria-hidden />
            </Button>
          </TooltipWrap>

          <TooltipWrap label={`Forward ${skipForwardSeconds}s`}>
            <Button
              variant="ghost"
              size="icon-sm"
              onClick={skipForward}
              aria-label={`Forward ${skipForwardSeconds} seconds`}
              className="hidden md:inline-flex"
            >
              <RotateCw className="size-[18px]" aria-hidden />
            </Button>
          </TooltipWrap>
        </div>

        <button
          type="button"
          onClick={toggleExpand}
          aria-label="Open the full player"
          className="hidden min-w-0 flex-1 cursor-pointer px-2 py-1 outline-none focus-visible:underline lg:flex"
        >
          <Timeline showLabels />
        </button>

        <div className="hidden shrink-0 items-center gap-0.5 lg:flex">
          <TooltipWrap label={track.favorite ? 'Remove from favorites' : 'Add to favorites'}>
            <Button
              variant="ghost"
              size="icon"
              onClick={() => void setFavorite(!track.favorite)}
              aria-pressed={track.favorite}
              aria-label={track.favorite ? 'Remove from favorites' : 'Add to favorites'}
              className={cn(track.favorite && 'text-accent')}
            >
              <Heart className={cn('size-[18px]', track.favorite && 'fill-current')} aria-hidden />
            </Button>
          </TooltipWrap>

          <TooltipWrap label="Stop playback">
            <Button variant="ghost" size="icon" onClick={stop} aria-label="Stop playback">
              <X className="size-[18px]" aria-hidden />
            </Button>
          </TooltipWrap>
        </div>
      </div>
    </div>
  );
}
