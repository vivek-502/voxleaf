'use client';

import * as React from 'react';
import Link from 'next/link';
import { Bookmark, Pencil, Play, Trash2 } from 'lucide-react';

import { EmptyState, ErrorState } from '@/components/common/states';
import { Button } from '@/components/ui/button';
import { ConfirmDialog } from '@/components/common/confirm-dialog';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { SavedTabs } from './saved-tabs';
import { useApiQuery } from '@/hooks/use-async';
import { api, errorMessage } from '@/hooks/use-api';
import { toast } from 'sonner';
import { usePlayer } from '@/components/providers/player-provider';
import { formatDuration, formatRelativeTime, pluralize } from '@/utils/format';
import { PageHeader } from '@/components/common/section-header';
import type { BookmarkInfo } from '@/types';

/**
 * Every saved moment, newest first.
 *
 * Each entry leads with its timecode, the way a marginal note leads with a page
 * number, and offers a one-tap "play from here".
 */
export function BookmarksList() {
  const { playChapter } = usePlayer();
  const { data, error, loading, reload } = useApiQuery<BookmarkInfo[]>('/api/bookmarks');

  const [editing, setEditing] = React.useState<BookmarkInfo | null>(null);
  const [deleting, setDeleting] = React.useState<BookmarkInfo | null>(null);
  const [note, setNote] = React.useState('');
  const [busy, setBusy] = React.useState(false);

  const playFrom = (bookmark: BookmarkInfo) => {
    void playChapter(bookmark.chapterId, bookmark.bookId);
  };

  const saveNote = async () => {
    if (!editing) return;
    setBusy(true);
    try {
      await api.patch(`/api/bookmarks/${editing.id}`, { note: note.trim() || null });
      toast.success('Bookmark updated.');
      setEditing(null);
      reload();
    } catch (error) {
      toast.error(errorMessage(error, 'Could not update that bookmark.'));
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    if (!deleting) return;
    setBusy(true);
    try {
      await api.delete(`/api/bookmarks/${deleting.id}`);
      toast.success('Bookmark deleted.');
      setDeleting(null);
      reload();
    } catch (error) {
      toast.error(errorMessage(error, 'Could not delete that bookmark.'));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="container-page pb-16">
      <PageHeader
        className="mt-8 sm:mt-10"
        eyebrow="Marginalia"
        title="Saved"
        description="Moments you marked while listening, with any notes you left beside them."
      />
      <SavedTabs active="bookmarks" />

      <div className="mt-7 mb-4 flex items-center justify-between border-b border-hairline pb-2">
        <p className="eyebrow">
          {loading && !data ? 'Reading your notes…' : pluralize(data?.length ?? 0, 'bookmark')}
        </p>
        <Link
          href="/favorites"
          className="font-mono text-[10px] uppercase tracking-[0.09em] text-accent hover:text-accent-deep"
        >
          Starred books
        </Link>
      </div>

      {error && <ErrorState title="Could not load bookmarks" message={error} onRetry={reload} />}

      {loading && !data ? (
        <div className="flex flex-col">
          {Array.from({ length: 5 }, (_, index) => (
            <div key={index} className="skeleton h-24 border-b border-hairline" />
          ))}
        </div>
      ) : data && data.length === 0 ? (
        <EmptyState
          icon={Bookmark}
          title="No saved moments yet"
          description="While listening, press the bookmark button in the player to keep the current second, with an optional note."
        />
      ) : data ? (
        <ol className="border-t border-ink">
          {data.map((bookmark) => (
            <li key={bookmark.id} className="border-b border-hairline">
              <div className="grid gap-3 py-5 sm:grid-cols-[7.5rem_1fr_auto] sm:gap-6">
                <div>
                  <p className="font-mono text-[19px] leading-none tabular-nums text-ink">
                    {formatDuration(bookmark.timestamp)}
                  </p>
                  <p className="mt-1.5 font-mono text-[9.5px] uppercase tracking-[0.08em] text-ink-subtle">
                    {formatRelativeTime(bookmark.createdAt)}
                  </p>
                </div>

                <div className="min-w-0">
                  <Link
                    href={`/book/${bookmark.bookId}`}
                    className="font-display text-[19px] leading-tight font-medium text-ink hover:text-accent"
                  >
                    {bookmark.bookTitle}
                  </Link>
                  <p className="mt-1 font-mono text-[9.5px] uppercase tracking-[0.08em] text-ink-subtle">
                    {bookmark.chapterNumber > 0 ? `Chapter ${bookmark.chapterNumber}` : 'Chapter'}
                    {bookmark.chapterTitle && ` · ${bookmark.chapterTitle}`}
                  </p>
                  {bookmark.note && (
                    <p className="mt-3 max-w-2xl border-l-2 border-accent-line pl-3 font-display text-[15px] italic leading-relaxed text-ink-muted">
                      {bookmark.note}
                    </p>
                  )}
                </div>

                <div className="flex shrink-0 items-start gap-1 sm:items-center">
                  <Button
                    variant="subtle"
                    size="sm"
                    onClick={() => playFrom(bookmark)}
                    icon={<Play className="size-3.5 fill-current" aria-hidden />}
                  >
                    Play
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    onClick={() => {
                      setEditing(bookmark);
                      setNote(bookmark.note ?? '');
                    }}
                    aria-label={`Edit note for bookmark in ${bookmark.bookTitle}`}
                  >
                    <Pencil className="size-4" aria-hidden />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    onClick={() => setDeleting(bookmark)}
                    aria-label={`Delete bookmark in ${bookmark.bookTitle}`}
                  >
                    <Trash2 className="size-4 text-danger" aria-hidden />
                  </Button>
                </div>
              </div>
            </li>
          ))}
        </ol>
      ) : null}

      <Dialog
        open={Boolean(editing)}
        onOpenChange={(open) => {
          if (!open && !busy) setEditing(null);
        }}
      >
        <DialogContent className="max-w-md">
          <DialogHeader>
            <DialogTitle>Edit bookmark</DialogTitle>
            <DialogDescription>
              {editing?.bookTitle} · {editing ? formatDuration(editing.timestamp) : ''}
            </DialogDescription>
          </DialogHeader>
          <textarea
            value={note}
            onChange={(event) => setNote(event.target.value)}
            rows={4}
            maxLength={2000}
            autoFocus
            placeholder="Add a note about this moment…"
            aria-label="Bookmark note"
            className="w-full resize-y rounded-[3px] border border-hairline bg-surface px-3.5 py-3 text-[13.5px] leading-relaxed text-ink placeholder:text-ink-subtle transition-colors hover:border-hairline-strong focus:border-accent focus:bg-raised focus:outline-none"
          />
          <DialogFooter>
            <Button variant="ghost" onClick={() => setEditing(null)} disabled={busy}>
              Cancel
            </Button>
            <Button onClick={() => void saveNote()} loading={busy}>
              Save note
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <ConfirmDialog
        open={Boolean(deleting)}
        onOpenChange={(open) => {
          if (!open) setDeleting(null);
        }}
        title="Delete this bookmark?"
        description="The saved position and its note will be removed. This cannot be undone."
        confirmLabel="Delete bookmark"
        destructive
        onConfirm={remove}
      />
    </div>
  );
}
