import * as React from 'react';

import { api, errorMessage } from '@/hooks/use-api';

/**
 * Minimal data-fetching hook.
 *
 * Deliberately small: the app reads from a local SQLite database, so there is
 * no cache to revalidate across users and no need for a query library. What
 * matters is that loading, error and retry states are explicit.
 *
 * `deps` re-runs the request when it changes, e.g. on page or query changes.
 * Results from a superseded request are discarded, so a slow response can
 * never overwrite a newer one.
 */
export function useAsync<T>(
  loader: (signal: AbortSignal) => Promise<T>,
  deps: React.DependencyList,
): {
  data: T | null;
  error: string | null;
  loading: boolean;
  reload: () => void;
} {
  const [data, setData] = React.useState<T | null>(null);
  const [error, setError] = React.useState<string | null>(null);
  const [loading, setLoading] = React.useState(true);
  const [token, setToken] = React.useState(0);

  // Keep the latest loader in a ref so changing it does not re-trigger by itself.
  const loaderRef = React.useRef(loader);
  loaderRef.current = loader;

  React.useEffect(() => {
    const controller = new AbortController();
    let cancelled = false;

    setLoading(true);
    setError(null);

    loaderRef
      .current(controller.signal)
      .then((result) => {
        if (cancelled) return;
        setData(result);
        setLoading(false);
      })
      .catch((caught) => {
        if (cancelled || controller.signal.aborted) return;
        setError(errorMessage(caught));
        setLoading(false);
      });

    return () => {
      cancelled = true;
      controller.abort();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [...deps, token]);

  const reload = React.useCallback(() => setToken((value) => value + 1), []);

  return { data, error, loading, reload };
}

/** Convenience wrapper for a plain GET against our own API. */
export function useApiQuery<T>(
  path: string,
  params?: Record<string, string | number | undefined | null>,
): {
  data: T | null;
  error: string | null;
  loading: boolean;
  reload: () => void;
} {
  const key = JSON.stringify(params ?? {});
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const loader = React.useCallback(() => api.get<T>(path, params), [path, key]);
  return useAsync(loader, [path, key]);
}
