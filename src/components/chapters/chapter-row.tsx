import * as React from 'react';
import { toast } from 'sonner';
import {
  Check,
  CircleAlert,
  Clock,
  FileAudio,
  GripVertical,
  Loader2,
  Pause,
  Pencil,
  Play,
  Sparkles,
  Trash2,
  UploadCloud,
} from 'lucide-react';

import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/primitives';
import { TooltipWrap } from '@/components/ui/tooltip';
import { ProgressBar } from '@/components/book/book-card';
import { ConfirmDialog } from '@/components/common/confirm-dialog';
import { api, errorMessage } from '@/hooks/use-api';
import { usePlayer } from '@/components/providers/player-provider';
import { formatBytes, formatDuration, formatPercent, formatRelativeTime, pluralize } from '@/utils/format';
import { cn } from '@/utils/cn';
import type { ChapterInfo } from '@/types';

export interface ChapterRowProps {
  chapter: ChapterInfo;
  bookId: string;
  /** Drag handle is only rendered when the list is reorderable. */
  sortable?: boolean;
  dragHandleProps?: React.HTMLAttributes<HTMLElement>;
  isDragging?: boolean;
  onUpload: (chapter: ChapterInfo) => void;
  onEdit: (chapter: ChapterInfo) => void;
  onGenerateTts?: (chapter: ChapterInfo) => void;
  /** This chapter is being narrated right now, so its button shows progress. */
  generatingTts?: boolean;
  onChanged?: () => void;
}

/**
 * One chapter in the book detail list.
 *
 * Every state is visible at a glance — has audio, is playing, is finished,
 * needs audio — because the whole point of this app is a hand-built chapter
 * list that is only partially filled in.
 */
export function ChapterRow({
  chapter,
  bookId,
  sortable = false,
  dragHandleProps,
  isDragging = false,
  onUpload,
  onEdit,
  onGenerateTts,
  generatingTts = false,
  onChanged,
}: ChapterRowProps) {
  const { track, isPlaying, playChapter, pause, toggle, seekTo } = usePlayer();

  const [confirmOpen, setConfirmOpen] = React.useState(false);
  const [removingAudio, setRemovingAudio] = React.useState(false);

  const isCurrent = track?.chapterId === chapter.id;
  const progress = chapter.progress;
  const completed = Boolean(progress?.completed);
  const partiallyPlayed = Boolean(progress && !completed && progress.position > 1);

  const play = async () => {
    if (isCurrent) {
      await toggle();
      return;
    }
    await playChapter(chapter.id, bookId);
  };

  const restart = () => {
    if (isCurrent) {
      seekTo(0);
      if (!isPlaying) void toggle();
    } else {
      void playChapter(chapter.id, bookId);
    }
  };

  const removeAudio = async () => {
    setRemovingAudio(true);
    try {
      await api.delete(`/api/chapters/${chapter.id}/audio`);
      toast.success(`Removed audio for “${chapter.title}”.`);
      onChanged?.();
    } catch (error) {
      toast.error(errorMessage(error, 'Could not remove the audio.'));
    } finally {
      setRemovingAudio(false);
    }
  };

  return (
    <>
      <li
        className={cn(
          'group card-surface relative flex items-center gap-3 p-3 transition-colors sm:gap-4 sm:p-3.5',
          isCurrent && 'border-accent-line bg-accent-wash/40',
          isDragging && 'opacity-50',
          'hover:border-hairline-strong',
        )}
      >
        {sortable && (
          <button
            type="button"
            aria-label={`Reorder ${chapter.title}`}
            className="cursor-grab touch-none text-ink-subtle transition-colors hover:text-ink active:cursor-grabbing"
            {...dragHandleProps}
          >
            <GripVertical className="size-5" aria-hidden />
          </button>
        )}

        {/*
          The lead control is whatever the reader would do next. With audio it is
          play; without, pressing the circle narrates the chapter instead — a
          dead circle that only reports "no audio" would make the row look
          broken, when in fact the missing audio is one press away.
        */}
        {chapter.audio ? (
          <button
            type="button"
            onClick={() => void play()}
            aria-label={isCurrent && isPlaying ? `Pause ${chapter.title}` : `Play ${chapter.title}`}
            className={cn(
              'grid size-10 shrink-0 place-items-center rounded-full transition-all duration-150',
              isCurrent
                ? 'bg-accent text-white'
                : 'bg-overlay text-ink hover:bg-accent hover:text-white',
            )}
          >
            {isCurrent && isPlaying ? (
              <Pause className="size-4 fill-current" aria-hidden />
            ) : (
              <Play className="size-4 translate-x-px fill-current" aria-hidden />
            )}
          </button>
        ) : onGenerateTts ? (
          <TooltipWrap label={generatingTts ? 'Generating audio…' : 'Narrate this chapter'}>
            <button
              type="button"
              onClick={() => onGenerateTts(chapter)}
              disabled={generatingTts}
              aria-label={`Narrate ${chapter.title} with text-to-speech`}
              className="grid size-10 shrink-0 place-items-center rounded-full border border-dashed border-hairline-strong text-ink-subtle transition-colors hover:border-accent-line hover:text-accent disabled:opacity-60"
            >
              {generatingTts ? (
                <Loader2 className="size-4 animate-spin" aria-hidden />
              ) : (
                <Sparkles className="size-4" aria-hidden />
              )}
            </button>
          </TooltipWrap>
        ) : (
          <div
            aria-label={`No audio for ${chapter.title}`}
            className="grid size-10 shrink-0 place-items-center rounded-full border border-dashed border-hairline-strong text-ink-subtle"
          >
            <CircleAlert className="size-4" aria-hidden />
          </div>
        )}

        {/* Chapter identity */}
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <span className="shrink-0 font-mono text-xs text-ink-subtle tabular-nums">
              {chapter.chapterNumber > 0 ? chapter.chapterNumber : '·'}
            </span>
            <h3
              className={cn(
                'min-w-0 flex-1 truncate text-sm font-medium',
                chapter.audio ? 'text-ink' : 'text-ink-muted',
              )}
            >
              {chapter.title}
            </h3>
            {isCurrent && (
              <span className="flex h-3.5 items-end gap-0.5" aria-label="Playing">
                {[0, 1, 2].map((index) => (
                  <span
                    key={index}
                    className="w-0.5 origin-bottom rounded-full bg-accent"
                    style={{
                      height: '100%',
                      animation: `equalize 900ms ease-in-out ${index * 160}ms infinite`,
                    }}
                  />
                ))}
              </span>
            )}
          </div>

          <div className="mt-1.5 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-ink-subtle">
            {chapter.audio ? (
              <>
                <span className="inline-flex items-center gap-1">
                  <FileAudio className="size-3" aria-hidden />
                  {formatDuration(chapter.audio.duration)}
                  {chapter.audio.fileSize > 0 && ` · ${formatBytes(chapter.audio.fileSize)}`}
                </span>
                {completed && (
                  <Badge variant="positive">
                    <Check className="size-3" aria-hidden />
                    Finished
                  </Badge>
                )}
                {!completed && partiallyPlayed && (
                  <span className="inline-flex items-center gap-1">
                    <Clock className="size-3" aria-hidden />
                    {formatPercent(progress?.percentage)}%
                  </span>
                )}
                {progress && progress.playCount > 1 && (
                  <span>{pluralize(progress.playCount, 'play')}</span>
                )}
              </>
            ) : (
              /* Says what to do, not just what is missing. */
              <Badge variant="caution">
                {onGenerateTts ? 'Not narrated yet' : 'No audio'}
              </Badge>
            )}
            {chapter.bookmarkCount > 0 && (
              <span>
                {pluralize(chapter.bookmarkCount, 'bookmark', 'bookmarks')}
              </span>
            )}
            {progress?.updatedAt && (
              <span className="hidden sm:inline">
                Last played {formatRelativeTime(progress.updatedAt)}
              </span>
            )}
          </div>

          {!completed && progress && progress.position > 1 && chapter.audio && (
            <div className="mt-2 max-w-md">
              <ProgressBar
                value={progress.percentage}
                label={`${chapter.title} progress`}
              />
            </div>
          )}
        </div>

        {/* Actions */}
        <div className="flex shrink-0 items-center gap-0.5 sm:gap-1">
          {chapter.audio && (
            <TooltipWrap label="Play from the start">
              <Button
                variant="ghost"
                size="icon-sm"
                onClick={restart}
                aria-label={`Play ${chapter.title} from the beginning`}
                className="hidden sm:inline-flex"
              >
                <Play className="size-4" aria-hidden />
              </Button>
            </TooltipWrap>
          )}

          {isCurrent && (
            <TooltipWrap label="Pause">
              <Button
                variant="ghost"
                size="icon-sm"
                onClick={pause}
                aria-label="Pause"
                className="sm:hidden"
              >
                <Pause className="size-4" aria-hidden />
              </Button>
            </TooltipWrap>
          )}

          <TooltipWrap label={chapter.audio ? 'Replace audio' : 'Import an audio file'}>
            <Button
              variant="ghost"
              size="icon-sm"
              onClick={() => onUpload(chapter)}
              aria-label={chapter.audio ? `Replace audio for ${chapter.title}` : `Import audio for ${chapter.title}`}
            >
              <UploadCloud className="size-4" aria-hidden />
            </Button>
          </TooltipWrap>

          {/*
            Narrating is the way most chapters get their audio, so it is a
            labelled button rather than a bare icon — the circle at the left is
            the shortcut, this is the one you can read.
          */}
          {!chapter.audio && onGenerateTts && (
            <TooltipWrap
              label={generatingTts ? 'Generating audio…' : 'Narrate with the voice set in Settings'}
            >
              <Button
                variant="subtle"
                size="sm"
                onClick={() => onGenerateTts(chapter)}
                disabled={generatingTts}
                icon={
                  generatingTts ? (
                    <Loader2 className="size-4 animate-spin" aria-hidden />
                  ) : (
                    <Sparkles className="size-4" aria-hidden />
                  )
                }
              >
                {generatingTts ? 'Narrating…' : 'Narrate'}
              </Button>
            </TooltipWrap>
          )}

          <TooltipWrap label="Rename chapter">
            <Button
              variant="ghost"
              size="icon-sm"
              onClick={() => onEdit(chapter)}
              aria-label={`Rename ${chapter.title}`}
            >
              <Pencil className="size-4" aria-hidden />
            </Button>
          </TooltipWrap>

          {chapter.audio ? (
            <TooltipWrap label="Remove audio">
              <Button
                variant="ghost"
                size="icon-sm"
                onClick={() => void removeAudio()}
                disabled={removingAudio}
                aria-label={`Remove audio for ${chapter.title}`}
              >
                <Trash2 className="size-4" aria-hidden />
              </Button>
            </TooltipWrap>
          ) : (
            <TooltipWrap label="Delete chapter">
              <Button
                variant="ghost"
                size="icon-sm"
                onClick={() => setConfirmOpen(true)}
                aria-label={`Delete ${chapter.title}`}
              >
                <Trash2 className="size-4 text-danger" aria-hidden />
              </Button>
            </TooltipWrap>
          )}
        </div>
      </li>

      <ConfirmDialog
        open={confirmOpen}
        onOpenChange={setConfirmOpen}
        title={`Delete “${chapter.title}”?`}
        description="This removes the chapter from this book. Any listening progress and bookmarks for it are deleted too. This cannot be undone."
        confirmLabel="Delete chapter"
        destructive
        onConfirm={async () => {
          try {
            await api.delete(`/api/chapters/${chapter.id}`);
            toast.success(`Deleted “${chapter.title}”.`);
            onChanged?.();
          } catch (error) {
            toast.error(errorMessage(error, 'Could not delete that chapter.'));
          }
        }}
      />
    </>
  );
}
