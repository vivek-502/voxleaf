'use client';

import * as React from 'react';
import { Check, Languages, RotateCcw, Trash2 } from 'lucide-react';

import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { ProgressBar } from '@/components/book/book-card';
import { TRANSLATION_LANGUAGES, translationLanguageName } from '@/utils/constants';
import { cn } from '@/utils/cn';
import type { EpubTranslationSummary, LanguageDetection } from '@/types';

export interface TranslateDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Title of the section being translated, for the heading of the dialog. */
  sectionTitle: string;
  /** Saved translations of the section being read. */
  saved: EpubTranslationSummary[];
  /** The book-wide language, or null when the original is being read. */
  language: string | null;
  languageName: string | null;
  /** Whether the reader is looking at translated text. */
  showing: boolean;
  /** A language is chosen but this section has no translation of it. */
  missingHere: boolean;
  /** Languages the book has translations in, with how many sections. */
  bookLanguages: { language: string; languageName: string; sections: number }[];
  /** What language the section is written in, or null if it could not be told. */
  source: LanguageDetection | null;
  /** The source language is being detected. */
  detectingSource: boolean;
  /** Characters translated so far, while a job runs. */
  progress: { done: number; total: number } | null;
  busy: boolean;
  showOriginal: () => void;
  showTranslation: (language: string) => void;
  translate: (language: string, options?: { force?: boolean }) => Promise<boolean>;
  remove: (language: string) => Promise<void>;
}
/**
 * Choosing a language for this section.
 *
 * One list does three jobs, because they are the same decision seen from
 * different sides: which language to read in, which languages this section has
 * already been translated into (saved, so they can be reopened or removed), and
 * which language to translate it into now.
 *
 * The provider is free and takes the whole section in paragraphs, so a long one
 * takes a while. Progress is reported in characters rather than shown as a
 * spinner, because "which of these twenty seconds is it in" is the question a
 * reader has, and a spinner answers none of it.
 */
export function TranslateDialog({
  open,
  onOpenChange,
  sectionTitle,
  saved,
  language,
  languageName,
  showing,
  missingHere,
  bookLanguages,
  source,
  detectingSource,
  progress,
  busy,
  showOriginal,
  showTranslation,
  translate,
  remove,
}: TranslateDialogProps) {
  const [query, setQuery] = React.useState('');
  const [chosen, setChosen] = React.useState<string>('');
  const sourceCode = source?.language?.toLowerCase() ?? null;
  const sourceBase = sourceCode?.split('-')[0] ?? null;

  /**
   * Whether a target is the language already on screen.
   *
   * Detection may answer with a bare code (`pt`) while the list holds regional
   * ones (`pt-BR`), so comparing the two strings alone offered to translate
   * Portuguese into Brazilian Portuguese. A bare source covers every variant of
   * its language; a regional source still leaves its siblings as real targets —
   * European Portuguese is a genuine translation of Brazilian Portuguese.
   */
  const isSourceLanguage = React.useCallback(
    (code: string) => {
      if (!sourceCode) return false;
      const lower = code.toLowerCase();
      if (lower === sourceCode) return true;
      if (lower.split('-')[0] !== sourceBase) return false;
      return !lower.includes('-') || !sourceCode.includes('-');
    },
    [sourceBase, sourceCode],
  );

  /**
   * Where to start when nothing is remembered.
   *
   * English is the useful target for a book written in anything else, and for a
   * book already in English it is no target at all. Before detection answers —
   * or if it cannot — Spanish keeps the old, safe default rather than offering to
   * translate an English book into English.
   */
  const defaultTarget = sourceCode ? (sourceCode === 'en' ? 'es' : 'en') : 'es';

  // Default to what the reader is already reading in, then this section's own
  // translation, then the language that makes a translation worth making.
  React.useEffect(() => {
    if (!open) return;
    setChosen((current) => {
      // A language detected after the dialog opened can be the one already
      // selected (an English book starts on English before detection answers);
      // move off it rather than offering a translation into the source.
      if (current && !isSourceLanguage(current)) return current;
      return language || saved[0]?.language || defaultTarget;
    });
  }, [open, language, saved, sourceCode, defaultTarget, isSourceLanguage]);

  React.useEffect(() => {
    if (open) return;
    setQuery('');
  }, [open]);

  const savedByCode = React.useMemo(
    () => new Map(saved.map((item) => [item.language, item])),
    [saved],
  );

  /**
   * Languages to offer: everything on file first — those are the ones this book
   * has already been read in — then the standard list, filtered by the search.
   */
  const options = React.useMemo(() => {
    const needle = query.trim().toLowerCase();
    const extra = bookLanguages
      .filter((item) => !TRANSLATION_LANGUAGES.some((known) => known.code === item.language))
      .map((item) => ({ code: item.language, name: item.languageName, native: item.languageName }));

    // The section's own language is not a translation target. This is the whole
    // reason detection exists: without it, an English book had no English entry
    // to suppress and every non-English book looked English.
    const all = [...extra, ...TRANSLATION_LANGUAGES].filter(
      (item) => !isSourceLanguage(item.code),
    );
    if (!needle) return all;
    return all.filter(
      (item) =>
        item.code.toLowerCase().includes(needle) ||
        item.name.toLowerCase().includes(needle) ||
        item.native.toLowerCase().includes(needle),
    );
  }, [bookLanguages, query, isSourceLanguage]);

  const chosenName = chosen ? translationLanguageName(chosen) : null;
  const chosenSaved = chosen ? savedByCode.get(chosen) : undefined;
  const fraction = progress && progress.total > 0 ? progress.done / progress.total : 0;

  return (
    <Dialog open={open} onOpenChange={busy ? () => undefined : onOpenChange}>
      <DialogContent className="max-w-xl">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Languages className="size-4.5 shrink-0 text-accent" aria-hidden />
            Translate this section
          </DialogTitle>
          <DialogDescription>
            {sectionTitle} — a chapter is translated a paragraph at a time and then saved, so this
            section reads in that language from now on, even offline.
          </DialogDescription>
        </DialogHeader>

        {showing && languageName ? (
          <div
            className="mb-4 flex items-center gap-3 border border-hairline bg-overlay px-3.5 py-2.5 text-sm"
          >
            <Check className="size-4 shrink-0 text-accent" aria-hidden />
            <span className="min-w-0 flex-1">
              Showing the <strong className="font-semibold">{languageName}</strong> translation.
            </span>
            <Button variant="ghost" size="sm" onClick={showOriginal}>
              Original
            </Button>
          </div>
        ) : missingHere && languageName ? (
          <div className="mb-4 border-l-2 border-caution/60 bg-overlay px-3.5 py-2.5 text-sm">
            This section has no {languageName} translation yet. Choose it below and it will be made and
            saved.
          </div>
        ) : null}

        {detectingSource || source?.language ? (
          <p
            className="mb-3 flex items-center gap-2 text-xs leading-relaxed text-ink-subtle"
            aria-live="polite"
          >
            <Languages className="size-3.5 shrink-0 text-accent" aria-hidden />
            {detectingSource ? (
              <span>Detecting the language this section is written in…</span>
            ) : (
              <span>
                This section is written in{' '}
                <strong className="font-medium text-ink-muted">{source?.languageName}</strong>
                {source?.source === 'metadata' ? ' (from the book’s metadata)' : ''}.
              </span>
            )}
          </p>
        ) : null}

        <label className="sr-only" htmlFor="translation-search">
          Search languages
        </label>
        <input
          id="translation-search"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          placeholder="Search languages…"
          disabled={busy}
          autoComplete="off"
          className="mb-3 h-10 w-full border border-hairline bg-overlay px-3.5 text-sm text-ink transition-colors placeholder:text-ink-subtle hover:border-hairline-strong focus:outline-none focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent"
        />

        <ul
          className="max-h-64 overflow-y-auto overscroll-contain border border-hairline"
          role="listbox"
          aria-label="Languages"
        >
          {options.length === 0 ? (
            <li className="px-4 py-6 text-center text-sm text-ink-subtle">
              No language matches “{query}”.
            </li>
          ) : (
            options.map((item) => {
              const isChosen = item.code === chosen;
              const stored = savedByCode.get(item.code);
              return (
                <li key={item.code}>
                  <button
                    type="button"
                    role="option"
                    aria-selected={isChosen}
                    disabled={busy}
                    onClick={() => setChosen(item.code)}
                    className={cn(
                      'flex w-full items-center gap-3 px-3.5 py-2.5 text-left text-sm transition-colors',
                      'hover:bg-overlay disabled:cursor-not-allowed disabled:opacity-50',
                      isChosen && 'bg-overlay',
                    )}
                  >
                    <span
                      className={cn(
                        'grid size-4 shrink-0 place-items-center border',
                        isChosen ? 'border-accent bg-accent' : 'border-hairline-strong',
                      )}
                      aria-hidden
                    >
                      {isChosen && <Check className="size-3 text-surface" />}
                    </span>
                    <span className="min-w-0 flex-1">
                      <span className="block truncate font-medium text-ink">{item.name}</span>
                      <span className="block truncate text-xs text-ink-subtle">
                        {item.native} · {item.code}
                      </span>
                    </span>
                    {stored ? (
                      <span className="shrink-0 text-[11px] text-ink-subtle">
                        {stored.stale ? 'needs redoing' : 'saved'}
                      </span>
                    ) : null}
                  </button>
                </li>
              );
            })
          )}
        </ul>

        {saved.length > 0 ? (
          <div className="mt-4">
            <p className="mb-2 font-mono text-[10px] uppercase tracking-[0.12em] text-ink-subtle">
              Saved for this section
            </p>
            <ul className="flex flex-wrap gap-2">
              {saved.map((item) => (
                <SavedChip
                  key={item.language}
                  item={item}
                  active={item.language === language && showing}
                  disabled={busy}
                  onShow={() => showTranslation(item.language)}
                  onRemove={() => void remove(item.language)}
                />
              ))}
            </ul>
          </div>
        ) : null}

        {busy ? (
          <div className="mt-4 border border-hairline bg-overlay px-3.5 py-3">
            <ProgressBar value={fraction} label="Translating" showValue />
            <p className="mt-2 text-xs text-ink-subtle">
              {progress && progress.total > 0
                ? `${progress.done.toLocaleString()} of ${progress.total.toLocaleString()} characters. A long chapter can take a minute.`
                : 'Sending the section…'}
            </p>
          </div>
        ) : null}

        <DialogFooter className="mt-5">
          <Button variant="ghost" disabled={busy} onClick={() => onOpenChange(false)}>
            Close
          </Button>
          {chosenSaved ? (
            <>
              <Button
                variant="ghost"
                icon={<RotateCcw className="size-4" aria-hidden />}
                disabled={busy || !showing}
                onClick={() => void translate(chosen, { force: true })}
              >
                Translate again
              </Button>
              <Button
                icon={<Check className="size-4" aria-hidden />}
                disabled={busy || (showing && chosen === language)}
                onClick={() => showTranslation(chosen)}
              >
                {showing && chosen === language ? 'Reading this' : 'Read this'}
              </Button>
            </>
          ) : (
            <Button
              icon={<Languages className="size-4" aria-hidden />}
              disabled={busy || !chosen}
              onClick={() => void translate(chosen)}
            >
              {chosenName ? `Translate to ${chosenName}` : 'Translate'}
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/** One saved translation, with the two things a reader wants to do with it. */
function SavedChip({
  item,
  active,
  disabled,
  onShow,
  onRemove,
}: {
  item: EpubTranslationSummary;
  active: boolean;
  disabled: boolean;
  onShow: () => void;
  onRemove: () => void;
}) {
  return (
    <li
      className={cn(
        'flex items-center gap-1 border py-1 pl-2.5 pr-1 text-xs',
        active ? 'border-accent text-ink' : 'border-hairline text-ink-subtle',
      )}
    >
      <button
        type="button"
        disabled={disabled}
        onClick={onShow}
        title={item.stale ? 'This section changed since it was translated' : undefined}
        className="font-medium transition-opacity hover:opacity-70 disabled:opacity-50"
      >
        {item.languageName}
        {item.stale ? ' ·' : ''}
      </button>
      <button
        type="button"
        disabled={disabled}
        onClick={onRemove}
        aria-label={`Remove the ${item.languageName} translation`}
        className="grid size-6 place-items-center transition-colors hover:bg-overlay hover:text-ink disabled:opacity-50"
      >
        <Trash2 className="size-3.5" aria-hidden />
      </button>
    </li>
  );
}
