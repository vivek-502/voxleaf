import * as React from 'react';
import { Link } from 'react-router-dom';
import { Heart } from 'lucide-react';

import { BookGrid } from '@/components/book/book-card';
import { PageHeader } from '@/components/common/section-header';
import { BookGridSkeleton, EmptyState, ErrorState } from '@/components/common/states';
import { Button } from '@/components/ui/button';
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
    } else {
      void playChapter(entry.bookId);
    }
  };

  return (
    <div className="container-page py-8 sm:py-10">
      <PageHeader
        title="Favorites"
        description="The books you have starred, for quick access."
      />

      {error && <ErrorState title="Could not load favorites" message={error} onRetry={reload} />}

      {loading && !data ? (
        <BookGridSkeleton count={12} />
      ) : favorites.length === 0 ? (
        <EmptyState
          icon={Heart}
          title="No favorites yet"
          description="Star a book from its page or the store and it will appear here."
          action={
            <Button asChild>
              <Link to="/store">Browse the store</Link>
            </Button>
          }
        />
      ) : (
        <BookGrid
          books={favorites.map((entry) => ({
            book: entry.book,
            library: entry,
            onPlay: entry.chaptersWithAudio > 0 ? () => resume(entry) : undefined}))}
        />
      )}
    </div>
  );
}
