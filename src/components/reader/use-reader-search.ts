'use client';

import * as React from 'react';

import { api, errorMessage } from '@/hooks/use-api';
import type { EpubSearchResponse, EpubSearchResult } from '@/types';

/**
 * Whole-book search.
 *
 * The server scans every section, so this hook only has to decide when to ask:
 * as the reader types, after a pause long enough to be one request rather than
 * one per keystroke, and again when the reading language changes so the results
 * describe the words actually on the page.
 */

export interface SearchOptions {
  caseSensitive: boolean;
  matchDiacritics: boolean;
  wholeWord: boolean;
}

export interface ReaderSearchState {
  query: string;
  setQuery: (query: string) => void;
  options: SearchOptions;
  toggleOption: (key: keyof SearchOptions) => void;
  results: EpubSearchResult[];
  total: number;
  truncated: boolean;
  chaptersScanned: number;
  loading: boolean;
  error: string | null;
  /** Run the current query now, without waiting for the debounce. */
  run: () => void;
  reset: () => void;
}

const DEBOUNCE_MS = 320;
const MIN_QUERY = 2;

export function useReaderSearch(
  bookId: string,
  language: string | null,
  enabled: boolean,
): ReaderSearchState {
  const [query, setQuery] = React.useState('');
  const [options, setOptions] = React.useState<SearchOptions>({
    caseSensitive: false,
    matchDiacritics: false,
    wholeWord: false,
  });
  const [results, setResults] = React.useState<EpubSearchResult[]>([]);
  const [total, setTotal] = React.useState(0);
  const [truncated, setTruncated] = React.useState(false);
  const [chaptersScanned, setChaptersScanned] = React.useState(0);
  const [loading, setLoading] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);

  const requestId = React.useRef(0);
  const trimmed = query.trim();

  const search = React.useCallback(
    async (text: string, lang: string | null, flags: SearchOptions) => {
      const id = requestId.current + 1;
      requestId.current = id;

      if (text.length < MIN_QUERY) {
        setResults([]);
        setTotal(0);
        setTruncated(false);
        setChaptersScanned(0);
        setError(null);
        setLoading(false);
        return;
      }

      setLoading(true);
      setError(null);
      try {
        const response = await api.get<EpubSearchResponse>(`/api/epub/${bookId}/search`, {
          q: text,
          language: lang ?? undefined,
          case: flags.caseSensitive ? '1' : undefined,
          diacritics: flags.matchDiacritics ? '1' : undefined,
          whole: flags.wholeWord ? '1' : undefined,
          limit: 120,
        });
        if (requestId.current !== id) return;
        setResults(response.results);
        setTotal(response.total);
        setTruncated(response.truncated);
        setChaptersScanned(response.chaptersScanned);
      } catch (err) {
        if (requestId.current !== id) return;
        setResults([]);
        setTotal(0);
        setError(errorMessage(err, 'Could not search this book.'));
      } finally {
        if (requestId.current === id) setLoading(false);
      }
    },
    [bookId],
  );

  React.useEffect(() => {
    if (!enabled) return;
    const timer = window.setTimeout(() => {
      void search(trimmed, language, options);
    }, DEBOUNCE_MS);
    return () => window.clearTimeout(timer);
  }, [enabled, language, options, search, trimmed]);

  const run = React.useCallback(() => {
    void search(trimmed, language, options);
  }, [language, options, search, trimmed]);

  const toggleOption = React.useCallback((key: keyof SearchOptions) => {
    setOptions((current) => ({ ...current, [key]: !current[key] }));
  }, []);

  const reset = React.useCallback(() => {
    requestId.current += 1;
    setQuery('');
    setResults([]);
    setTotal(0);
    setTruncated(false);
    setError(null);
    setLoading(false);
  }, []);

  return {
    query,
    setQuery,
    options,
    toggleOption,
    results,
    total,
    truncated,
    chaptersScanned,
    loading,
    error,
    run,
    reset,
  };
}
