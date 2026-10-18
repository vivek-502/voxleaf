import * as React from 'react';

import type { ThemeId } from '@/types';

interface ThemeContextValue {
  theme: ThemeId;
  /** The theme actually applied right now, after resolving `system`. */
  resolved: 'dark' | 'light';
  setTheme: (theme: ThemeId) => void;
}

const ThemeContext = React.createContext<ThemeContextValue | null>(null);

const STORAGE_KEY = 'voxleaf:theme';

function readStoredTheme(): ThemeId | null {
  if (typeof window === 'undefined') return null;
  const value = window.localStorage.getItem(STORAGE_KEY);
  return value === 'dark' || value === 'light' || value === 'system' ? value : null;
}

export function ThemeProvider({
  children,
  initialTheme = 'dark',
}: {
  children: React.ReactNode;
  initialTheme?: ThemeId;
}) {
  const [theme, setThemeState] = React.useState<ThemeId>(initialTheme);
  const [systemDark, setSystemDark] = React.useState(true);

  // Adopt the persisted choice on mount, before the first paint of any
  // user-navigated view.
  React.useEffect(() => {
    const stored = readStoredTheme();
    if (stored) setThemeState(stored);
  }, []);

  // Track the OS preference so `system` stays live.
  React.useEffect(() => {
    const query = window.matchMedia('(prefers-color-scheme: dark)');
    const update = () => setSystemDark(query.matches);
    update();
    query.addEventListener('change', update);
    return () => query.removeEventListener('change', update);
  }, []);

  const setTheme = React.useCallback((next: ThemeId) => {
    setThemeState(next);
    try {
      window.localStorage.setItem(STORAGE_KEY, next);
    } catch {
      // Private-mode storage failures are not worth surfacing.
    }
  }, []);

  const resolved: 'dark' | 'light' = theme === 'system' ? (systemDark ? 'dark' : 'light') : theme;

  const value = React.useMemo(() => ({ theme, resolved, setTheme }), [theme, resolved, setTheme]);

  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>;
}

export function useTheme(): ThemeContextValue {
  const context = React.useContext(ThemeContext);
  if (!context) throw new Error('useTheme must be used inside <ThemeProvider>');
  return context;
}
