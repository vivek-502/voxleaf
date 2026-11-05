'use client';

import * as React from 'react';
import Link from 'next/link';
import { useRouter } from 'next/navigation';
import { Heart } from 'lucide-react';

import { BookGrid } from '@/components/book/book-card';
import { PageHeader } from '@/components/common/section-header';
import { BookGridSkeleton, EmptyState, ErrorState } from '@/components/common/states';
import { Button } from '@/components/ui/button';
import { SavedTabs } from '@/components/bookmarks/saved-tabs';
import { useApiQuery } from '@/hooks/use-async';
import { usePlayer } from '@/components/providers/player-provider';
import type { LibraryBookInfo } from '@/types';

/** `/api/library` is paginated; this page shows the whole (single-user) library. */
interface LibraryPayload {
  books: LibraryBookInfo[];
  total: number;
}

/** Books the user has starred. */
export function FavoritesList() {
  const router = useRouter();
  const { playChapter } = usePlayer();
  const { data, error, loading, reload } = useApiQuery<LibraryPayload>('/api/library', {
    filter: 'all',
    pageSize: 100,
  });

  const favorites = React.useMemo(
    () => (data?.books ?? []).filter((entry) => entry.favorite),
    [data],
  );

  const resume = (entry: LibraryBookInfo) => {
    if (entry.lastPlayedChapterId) {
      void playChapter(entry.lastPlayedChapterId, entry.bookId);
      return;
    }
    router.push(`/book/${entry.bookId}`);
  };

  return (
    <div className="container-page pb-16">
      <PageHeader
        className="mt-8 sm:mt-10"
        eyebrow="Starred"
        title="Saved"
        description="The books you have starred, kept within reach."
      />
      <SavedTabs active="favorites" />

      <div className="mt-7 mb-4 flex items-center justify-between border-b border-hairline pb-2">
        <p className="eyebrow">
          {loading && !data ? 'Reading the shelf…' : `${favorites.length} starred`}
        </p>
        <Link
          href="/bookmarks"
          className="font-mono text-[10px] uppercase tracking-[0.09em] text-accent hover:text-accent-deep"
        >
          Saved moments
        </Link>
      </div>

      {error && <ErrorState title="Could not load favorites" message={error} onRetry={reload} />}

      {loading && !data ? (
        <BookGridSkeleton count={12} />
      ) : favorites.length === 0 ? (
        <EmptyState
          icon={Heart}
          title="No favorites yet"
          description="Star a book from its page or in the library and it will appear here."
          action={
            <Button asChild>
              <Link href="/library">Open the library</Link>
            </Button>
          }
        />
      ) : (
        <BookGrid
          books={favorites.map((entry) => ({
            book: entry.book,
            library: entry,
            onPlay: entry.lastPlayedChapterId ? () => resume(entry) : undefined,
          }))}
        />
      )}
    </div>
  );
}
