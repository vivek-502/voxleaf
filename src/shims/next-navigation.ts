import * as React from 'react';
import {
  useLocation,
  useNavigate,
  useSearchParams as useRouterSearchParams,
} from 'react-router-dom';

/**
 * The slice of `next/navigation` the ported pages use, expressed on React
 * Router. Keeping the names identical means the shared components carry no
 * router-specific branches of their own.
 */
export function usePathname(): string {
  return useLocation().pathname;
}

export function useSearchParams(): URLSearchParams {
  const [params] = useRouterSearchParams();
  return params;
}

export function useRouter() {
  const navigate = useNavigate();
  return React.useMemo(
    () => ({
      push: (href: string) => navigate(href),
      replace: (href: string) => navigate(href, { replace: true }),
      back: () => navigate(-1),
      forward: () => navigate(1),
      refresh: () => undefined,
      prefetch: () => undefined,
    }),
    [navigate],
  );
}
