import * as React from 'react';

import { api } from '@/hooks/use-api';
import type { EpubTranslationSummary } from '@/types';

/**
 * Choosing the language narration should produce.
 *
 * A book can have translations saved into several languages, and which one the
 * reader wants spoken is a per-book decision, not a global preference: the same
 * book may be read in Hindi on a phone and English on a laptop. The choice is
 * remembered per book in localStorage, and defaults to the language most of the
 * book has been translated into — usually the one they actually read.
 *
 * Returns `null` for the original language, which is the correct answer for a
 * book with no translations at all and the only one offered in that case.
 */

/** Kept per book id so switching books does not drag the choice along. */
const STORAGE_PREFIX = 'voxleaf:narrate-language:';

export interface NarrationLanguageOption {
  /** Provider language code, or null for the original. */
  value: string | null;
  label: string;
  /** Sections saved in this language, for showing coverage. */
  sections: number;
}

export interface UseNarrationLanguage {
  /** The current choice, ready to pass to `generate`. */
  value: string | null;
  /** Name of the chosen language, or null for the original. */
  languageName: string | null;
  /** The original plus every saved translation. Empty while loading. */
  options: NarrationLanguageOption[];
  setLanguage: (value: string | null) => void;
  loading: boolean;
}

function storageKey(bookId: string): string {
  return `${STORAGE_PREFIX}${bookId}`;
}

/** Read the remembered choice, tolerating a disabled or absent store. */
function readRemembered(bookId: string): string | null | undefined {
  try {
    return window.localStorage.getItem(storageKey(bookId));
  } catch {
    return undefined;
  }
}

function writeRemembered(bookId: string, value: string | null): void {
  try {
    if (value === null) window.localStorage.removeItem(storageKey(bookId));
    else window.localStorage.setItem(storageKey(bookId), value);
  } catch {
    // A private-mode browser that refuses storage still gets a working choice,
    // it just will not be remembered next time.
  }
}

export function useNarrationLanguage(bookId: string): UseNarrationLanguage {
  const [languages, setLanguages] = React.useState<EpubTranslationSummary[]>([]);
  const [loading, setLoading] = React.useState(true);
  const [chosen, setChosen] = React.useState<string | null | undefined>(undefined);

  React.useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const result = await api.get<{ entries: EpubTranslationSummary[] }>(
          `/api/epub/${bookId}/translations`,
        );
        if (!cancelled) setLanguages(result.entries ?? []);
      } catch {
        if (!cancelled) setLanguages([]);
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [bookId]);

  React.useEffect(() => {
    setChosen(readRemembered(bookId) ?? null);
  }, [bookId]);

  const byLanguage = React.useMemo(() => {
    const counts = new Map<string, { name: string; sections: number }>();
    for (const entry of languages) {
      const existing = counts.get(entry.language);
      if (existing) existing.sections += 1;
      else counts.set(entry.language, { name: entry.languageName, sections: 1 });
    }
    return counts;
  }, [languages]);

  // Most-translated first, so the default is the language the reader has been
  // using rather than whichever happens to sort first by name.
  const options = React.useMemo<NarrationLanguageOption[]>(() => {
    const translated = [...byLanguage.entries()]
      .sort((a, b) => b[1].sections - a[1].sections || a[1].name.localeCompare(b[1].name))
      .map(([value, info]) => ({
        value,
        label: info.name,
        sections: info.sections,
      }));
    return [{ value: null, label: 'Original language', sections: 0 }, ...translated];
  }, [byLanguage]);

  const defaultLanguage = options.length > 1 ? (options[1]?.value ?? null) : null;

  // A remembered language the book no longer has a translation for is dropped,
  // so narration does not silently fall back to the original for a language the
  // reader never chose.
  const value =
    chosen !== null && chosen !== undefined && byLanguage.has(chosen) ? chosen : defaultLanguage;

  const setLanguage = React.useCallback(
    (next: string | null) => {
      setChosen(next);
      writeRemembered(bookId, next);
    },
    [bookId],
  );

  return {
    value,
    languageName: value === null ? null : (byLanguage.get(value)?.name ?? null),
    options,
    setLanguage,
    loading,
  };
}