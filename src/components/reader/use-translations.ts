import * as React from 'react';
import { toast } from 'sonner';

import { api, errorMessage } from '@/hooks/use-api';
import { translationLanguageName } from '@/utils/constants';
import type { EpubTranslationEntry, EpubTranslationSummary } from '@/types';

/**
 * Reading a book in another language, inside the reader.
 *
 * The chosen language belongs to the book rather than to the section: a reader
 * who translates one chapter into Spanish and then reads on expects the next
 * chapter in Spanish too, and having to choose again per chapter would make the
 * feature something they use once. So the language is remembered per book, and
 * turning to a section that has not been translated shows the original with an
 * offer rather than silently switching back.
 *
 * What is *not* remembered is the text: each section's translation is fetched
 * when it is displayed, because a book's worth of translations is far too much to
 * hold in memory for a book nobody is reading in that language.
 */

export interface TranslationsState {
  /** Saved translations of the section being read. */
  saved: EpubTranslationSummary[];
  /** The translation on screen, or null when the original is being read. */
  entry: EpubTranslationEntry | null;
  /** The book-wide language, or null for the original. */
  language: string | null;
  /** Name of the book-wide language, for labels. */
  languageName: string | null;
  /** Whether the reader is looking at translated text rather than the original. */
  showing: boolean;
  /** The language is chosen but this section has no translation of it yet. */
  missingHere: boolean;
  /** Languages the book has any translations in, with how many sections. */
  bookLanguages: { language: string; languageName: string; sections: number }[];
  /** Characters translated so far, while a job runs. */
  progress: { done: number; total: number } | null;
  /** A translation job is running. */
  busy: boolean;
  /** The dialog is open. */
  open: boolean;
  setOpen: (open: boolean) => void;
  /** Read the original text, whatever the chosen language. */
  showOriginal: () => void;
  /** Read the saved translation of `language` for the current section. */
  showTranslation: (language: string) => void;
  /** Translate the current section, then show it. */
  translate: (language: string, options?: { force?: boolean }) => Promise<boolean>;
  /** Forget a saved translation. */
  remove: (language: string) => Promise<void>;
}

const STORAGE_PREFIX = 'voxleaf:translation:';

export function useTranslations(bookId: string, chapterIndex: number, enabled: boolean): TranslationsState {
  const [saved, setSaved] = React.useState<EpubTranslationSummary[]>([]);
  const [bookLanguages, setBookLanguages] = React.useState<
    { language: string; languageName: string; sections: number }[]
  >([]);
  const [entry, setEntry] = React.useState<EpubTranslationEntry | null>(null);
  const [language, setLanguage] = React.useState<string | null>(null);
  const [progress, setProgress] = React.useState<{ done: number; total: number } | null>(null);
  const [busy, setBusy] = React.useState(false);
  const [open, setOpen] = React.useState(false);

  const abortRef = React.useRef<AbortController | null>(null);
  const storageKey = `${STORAGE_PREFIX}${bookId}`;

  // The remembered language is read once per book, before the first section is
  // shown, so a reader who left the book in Spanish finds it in Spanish.
  const restoredRef = React.useRef(false);
  React.useEffect(() => {
    if (restoredRef.current) return;
    restoredRef.current = true;
    try {
      const stored = window.localStorage.getItem(storageKey);
      if (stored && stored !== 'original') setLanguage(stored);
      else if (stored === 'original') setLanguage(null);
    } catch {
      /* private browsing, or no storage: the reader's choice just will not stick */
    }
  }, [storageKey]);

  const remember = React.useCallback(
    (next: string | null) => {
      try {
        if (next) window.localStorage.setItem(storageKey, next);
        else window.localStorage.setItem(storageKey, 'original');
      } catch {
        /* not being able to remember is not worth interrupting reading for */
      }
    },
    [storageKey],
  );

  /** Every saved translation of the book, which is also what the picker offers. */
  const refresh = React.useCallback(async () => {
    if (!enabled) return;
    try {
      const [section, all] = await Promise.all([
        api.get<{ entries: EpubTranslationSummary[] }>(`/api/epub/${bookId}/translations`, { index: chapterIndex }),
        api.get<{ entries: EpubTranslationSummary[] }>(`/api/epub/${bookId}/translations`),
      ]);
      setSaved(section.entries);

      const counts = new Map<string, number>();
      for (const item of all.entries) counts.set(item.language, (counts.get(item.language) ?? 0) + 1);
      setBookLanguages(
        [...counts.entries()]
          .map(([code, sections]) => ({
            language: code,
            languageName: all.entries.find((item) => item.language === code)?.languageName ?? code,
            sections,
          }))
          .sort((a, b) => b.sections - a.sections || a.languageName.localeCompare(b.languageName)),
      );
    } catch {
      /* the picker simply offers nothing; the original text still reads */
    }
  }, [bookId, chapterIndex, enabled]);

  React.useEffect(() => {
    void refresh();
  }, [refresh]);

  // Load the text of the chosen language for this section.
  React.useEffect(() => {
    if (!enabled || !language) {
      setEntry(null);
      return;
    }
    let cancelled = false;
    void (async () => {
      try {
        const found = await api.get<EpubTranslationEntry>(
          `/api/epub/${bookId}/translations/${chapterIndex}`,
          { language },
        );
        if (!cancelled) setEntry(found);
      } catch {
        if (!cancelled) setEntry(null);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [bookId, chapterIndex, enabled, language]);

  const showTranslation = React.useCallback((code: string) => {
    setLanguage(code);
    remember(code);
    // Whichever way a language was asked for — a saved chip, the button at the
    // foot of the dialog, or a translation that has just been made — the reader
    // wants to read it, so the picker steps aside instead of leaving the text
    // behind a modal they have to dismiss first.
    setOpen(false);
  }, [remember]);

  const showOriginal = React.useCallback(() => {
    setLanguage(null);
    remember(null);
  }, [remember]);

  const translate = React.useCallback(
    async (code: string, options: { force?: boolean } = {}) => {
      abortRef.current?.abort();
      const controller = new AbortController();
      abortRef.current = controller;

      setBusy(true);
      setProgress({ done: 0, total: 0 });
      try {
        const savedEntry = await api.stream<EpubTranslationEntry>(
          `/api/epub/${bookId}/translations/${chapterIndex}`,
          { language: code, force: options.force === true },
          (update) => setProgress(update),
          controller.signal,
        );
        setEntry(savedEntry);
        showTranslation(code);
        await refresh();
        toast.success(
          `Translated into ${savedEntry.languageName}. It is saved, so this section will read in ${savedEntry.languageName} from now on.`,
        );
        return true;
      } catch (error) {
        if ((error as Error).name === 'AbortError' || (error as { code?: string })?.code === 'aborted') return false;
        toast.error(errorMessage(error, 'The translation did not finish. Nothing was saved.'));
        return false;
      } finally {
        setBusy(false);
        setProgress(null);
        if (abortRef.current === controller) abortRef.current = null;
      }
    },
    [bookId, chapterIndex, refresh, showTranslation],
  );

  const remove = React.useCallback(
    async (code: string) => {
      try {
        await api.delete<{ removed: boolean }>(
          `/api/epub/${bookId}/translations/${chapterIndex}?language=${encodeURIComponent(code)}`,
        );
        if (language === code) {
          setEntry(null);
          setLanguage(null);
          remember(null);
        }
        await refresh();
        toast.success('That translation was removed. The original text is back.');
      } catch (error) {
        toast.error(errorMessage(error, 'Could not remove that translation.'));
      }
    },
    [bookId, chapterIndex, language, refresh, remember],
  );

  const showing = entry !== null;

  return {
    saved,
    entry,
    language,
    languageName: entry?.languageName ?? (language ? translationLanguageName(language) : null),
    showing,
    missingHere: language !== null && !showing,
    bookLanguages,
    progress,
    busy,
    open,
    setOpen,
    showOriginal,
    showTranslation,
    translate,
    remove,
  };
}
