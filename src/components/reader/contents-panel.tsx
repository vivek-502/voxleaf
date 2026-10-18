import * as React from 'react';
import { AudioLines, Headphones, List, Loader2, Search, X } from 'lucide-react';

import type { ContentsRow } from './contents-rows';
import { filterContentsRows } from './contents-rows';
import { Button } from '@/components/ui/button';
import { cn } from '@/utils/cn';
import { formatDuration, formatPercent } from '@/utils/format';
import type { EpubAudioEntry } from '@/types';

export interface ContentsPanelProps {
  open: boolean;
  /**
   * True when there is room for the contents to sit beside the text.
   *
   * Docked it is a column in the reader's own layout: it stays where it is when
   * a row is chosen, because a panel that closes under you is not a side panel.
   * Undocked it is a drawer over the text, where covering the page is the point.
   */
  docked: boolean;
  onClose: () => void;
  rows: ContentsRow[];
  /** Section count, for the header's summary line. */
  chapterCount: number;
  activeKey: string | null;
  /** 0..1 progress through the whole book, for the rail. */
  overallProgress: number;
  onSelect: (row: ContentsRow) => void;
  bookTitle: string;
  author?: string;
  /** Audio state keyed by section index. */
  audioByIndex?: Map<number, EpubAudioEntry>;
  /** Section index currently playing, for the moving indicator. */
  playingIndex?: number | null;
  /** Section index with an upload dialog open. */
  preparingIndex?: number | null;
  /**
   * Play a section's audio, or start an import when it has none. One control
   * because the user is choosing between two things at the same time.
   */
  onPlayChapter?: (index: number) => void;
}

/**
 * The book's own table of contents, nested as the book nests it.
 *
 * Rows are the contents' rows, not the archive's documents: a book whose
 * contents point twice into one file, or once into a file that was split up, is
 * still navigable, and text the contents never mentions keeps a row so it does
 * not become unreachable.
 *
 * The reading theme owns its colour rather than the app's, so the panel belongs
 * to the page being read instead of to the chrome around it.
 */
export function ContentsPanel({
  open,
  docked,
  onClose,
  rows,
  chapterCount,
  activeKey,
  overallProgress,
  onSelect,
  bookTitle,
  author,
  audioByIndex,
  playingIndex = null,
  preparingIndex = null,
  onPlayChapter,
}: ContentsPanelProps) {
  const listRef = React.useRef<HTMLUListElement>(null);
  const activeRef = React.useRef<HTMLButtonElement>(null);
  const [query, setQuery] = React.useState('');

  const visible = React.useMemo(() => filterContentsRows(rows, query), [rows, query]);
  const withAudio = React.useMemo(
    () => rows.filter((row) => row.leads && audioByIndex?.get(row.index)?.hasAudio).length,
    [audioByIndex, rows],
  );

  // Follow the reader, but only when the panel is on screen to follow them. The
  // scroll handler only calls this when the active row has actually changed, so
  // this runs on navigation rather than on every frame.
  React.useEffect(() => {
    if (!open) return;
    const frame = requestAnimationFrame(() => {
      activeRef.current?.scrollIntoView({ block: 'nearest' });
    });
    return () => cancelAnimationFrame(frame);
  }, [activeKey, open]);

  // Escape closes it. A docked panel is not modal over anything, so focus stays
  // where the reader put it — stealing it would break type-to-find mid-page.
  React.useEffect(() => {
    if (!open) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      event.preventDefault();
      onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose, open]);

  // A new search should not leave a stale highlight hiding the matches.
  React.useEffect(() => {
    if (open) setQuery('');
  }, [open]);

  if (!open) return null;

  const panel = (
    <aside
      aria-label="Contents"
      style={{
        background: 'var(--reader-chrome)',
        backdropFilter: 'blur(18px) saturate(140%)',
        WebkitBackdropFilter: 'blur(18px) saturate(140%)',
        color: 'var(--reader-fg)',
      }}
      className={cn(
        docked
          ? 'relative z-20 flex h-full w-[min(21rem,32vw)] shrink-0 flex-col max-[900px]:hidden'
          : 'relative flex h-full w-[min(21rem,86vw)] flex-col shadow-[0_18px_40px_-18px_rgb(0_0_0/0.6)]',
        docked
          ? 'animate-[contents-fade_200ms_var(--ease-out-quint)]'
          : 'animate-[contents-slide_220ms_var(--ease-out-quint)]',
      )}
    >
      <header
        className="shrink-0 px-5 pt-5 pb-3"
        style={{ borderBottom: '1px solid var(--reader-rule)' }}
      >
        <div className="flex items-start justify-between gap-2">
          <div className="min-w-0">
            <h2
              className="text-[0.7rem] font-bold tracking-[0.09em] uppercase"
              style={{ color: 'var(--reader-muted)' }}
            >
              Contents
            </h2>
            <p className="mt-1 truncate text-sm font-semibold">{bookTitle}</p>
            {author && (
              <p className="truncate text-xs" style={{ color: 'var(--reader-muted)' }}>
                {author}
              </p>
            )}
          </div>
          <Button
            variant="ghost"
            size="icon-sm"
            onClick={onClose}
            aria-label="Hide contents"
            className="shrink-0"
            style={{ color: 'var(--reader-muted)' }}
          >
            <X className="size-4" aria-hidden />
          </Button>
        </div>

        <p
          className="mt-2.5 text-[0.7rem] tabular-nums"
          style={{ color: 'var(--reader-muted)' }}
        >
          {rows.length} {rows.length === 1 ? 'entry' : 'entries'} ·{' '}
          {chapterCount} {chapterCount === 1 ? 'section' : 'sections'}
          {withAudio > 0 && ` · ${withAudio} with audio`}
          {overallProgress > 0 && ` · ${formatPercent(overallProgress)}% read`}
        </p>

        {rows.length > 12 && (
          <div
            className="mt-3 flex items-center gap-2 rounded-card px-2.5 py-1.5"
            style={{ background: 'color-mix(in srgb, var(--reader-fg) 6%, transparent)' }}
          >
            <Search className="size-3.5 shrink-0" style={{ color: 'var(--reader-muted)' }} aria-hidden />
            <input
              type="search"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder="Find a section"
              aria-label="Find a section"
              className="min-w-0 flex-1 bg-transparent text-sm outline-none placeholder:opacity-60"
            />
            {query && (
              <button
                type="button"
                onClick={() => setQuery('')}
                aria-label="Clear search"
                className="shrink-0 rounded-full p-0.5 hover:bg-black/10 dark:hover:bg-white/10"
                style={{ color: 'var(--reader-muted)' }}
              >
                <X className="size-3.5" aria-hidden />
              </button>
            )}
          </div>
        )}
      </header>

      <div className="h-px w-full shrink-0" style={{ background: 'var(--reader-rule)' }} aria-hidden>
        <div
          className="h-full transition-[width] duration-300"
          style={{ width: `${Math.round(overallProgress * 100)}%`, background: 'var(--reader-link)' }}
        />
      </div>

      {visible.length === 0 ? (
        <p
          className="px-5 py-8 text-center text-sm"
          style={{ color: 'var(--reader-muted)' }}
        >
          Nothing in this book is called “{query.trim()}”.
        </p>
      ) : (
        <ul
          ref={listRef}
          tabIndex={-1}
          className="min-h-0 flex-1 overflow-y-auto overscroll-contain py-1.5 focus:outline-none"
        >
          {visible.map((row) => {
            const active = row.key === activeKey;
            const audio = audioByIndex?.get(row.index);
            const isPlaying = playingIndex === row.index;
            const isPreparing = preparingIndex === row.index;

            return (
              <li key={row.key} className="group relative">
                {/* A rail down the nested levels, so depth reads as structure
                    rather than only as an indent. */}
                {row.depth > 0 && (
                  <span
                    aria-hidden
                    className="pointer-events-none absolute inset-y-0 w-px"
                    style={{
                      left: `${0.75 + (row.depth - 1) * 0.9}rem`,
                      background: 'var(--reader-rule)',
                    }}
                  />
                )}

                <div className="flex items-stretch">
                  <button
                    ref={active ? activeRef : undefined}
                    type="button"
                    onClick={() => onSelect(row)}
                    aria-current={active ? 'true' : undefined}
                    style={{ paddingInlineStart: `${0.75 + row.depth * 0.9}rem` }}
                    className="relative flex min-w-0 flex-1 items-center py-2 pr-2 text-left transition-colors"
                  >
                    <span
                      aria-hidden
                      className={cn(
                        'absolute inset-y-1 left-0 w-[2px] rounded-full transition-colors',
                        active ? 'opacity-100' : 'opacity-0 group-hover:opacity-40',
                      )}
                      style={{ background: 'var(--reader-link)' }}
                    />

                    <span
                      className="min-w-0 flex-1"
                      style={{
                        background: active
                          ? 'color-mix(in srgb, var(--reader-link) 12%, transparent)'
                          : undefined,
                        borderRadius: 'var(--radius-sm)',
                        padding: '0.3rem 0.5rem',
                      }}
                    >
                      <span
                        className={cn(
                          'block truncate leading-snug',
                          row.depth >= 2 ? 'text-[13px]' : 'text-sm',
                          active ? 'font-semibold' : 'font-normal',
                        )}
                        style={{ color: active ? 'var(--reader-link)' : 'var(--reader-fg)' }}
                      >
                        {row.title}
                      </span>
                      {row.leads && audio?.hasAudio && audio.duration > 0 && (
                        <span
                          className="mt-0.5 block text-[11px] tabular-nums"
                          style={{ color: 'var(--reader-muted)' }}
                        >
                          {formatDuration(audio.duration)}
                          {audio.progress > 0.005 && !audio.completed
                            ? ` · ${formatPercent(audio.progress)}%`
                            : ''}
                        </span>
                      )}
                    </span>
                  </button>

                  {onPlayChapter && row.leads && (
                    <button
                      type="button"
                      onClick={() => onPlayChapter(row.index)}
                      disabled={isPreparing}
                      aria-label={
                        isPreparing
                          ? `Preparing audio for ${row.title}`
                          : audio?.hasAudio
                            ? `Play audio for ${row.title}`
                            : `Import audio for ${row.title}`
                      }
                      title={audio?.hasAudio ? 'Play section audio' : 'Import audio for this section'}
                      className={cn(
                        'grid w-11 shrink-0 place-items-center transition-colors disabled:opacity-50',
                        audio?.hasAudio ? 'opacity-100' : 'opacity-45 hover:opacity-90',
                      )}
                      style={{ borderLeft: '1px solid var(--reader-rule)', color: 'var(--reader-link)' }}
                    >
                      {isPreparing ? (
                        <Loader2 className="size-4 animate-spin" aria-hidden />
                      ) : isPlaying ? (
                        <AudioLines className="size-4" aria-hidden />
                      ) : audio?.hasAudio ? (
                        <Headphones className="size-4" aria-hidden />
                      ) : (
                        <List className="size-4" aria-hidden />
                      )}
                    </button>
                  )}
                </div>
              </li>
            );
          })}
        </ul>
      )}
    </aside>
  );

  if (docked) return panel;

  return (
    <div className="fixed inset-0 z-100 flex" role="dialog" aria-modal="true" aria-label="Contents">
      <button
        type="button"
        aria-label="Close contents"
        tabIndex={-1}
        onClick={onClose}
        className="animate-[scrim-enter_160ms_ease-out] absolute inset-0 bg-black/50"
      />
      {panel}
    </div>
  );
}