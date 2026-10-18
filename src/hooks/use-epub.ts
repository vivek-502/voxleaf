import * as React from 'react';
import { toast } from 'sonner';

import { api, errorMessage } from '@/hooks/use-api';
import type { EpubStatusInfo } from '@/types';

/**
 * Client hook for a book's stored EPUB.
 *
 * The file only ever arrives by upload, so this hook is read-only apart from
 * removal: it reports what is on disk, and the panel drives the write paths
 * (upload, rebuild) directly.
 */

export interface UseEpubStatusResult {
  status: EpubStatusInfo | null;
  loading: boolean;
  busy: boolean;
  error: string | null;
  remove: () => Promise<void>;
  refresh: () => void;
}

export function useEpubStatus(bookId: string | null): UseEpubStatusResult {
  const [status, setStatus] = React.useState<EpubStatusInfo | null>(null);
  const [loading, setLoading] = React.useState(Boolean(bookId));
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const [tick, setTick] = React.useState(0);

  React.useEffect(() => {
    if (!bookId) {
      setStatus(null);
      setLoading(false);
      return;
    }
    let cancelled = false;

    const load = async () => {
      try {
        const next = await api.get<EpubStatusInfo>(`/api/books/${bookId}/epub`);
        if (!cancelled) {
          setStatus(next);
          setError(next.error ?? null);
        }
      } catch (err) {
        if (!cancelled) setError(errorMessage(err, 'Could not read the book file status.'));
      } finally {
        if (!cancelled) setLoading(false);
      }
    };

    void load();
    return () => {
      cancelled = true;
    };
  }, [bookId, tick]);

  const remove = async () => {
    if (!bookId) return;
    setBusy(true);
    try {
      await api.delete(`/api/books/${bookId}/epub`);
      setStatus(null);
      setError(null);
      setTick((n) => n + 1);
      toast.success('Book file removed.');
    } catch (err) {
      toast.error(errorMessage(err, 'Could not remove the book file.'));
    } finally {
      setBusy(false);
    }
  };

  return {
    status,
    loading,
    busy,
    error,
    remove,
    refresh: () => setTick((n) => n + 1),
  };
}
