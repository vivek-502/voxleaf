'use client';

import * as React from 'react';
import { toast } from 'sonner';

import { useSettings } from '@/components/providers/settings-provider';
import { api, errorMessage } from '@/hooks/use-api';
import { baseLanguage } from '@/utils/constants';
import type { EpubTranslationSummary, LanguageDetection } from '@/types';

/**
 * Asking which language a chapter should be narrated in, when there is a choice.
 *
 * The reader shows one text at a time. Audio should match it — narrating the
 * English original of a chapter the reader is reading in Hindi is the same bug
 * as showing the wrong words. But whether there is a choice depends on the
 * *section*, not the book: a translation may exist for chapter three and not
 * chapter four. So the saved translations of the section are fetched first, and
 * a section with none is narrated immediately in the original rather than
 * opening a dialog whose only option is "original".
 *
 * The original still has a language, though, and the configured voice may not
 * be able to read it. So that path checks the detected language against the
 * voice and opens the same dialog when they do not match — an English voice
 * given Portuguese text produces noise, not Portuguese, whether or not a
 * translation exists.
 */

export interface NarrateTarget {
  /** Spine index of the section, or null for a hand-made chapter. */
  epubIndex: number | null;
  title: string;
  chapterNumber?: number;
}

export type NarrateStart = (language: string | null) => void | Promise<void>;

export interface NarrateSource {
  /** Language the section is written in, as detected by the server. */
  language: string;
  languageName: string;
}

export interface NarrateChooser {
  /** The section waiting for a language, or null when the dialog is closed. */
  target: NarrateTarget | null;
  /** Saved, usable translations of that section; null while they load. */
  entries: EpubTranslationSummary[] | null;
  /** Language of the original text, when it could be detected. */
  source: NarrateSource | null;
  loading: boolean;
  /** The language to select first, e.g. the one being read. */
  preferred: string | null;
  /** Ask for a language, then call `start` with the answer. */
  request: (target: NarrateTarget, start: NarrateStart, preferred?: string | null) => Promise<void>;
  /** Start narration in the chosen language and close the dialog. */
  start: (language: string | null) => Promise<void>;
  /** Dismiss without starting anything. */
  close: () => void;
}

export function useNarrateChapter(bookId: string): NarrateChooser {
  const settings = useSettings();
  const voice = settings.ttsVoice;

  const [target, setTarget] = React.useState<NarrateTarget | null>(null);
  const [entries, setEntries] = React.useState<EpubTranslationSummary[] | null>(null);
  const [source, setSource] = React.useState<NarrateSource | null>(null);
  const [loading, setLoading] = React.useState(false);
  const [preferred, setPreferred] = React.useState<string | null>(null);

  const startRef = React.useRef<NarrateStart | null>(null);
  // Every request gets a token: closing the dialog, or asking about another
  // chapter, must stop the older answer from opening it again.
  const tokenRef = React.useRef(0);

  React.useEffect(
    () => () => {
      tokenRef.current += 1;
    },
    [],
  );

  const clear = React.useCallback(() => {
    setTarget(null);
    setEntries(null);
    setSource(null);
    setLoading(false);
    setPreferred(null);
  }, []);

  const close = React.useCallback(() => {
    tokenRef.current += 1;
    startRef.current = null;
    clear();
  }, [clear]);

  const request = React.useCallback(
    async (nextTarget: NarrateTarget, start: NarrateStart, first?: string | null) => {
      const token = (tokenRef.current += 1);
      startRef.current = start;

      // No section behind the chapter means no translations and no text to
      // detect a language from; narrate it as it is.
      if (nextTarget.epubIndex === null) {
        startRef.current = null;
        await start(null);
        return;
      }

      const index = nextTarget.epubIndex;
      setTarget(nextTarget);
      setEntries(null);
      setSource(null);
      setPreferred(first ?? null);
      setLoading(true);

      // Both answers shape the same decision — which languages are on offer,
      // and whether the current voice can read the original — so they travel
      // together and a slow detection does not hold the translations back.
      const [translations, detection] = await Promise.allSettled([
        api.get<{ entries: EpubTranslationSummary[] }>(`/api/epub/${bookId}/translations`, {
          index,
        }),
        api.get<LanguageDetection>(`/api/epub/${bookId}/language`, { index }),
      ]);
      if (tokenRef.current !== token) return;

      // A stale translation was made from text the book no longer has;
      // narrating it would read wording the reader cannot see. The server
      // falls back to the original for those, so they are not a choice here.
      const usable =
        translations.status === 'fulfilled'
          ? (translations.value.entries ?? []).filter((entry) => !entry.stale)
          : [];
      const detected =
        detection.status === 'fulfilled' &&
        detection.value.language &&
        detection.value.languageName
          ? { language: detection.value.language, languageName: detection.value.languageName }
          : null;
      const voiceReadsOriginal =
        detected === null || baseLanguage(voice) === baseLanguage(detected.language);

      // The common case: nothing to choose between, and the voice can read the
      // text that is there.
      if (usable.length === 0 && voiceReadsOriginal) {
        startRef.current = null;
        clear();
        if (translations.status === 'rejected') {
          toast.error(
            errorMessage(translations.reason, 'Could not check saved translations. Narrating the original.'),
          );
        }
        await start(null);
        return;
      }

      if (translations.status === 'rejected') {
        toast.error(
          errorMessage(translations.reason, 'Could not check saved translations.'),
        );
      }

      setEntries(usable);
      setSource(detected);
      setLoading(false);
    },
    [bookId, voice, clear],
  );

  const start = React.useCallback(
    async (language: string | null) => {
      const begin = startRef.current;
      close();
      if (begin) await begin(language);
    },
    [close],
  );

  return { target, entries, source, loading, preferred, request, start, close };
}
