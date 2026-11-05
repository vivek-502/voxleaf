'use client';

import * as React from 'react';
import { BookOpenCheck, ExternalLink, Loader2, Search, Trash2, Volume2 } from 'lucide-react';

import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { api, errorMessage } from '@/hooks/use-api';
import { useSpeak } from '@/hooks/use-speak';
import { translationLanguageName } from '@/utils/constants';
import type { DictionaryResult, SavedWord } from '@/types';

export interface DictionaryRequest {
  word: string;
  language: string;
}

export interface DictionaryDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  request: DictionaryRequest | null;
  savedWords: SavedWord[];
  onSave: (entry: DictionaryResult) => Promise<SavedWord | null>;
  onRemove: (id: string) => Promise<void>;
  onSearchInBook: (word: string) => void;
}

/**
 * Defining a word without leaving the page.
 *
 * The server chooses the dictionary — dictionaryapi.dev for English, Wiktionary
 * for everything else — and caches the answer, so a word looked at twice is
 * looked up once. Saving a word puts it in the notebook in the study panel; the
 * saved copy is what survives after the reader has closed the book.
 */
export function DictionaryDialog({
  open,
  onOpenChange,
  request,
  savedWords,
  onSave,
  onRemove,
  onSearchInBook,
}: DictionaryDialogProps) {
  const [entry, setEntry] = React.useState<DictionaryResult | null>(null);
  const [loading, setLoading] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const [attempt, setAttempt] = React.useState(0);
  const [saving, setSaving] = React.useState(false);
  const { speak } = useSpeak();

  React.useEffect(() => {
    if (!open || !request) return;
    let cancelled = false;
    setLoading(true);
    setError(null);
    setEntry(null);

    void (async () => {
      try {
        const result = await api.get<{ entry: DictionaryResult }>('/api/dictionary', {
          word: request.word,
          language: request.language,
        });
        if (!cancelled) setEntry(result.entry);
      } catch (err) {
        if (!cancelled) setError(errorMessage(err, 'That word could not be looked up.'));
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [attempt, open, request]);

  const saved = React.useMemo(() => {
    if (!entry) return undefined;
    return savedWords.find(
      (word) =>
        word.word.toLowerCase() === entry.word.toLowerCase() && word.language === entry.language,
    );
  }, [entry, savedWords]);

  const sayIt = () => {
    const text = entry?.word ?? request?.word;
    if (!text) return;
    speak(text, entry?.language ?? request?.language ?? 'en');
  };

  if (!open || !request) return null;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-xl">
        <DialogHeader>
          <p className="font-mono text-[10px] uppercase tracking-[0.16em] text-ink-subtle">
            Dictionary · {translationLanguageName(request.language) || request.language}
          </p>
          <DialogTitle className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
            <span className="font-display text-[30px] leading-none tracking-[-0.02em]">
              {entry?.word ?? request.word}
            </span>
            {entry?.phonetic ? (
              <span className="font-mono text-[12px] font-normal tracking-normal text-ink-muted">
                {entry.phonetic}
              </span>
            ) : null}
          </DialogTitle>
          <DialogDescription>
            {entry
              ? `${entry.senses.reduce((count, sense) => count + sense.definitions.length, 0)} definitions · ${entry.source.name}`
              : 'Looking it up…'}
          </DialogDescription>
        </DialogHeader>

        {loading ? (
          <div className="flex items-center gap-2 py-10 text-sm text-ink-muted">
            <Loader2 className="size-4 animate-spin" aria-hidden />
            Looking up “{request.word}”…
          </div>
        ) : error ? (
          <div className="border border-danger/30 bg-danger/5 px-4 py-5 text-sm">
            <p className="text-ink">{error}</p>
            <p className="mt-1 text-ink-muted">
              The lookup services are free and occasionally rate-limit a burst of requests.
            </p>
            <Button
              type="button"
              variant="outline"
              size="sm"
              className="mt-3"
              onClick={() => setAttempt((value) => value + 1)}
            >
              Try again
            </Button>
          </div>
        ) : entry ? (
          <div className="max-h-[52dvh] space-y-5 overflow-y-auto pr-1">
            {entry.senses.map((sense, senseIndex) => (
              <section key={`${sense.partOfSpeech}-${senseIndex}`}>
                <h3 className="font-mono text-[10px] uppercase tracking-[0.14em] text-accent">
                  {sense.partOfSpeech}
                </h3>
                <ol className="mt-1.5 space-y-2.5">
                  {sense.definitions.map((definition, index) => (
                    <li key={index} className="text-[13.5px] leading-relaxed text-ink">
                      <span className="mr-1.5 font-mono text-[10px] text-ink-subtle tabular-nums">
                        {index + 1}.
                      </span>
                      {definition.definition}
                      {definition.example ? (
                        <p className="mt-1 border-l border-hairline pl-2.5 text-[12.5px] italic text-ink-muted">
                          {definition.example}
                        </p>
                      ) : null}
                    </li>
                  ))}
                </ol>
              </section>
            ))}

            <a
              href={entry.source.url}
              target="_blank"
              rel="noreferrer noopener"
              className="inline-flex items-center gap-1.5 text-[11px] text-ink-muted underline underline-offset-4 hover:text-ink"
            >
              From {entry.source.name}
              <ExternalLink className="size-3" aria-hidden />
            </a>
          </div>
        ) : null}

        <DialogFooter className="sm:justify-between">
          <Button
            type="button"
            variant={saved ? 'outline' : 'primary'}
            className="sm:mr-auto"
            icon={
              saved ? (
                <BookOpenCheck className="size-4" aria-hidden />
              ) : (
                <BookOpenCheck className="size-4" aria-hidden />
              )
            }
            loading={saving}
            disabled={!entry || Boolean(saved)}
            onClick={async () => {
              if (!entry) return;
              setSaving(true);
              try {
                await onSave(entry);
              } finally {
                setSaving(false);
              }
            }}
          >
            {saved ? 'Saved to words' : 'Save word'}
          </Button>
          {saved ? (
            <Button
              type="button"
              variant="ghost"
              icon={<Trash2 className="size-4" aria-hidden />}
              onClick={() => void onRemove(saved.id)}
            >
              Remove
            </Button>
          ) : null}
          <Button
            type="button"
            variant="ghost"
            icon={<Search className="size-4" aria-hidden />}
            onClick={() => {
              const word = entry?.word ?? request.word;
              onOpenChange(false);
              onSearchInBook(word);
            }}
          >
            In this book
          </Button>
          <Button
            type="button"
            variant="outline"
            icon={<Volume2 className="size-4" aria-hidden />}
            onClick={sayIt}
          >
            Say it
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
