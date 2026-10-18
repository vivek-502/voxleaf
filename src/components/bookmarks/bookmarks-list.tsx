import * as React from 'react';
import { Link } from 'react-router-dom';
import { Bookmark, Pencil, Play, Trash2 } from 'lucide-react';

import { EmptyState, ErrorState } from '@/components/common/states';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/primitives';
import { ConfirmDialog } from '@/components/common/confirm-dialog';
import { useApiQuery } from '@/hooks/use-async';
import { api, errorMessage } from '@/hooks/use-api';
import { toast } from 'sonner';
import { usePlayer } from '@/components/providers/player-provider';
import { formatDuration, formatRelativeTime } from '@/utils/format';
import { PageHeader } from '@/components/common/section-header';
import type { BookmarkInfo } from '@/types';

/**
 * Every saved moment, newest first.
 *
 * Each row links straight to the book and offers a one-tap "play from here",
 * which is the whole point of a bookmark: getting back to a specific second.
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
    <div className="container-page py-8 sm:py-10">
      <PageHeader
        title="Bookmarks"
        description="Moments you saved while listening. Tap play to jump straight back to that second."
      />

      {error && <ErrorState title="Could not load bookmarks" message={error} onRetry={reload} />}

      {loading && !data ? (
        <div className="flex flex-col gap-2">
          {Array.from({ length: 6 }, (_, index) => (
            <div key={index} className="skeleton h-20 rounded-card" />
          ))}
        </div>
      ) : data && data.length === 0 ? (
        <EmptyState
          icon={Bookmark}
          title="No bookmarks yet"
          description="While listening, press the bookmark button to save the current position with an optional note."
        />
      ) : data ? (
        <ul className="flex flex-col gap-2">
          {data.map((bookmark) => (
            <li
              key={bookmark.id}
              className="card-surface flex flex-col gap-3 p-4 sm:flex-row sm:items-center sm:gap-4"
            >
              <div className="min-w-0 flex-1">
                <div className="flex flex-wrap items-center gap-2">
                  <Link
                    to={`/book/${bookmark.bookId}`}
                    className="truncate text-sm font-semibold text-ink hover:text-accent-soft"
                  >
                    {bookmark.bookTitle}
                  </Link>
                  <Badge variant="accent">
                    {bookmark.chapterNumber > 0 ? `Ch. ${bookmark.chapterNumber}` : 'Chapter'} ·{' '}
                    {bookmark.chapterTitle}
                  </Badge>
                </div>

                <p className="mt-1 font-mono text-xs text-ink-subtle tabular-nums">
                  {formatDuration(bookmark.timestamp)}
                  <span className="ml-2 font-sans">· {formatRelativeTime(bookmark.createdAt)}</span>
                </p>

                {bookmark.note && (
                  <p className="mt-2 line-clamp-2-safe text-sm text-ink-muted">{bookmark.note}</p>
                )}
              </div>

              <div className="flex shrink-0 items-center gap-1">
                <Button
                  variant="subtle"
                  size="sm"
                  onClick={() => playFrom(bookmark)}
                  icon={<Play className="size-4 fill-current" aria-hidden />}
                >
                  Play from here
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
            </li>
          ))}
        </ul>
      ) : null}

      {/* Edit note dialog */}
      {editing && (
        <div
          role="dialog"
          aria-modal="true"
          aria-label="Edit bookmark note"
          className="fixed inset-0 z-100 flex items-center justify-center p-4"
        >
          <button
            type="button"
            aria-label="Close"
            className="absolute inset-0 cursor-default bg-black/70 backdrop-blur-sm"
            onClick={() => setEditing(null)}
          />
          <div className="card-surface relative w-full max-w-md p-6">
            <h2 className="text-lg font-semibold text-ink">Edit bookmark</h2>
            <p className="mt-1 text-sm text-ink-muted">
              {editing.bookTitle} · {formatDuration(editing.timestamp)}
            </p>
            <textarea
              value={note}
              onChange={(event) => setNote(event.target.value)}
              rows={4}
              maxLength={2000}
              autoFocus
              placeholder="Add a note about this moment…"
              className="mt-4 w-full resize-y rounded-xl border border-hairline bg-surface px-4 py-3 text-sm text-ink placeholder:text-ink-subtle focus:border-accent-line focus:outline-none"
            />
            <div className="mt-4 flex justify-end gap-2">
              <Button variant="ghost" onClick={() => setEditing(null)} disabled={busy}>
                Cancel
              </Button>
              <Button onClick={() => void saveNote()} loading={busy}>
                Save
              </Button>
            </div>
          </div>
        </div>
      )}

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
