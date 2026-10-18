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
import type { EpubTranslationSummary } from '@/types';

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
  progress,
  busy,
  showOriginal,
  showTranslation,
  translate,
  remove,
}: TranslateDialogProps) {
  const [query, setQuery] = React.useState('');
  const [chosen, setChosen] = React.useState<string>('');

  // Default to what the reader is already reading in, or the section's language.
  React.useEffect(() => {
    if (!open) return;
    // The top of the list is what a reader most often wants, and it is the first
    // entry because the list is ordered that way; only a remembered language or
    // one this section already has beats it.
    setChosen((current) => current || language || saved[0]?.language || TRANSLATION_LANGUAGES[0]!.code);
  }, [open, language, saved]);

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

    const all = [...extra, ...TRANSLATION_LANGUAGES];
    if (!needle) return all;
    return all.filter(
      (item) =>
        item.code.toLowerCase().includes(needle) ||
        item.name.toLowerCase().includes(needle) ||
        item.native.toLowerCase().includes(needle),
    );
  }, [bookLanguages, query]);

  const chosenName = chosen ? translationLanguageName(chosen) : null;
  const chosenSaved = chosen ? savedByCode.get(chosen) : undefined;
  const fraction = progress && progress.total > 0 ? progress.done / progress.total : 0;

  return (
    <Dialog open={open} onOpenChange={busy ? () => undefined : onOpenChange}>
      <DialogContent className="max-w-xl">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2 text-lg">
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
            className="mb-4 flex items-center gap-3 rounded-xl border border-hairline bg-overlay px-3.5 py-2.5 text-sm"
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
          <div className="mb-4 rounded-xl border border-hairline bg-overlay px-3.5 py-2.5 text-sm">
            This section has no {languageName} translation yet. Choose it below and it will be made and
            saved.
          </div>
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
          className="mb-3 h-10 w-full rounded-full border border-hairline bg-overlay px-4 text-sm text-ink transition-colors placeholder:text-ink-subtle hover:border-hairline-strong focus:outline-none focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent-soft"
        />

        <ul
          className="max-h-64 overflow-y-auto overscroll-contain rounded-xl border border-hairline"
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
                        'grid size-4 shrink-0 place-items-center rounded-full border',
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
            <p className="mb-2 text-xs font-medium uppercase tracking-wide text-ink-subtle">
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
          <div className="mt-4 rounded-xl border border-hairline bg-overlay px-3.5 py-3">
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
        'flex items-center gap-1 rounded-full border py-1 pl-3 pr-1 text-xs',
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
        className="grid size-6 place-items-center rounded-full transition-colors hover:bg-overlay hover:text-ink disabled:opacity-50"
      >
        <Trash2 className="size-3.5" aria-hidden />
      </button>
    </li>
  );
}
