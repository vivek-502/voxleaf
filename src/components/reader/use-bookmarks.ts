'use client';

import * as React from 'react';
import { toast } from 'sonner';

import { api, errorMessage } from '@/hooks/use-api';
import type { EpubBookmarkInfo } from '@/types';

/**
 * Saved places, and the bookmark control.
 *
 * A bookmark is a section plus how far down it the reader was. That is
 * deliberately not a character offset: the words on the page can be translated
 * or reflowed, and the reader still expects the same spot in the chapter.
 */

export interface BookmarksState {
  bookmarks: EpubBookmarkInfo[];
  loading: boolean;
  refresh: () => Promise<void>;
  create: (chapterIndex: number, scrollPercent: number, label?: string | null) => Promise<EpubBookmarkInfo | null>;
  remove: (id: string) => Promise<void>;
}

export function useBookmarks(bookId: string, enabled: boolean): BookmarksState {
  const [bookmarks, setBookmarks] = React.useState<EpubBookmarkInfo[]>([]);
  const [loading, setLoading] = React.useState(false);

  const refresh = React.useCallback(async () => {
    if (!enabled) return;
    setLoading(true);
    try {
      const result = await api.get<{ bookmarks: EpubBookmarkInfo[] }>(
        `/api/epub/${bookId}/bookmarks`,
      );
      setBookmarks(result.bookmarks);
    } catch (error) {
      toast.error(errorMessage(error, 'Could not load your bookmarks.'));
    } finally {
      setLoading(false);
    }
  }, [bookId, enabled]);

  React.useEffect(() => {
    void refresh();
  }, [refresh]);

  const create = React.useCallback(
    async (chapterIndex: number, scrollPercent: number, label?: string | null) => {
      try {
        const result = await api.post<{ bookmark: EpubBookmarkInfo }>(
          `/api/epub/${bookId}/bookmarks`,
          { chapterIndex, scrollPercent, label: label ?? null },
        );
        setBookmarks((current) => [...current, result.bookmark]);
        toast.success('Bookmark saved.');
        return result.bookmark;
      } catch (error) {
        toast.error(errorMessage(error, 'Could not save that bookmark.'));
        return null;
      }
    },
    [bookId],
  );

  const remove = React.useCallback(
    async (id: string) => {
      let removed: EpubBookmarkInfo | undefined;
      setBookmarks((current) => {
        removed = current.find((bookmark) => bookmark.id === id);
        return current.filter((bookmark) => bookmark.id !== id);
      });
      try {
        await api.delete(`/api/epub/${bookId}/bookmarks/${id}`);
        toast.success('Bookmark removed.');
      } catch (error) {
        if (removed) {
          const restored = removed;
          setBookmarks((current) => [...current, restored]);
        }
        toast.error(errorMessage(error, 'Could not remove that bookmark.'));
      }
    },
    [bookId],
  );

  return { bookmarks, loading, refresh, create, remove };
}
