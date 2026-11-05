'use client';

import * as React from 'react';
import Link from 'next/link';
import { toast } from 'sonner';
import { Languages, Loader2, Sparkles } from 'lucide-react';

import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { VoicePicker } from '@/components/tts/voice-picker';
import { useSettings } from '@/components/providers/settings-provider';
import { useTtsVoices } from '@/hooks/use-tts-voices';
import { api, errorMessage } from '@/hooks/use-api';
import { baseLanguage } from '@/utils/constants';
import { spokenLanguageName, voiceDisplayName } from '@/utils/voice';
import { formatRelativeTime } from '@/utils/format';
import { cn } from '@/utils/cn';
import type { NarrateSource, NarrateTarget } from '@/hooks/use-narrate-chapter';
import type { EpubTranslationSummary } from '@/types';

/**
 * One narration button, and the language question behind it.
 *
 * A chapter can have several saved translations, each of which can be spoken.
 * The reader presses Narrate once; if the section has translations, this asks
 * which one to read. If the voice configured in Settings cannot read the chosen
 * language — or the original, when that is what was chosen — the answer is not
 * a silent substitution: an English voice given Hindi text produces noise, not
 * Hindi. So the picker for that language appears here, with previews, and the
 * chosen voice is saved before generation starts.
 */

export interface NarrateDialogProps {
  target: NarrateTarget | null;
  entries: EpubTranslationSummary[] | null;
  /** Language of the original text, when it could be detected. */
  source: NarrateSource | null;
  loading: boolean;
  /** Language to select first, usually the one being read. */
  preferred: string | null;
  onOpenChange: (open: boolean) => void;
  onStart: (language: string | null) => Promise<void> | void;
}

export function NarrateDialog({
  target,
  entries,
  source,
  loading,
  preferred,
  onOpenChange,
  onStart,
}: NarrateDialogProps) {
  const settings = useSettings();
  const { voices } = useTtsVoices();

  const [selected, setSelected] = React.useState<string>('original');
  const [pendingVoice, setPendingVoice] = React.useState<string | null>(null);
  const [starting, setStarting] = React.useState(false);

  React.useEffect(() => {
    if (!target) return;
    const saved = entries?.some((entry) => entry.language === preferred) ?? false;
    setSelected(saved && preferred ? preferred : 'original');
    setPendingVoice(null);
  }, [target, entries, preferred]);

  const chosenLanguage = selected === 'original' ? null : selected;
  // The original has a language too: when it is known, "original" is checked
  // against the voice just like a translation is.
  const requiredLanguage = chosenLanguage
    ? baseLanguage(chosenLanguage)
    : source
      ? baseLanguage(source.language)
      : null;
  const configuredLanguage = baseLanguage(settings.ttsVoice);
  const mismatch = requiredLanguage !== null && configuredLanguage !== requiredLanguage;

  // Default the pending voice to the first voice that can read the language,
  // so pressing Narrate works without a detour through the list.
  React.useEffect(() => {
    if (!requiredLanguage || !mismatch) {
      setPendingVoice(null);
      return;
    }
    setPendingVoice((current) => {
      if (current && baseLanguage(current) === requiredLanguage) return current;
      const suggested = voices.find((voice) => baseLanguage(voice.locale) === requiredLanguage);
      return suggested?.name ?? null;
    });
  }, [requiredLanguage, mismatch, voices]);

  const effectiveVoice =
    mismatch && pendingVoice
      ? pendingVoice
      : settings.ttsVoice;
  const languageName = chosenLanguage
    ? spokenLanguageName(chosenLanguage)
    : source?.languageName ?? 'the original language';
  const languageArticle = /^[aeiou]/i.test(languageName) ? 'an' : 'a';

  const begin = async () => {
    if (starting) return;
    setStarting(true);
    try {
      // The generation route reads the saved setting, so the voice has to be on
      // the server before the request, not merely in this component's state.
      if (mismatch && pendingVoice && pendingVoice !== settings.ttsVoice) {
        await api.put('/api/settings', { ttsVoice: pendingVoice });
        settings.setTtsVoice(pendingVoice);
      }
      await onStart(chosenLanguage);
    } catch (error) {
      toast.error(errorMessage(error, 'Could not start narrating this chapter.'));
    } finally {
      setStarting(false);
    }
  };

  const canStart = !starting && (!mismatch || Boolean(pendingVoice));

  return (
    <Dialog open={target !== null} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle className="text-[19px]">
            Narrate “{target?.title ?? 'this chapter'}”
          </DialogTitle>
          <DialogDescription>
            {loading
              ? 'Checking which translations this chapter has…'
              : entries && entries.length > 0
                ? 'This chapter has saved translations. Choose which text should be spoken; each language is saved as its own audio.'
                : source
                  ? `This chapter has no saved translations, so it is read from the ${source.languageName} original.`
                  : 'This chapter has no saved translations, so it is read from the original text.'}
          </DialogDescription>
        </DialogHeader>

        {loading ? (
          <div className="space-y-1.5" aria-hidden>
            {Array.from({ length: 3 }).map((_, index) => (
              <div
                key={index}
                className="h-12 animate-pulse rounded-[2px] border border-hairline bg-surface"
              />
            ))}
          </div>
        ) : (
          <ul
            role="radiogroup"
            aria-label="Language to narrate"
            className="border-y border-hairline"
          >
            <NarrationOption
              checked={selected === 'original'}
              onSelect={() => setSelected('original')}
              title="Original language"
              description={
                source
                  ? `The text exactly as written — ${source.languageName}`
                  : 'The text exactly as written in the book'
              }
            />
            {(entries ?? []).map((entry) => (
              <NarrationOption
                key={entry.language}
                checked={selected === entry.language}
                onSelect={() => setSelected(entry.language)}
                title={entry.languageName}
                description={`Saved ${formatRelativeTime(entry.updatedAt)}`}
              />
            ))}
          </ul>
        )}

        {mismatch && (
          <div role="alert" className="mt-4 border-l-2 border-caution pl-3">
            <p className="flex items-center gap-2 text-[13px] font-medium text-ink">
              <Languages className="size-4 shrink-0 text-caution" aria-hidden />
              The selected voice cannot read {languageName}.
            </p>
            <p className="mt-1 text-xs leading-relaxed text-ink-muted">
              “{voiceDisplayName(settings.ttsVoice)}” reads{' '}
              {spokenLanguageName(settings.ttsVoice)}. Choose {languageArticle} {languageName} voice
              below; it will be saved as your default voice for future narration.
            </p>

            {pendingVoice ? (
              <VoicePicker
                value={effectiveVoice}
                language={chosenLanguage ?? source?.language ?? null}
                onChange={(voice) => setPendingVoice(voice.name)}
                compact
                className="mt-3"
              />
            ) : (
              <p className="mt-2 text-xs text-ink-muted">
                No compatible voice could be loaded.{' '}
                <Link href="/settings" className="underline underline-offset-4 hover:text-ink">
                  Change the voice in Settings
                </Link>{' '}
                and try again.
              </p>
            )}
          </div>
        )}

        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)} disabled={starting}>
            Cancel
          </Button>
          <Button
            onClick={() => void begin()}
            disabled={!canStart}
            icon={
              starting ? (
                <Loader2 className="size-4 animate-spin" aria-hidden />
              ) : (
                <Sparkles className="size-4" aria-hidden />
              )
            }
          >
            {starting
              ? 'Starting…'
              : chosenLanguage
                ? `Narrate in ${languageName}`
                : 'Narrate original'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function NarrationOption({
  checked,
  onSelect,
  title,
  description,
}: {
  checked: boolean;
  onSelect: () => void;
  title: string;
  description: string;
}) {
  return (
    <li>
      <button
        type="button"
        role="radio"
        aria-checked={checked}
        onClick={onSelect}
        className={cn(
          'flex w-full items-center gap-3 border-b border-hairline px-1 py-3 text-left last:border-b-0',
          'focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent',
          checked ? 'bg-accent-wash' : 'hover:bg-accent-wash/40',
        )}
      >
        <span
          aria-hidden
          className={cn(
            'grid size-4 shrink-0 place-items-center rounded-full border transition-colors',
            checked ? 'border-accent bg-accent' : 'border-hairline-strong',
          )}
        >
          {checked && <span className="size-1.5 rounded-full bg-[#fffaf1]" />}
        </span>
        <span className="min-w-0">
          <span
            className={cn(
              'block truncate text-[13.5px]',
              checked ? 'font-medium text-ink' : 'text-ink-muted',
            )}
          >
            {title}
          </span>
          <span className="block truncate text-[11px] text-ink-subtle">{description}</span>
        </span>
      </button>
    </li>
  );
}
