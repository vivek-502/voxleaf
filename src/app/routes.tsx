import * as React from 'react';
import { Navigate, Route, Routes, useParams } from 'react-router-dom';

import { HomeDashboard } from '@/components/home/home-dashboard';
import { StoreBrowser } from '@/components/store/store-browser';
import { LibraryBrowser } from '@/components/library/library-browser';
import { ContinueListening } from '@/components/continue/continue-listening';
import { FavoritesList } from '@/components/favorites/favorites-list';
import { BookmarksList } from '@/components/bookmarks/bookmarks-list';
import { SettingsPage } from '@/components/settings/settings-page';
import { BookDetail } from '@/components/book/book-detail';
import { EpubReader } from '@/components/reader/epub-reader';
import {
  BookGridSkeleton,
  BookRowSkeleton,
  PageSkeleton,
} from '@/components/common/states';
import { useApiQuery } from '@/hooks/use-async';

import type { BookSummary } from '@/types';

/**
 * The reader route.
 *
 * The reader paints itself as a `fixed inset-0` layer above the app shell, so
 * it stays inside the normal layout tree rather than getting a route of its
 * own: a second layout would mean a second copy of the providers, and audio
 * would stop the moment the reader navigated anywhere.
 */
function ReaderRoute() {
  const params = useParams<{ id: string }>();
  const bookId = params.id ?? '';

  // The catalogue title is only a placeholder — the EPUB's own metadata takes
  // over once the book loads. Fetching it keeps the header from ever being
  // empty, which is what the web app got from its server-rendered title.
  const { data } = useApiQuery<{ book?: BookSummary }>(`/books/${bookId}`);

  return (
    <React.Suspense fallback={null}>
      <EpubReader
        bookId={bookId}
        bookTitle={data?.book?.title ?? 'Reading'}
        authorName={data?.book?.author ?? ''}
      />
    </React.Suspense>
  );
}

function BookRoute() {
  const params = useParams<{ id: string }>();
  return (
    <React.Suspense fallback={<PageSkeleton />}>
      <BookDetail bookId={params.id ?? ''} />
    </React.Suspense>
  );
}

/**
 * Route table.
 *
 * Mirrors the web app's pages one-for-one, keeping each page's skeleton in the
 * Suspense fallback so a navigation never flashes an empty frame.
 */
export function AppRoutes() {
  return (
    <Routes>
      <Route path="/" element={<HomeDashboard />} />
      <Route
        path="/store"
        element={
          <React.Suspense fallback={<BookGridSkeleton count={12} />}>
            <StoreBrowser />
          </React.Suspense>
        }
      />
      <Route
        path="/library"
        element={
          <React.Suspense fallback={<BookGridSkeleton count={12} />}>
            <LibraryBrowser />
          </React.Suspense>
        }
      />
      <Route
        path="/continue"
        element={
          <React.Suspense fallback={<BookRowSkeleton count={6} />}>
            <ContinueListening />
          </React.Suspense>
        }
      />
      <Route
        path="/favorites"
        element={
          <React.Suspense fallback={<BookGridSkeleton count={12} />}>
            <FavoritesList />
          </React.Suspense>
        }
      />
      <Route
        path="/bookmarks"
        element={
          <React.Suspense fallback={<BookRowSkeleton count={6} />}>
            <BookmarksList />
          </React.Suspense>
        }
      />
      <Route
        path="/settings"
        element={
          <React.Suspense fallback={<PageSkeleton />}>
            <SettingsPage />
          </React.Suspense>
        }
      />
      <Route path="/book/:id" element={<BookRoute />} />
      <Route path="/read/:id" element={<ReaderRoute />} />
      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
  );
}
