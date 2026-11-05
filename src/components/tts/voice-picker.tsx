'use client';

import * as React from 'react';
import { toast } from 'sonner';
import { Check, Loader2, Play, Search, Square } from 'lucide-react';

import { Button } from '@/components/ui/button';
import { Input, Label } from '@/components/ui/inputs';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { TooltipWrap } from '@/components/ui/tooltip';
import { useTtsVoices } from '@/hooks/use-tts-voices';
import { errorMessage } from '@/hooks/use-api';
import { baseLanguage } from '@/utils/constants';
import {
  regionName,
  spokenLanguageName,
  voiceDescription,
  voiceDisplayName,
  voiceGender,
  voiceRegion,
  type VoiceGender,
} from '@/utils/voice';
import { cn } from '@/utils/cn';
import type { TtsVoice } from '@/types';

/**
 * Choosing a voice from Edge's catalogue.
 *
 * The catalogue is large — every language has several speakers, and a language
 * with regional variants has several more — so a flat dropdown is a wall of
 * identifiers. The filters are the four ways people actually narrow it down:
 * the language they want read, the speaker's gender, the accent's country, and
 * a search over everything else. A preview button on each row answers the
 * question a name cannot: what does this person sound like?
 */

export interface VoicePickerProps {
  /** Currently selected voice name. */
  value: string;
  onChange: (voice: TtsVoice) => void;
  /**
   * Restrict the picker to voices that read this language. Used when a
   * narration language has already been chosen and the current voice cannot
   * read it, so the only useful answers are in that language.
   */
  language?: string | null;
  /** Shorter list for use inside a dialog. */
  compact?: boolean;
  /** Focus the search field on mount. */
  autoFocus?: boolean;
  className?: string;
}

type GenderFilter = 'all' | VoiceGender;

export function VoicePicker({
  value,
  onChange,
  language = null,
  compact = false,
  autoFocus = false,
  className,
}: VoicePickerProps) {
  const { voices, loading, failed, reload } = useTtsVoices();
  const lockedLanguage = language ? baseLanguage(language) : null;

  const [query, setQuery] = React.useState('');
  const [languageFilter, setLanguageFilter] = React.useState<string>(() =>
    lockedLanguage ?? baseLanguage(value) ?? 'all',
  );
  const [gender, setGender] = React.useState<GenderFilter>('all');
  const [country, setCountry] = React.useState('all');

  // A locked language wins over whatever was remembered, and the picker follows
  // a change of target language instead of keeping a filter that now hides
  // every voice.
  React.useEffect(() => {
    if (lockedLanguage) setLanguageFilter(lockedLanguage);
  }, [lockedLanguage]);

  const languages = React.useMemo(() => {
    const codes = new Set<string>();
    for (const voice of voices) {
      const code = baseLanguage(voice.locale);
      if (code) codes.add(code);
    }
    return [...codes]
      .map((code) => ({ code, name: spokenLanguageName(code) }))
      .sort((a, b) => a.name.localeCompare(b.name));
  }, [voices]);

  const languagesShown = React.useMemo(
    () =>
      languageFilter === 'all'
        ? languages
        : languages.filter((item) => item.code === languageFilter),
    [languages, languageFilter],
  );

  // Countries come from the voices the language filter admits: offering
  // "Iceland" under Hindi would only ever produce an empty list.
  const countries = React.useMemo(() => {
    const codes = new Set<string>();
    for (const voice of voices) {
      if (languageFilter !== 'all' && baseLanguage(voice.locale) !== languageFilter) continue;
      const region = voiceRegion(voice.locale);
      if (region) codes.add(region);
    }
    return [...codes]
      .map((code) => ({ code, name: regionName(code) }))
      .sort((a, b) => a.name.localeCompare(b.name));
  }, [voices, languageFilter]);

  React.useEffect(() => {
    if (country !== 'all' && !countries.some((item) => item.code === country)) {
      setCountry('all');
    }
  }, [countries, country]);

  const filtered = React.useMemo(() => {
    const needle = query.trim().toLowerCase();
    return voices
      .filter((voice) => {
        if (languageFilter !== 'all' && baseLanguage(voice.locale) !== languageFilter) return false;
        if (gender !== 'all' && voiceGender(voice) !== gender) return false;
        const region = voiceRegion(voice.locale);
        if (country !== 'all' && region !== country) return false;
        if (!needle) return true;
        return [
          voice.name,
          voice.locale,
          spokenLanguageName(voice.locale),
          region ? regionName(region) : '',
          voiceGender(voice),
          voiceDisplayName(voice.name),
        ]
          .join(' ')
          .toLowerCase()
          .includes(needle);
      })
      .sort(
        (a, b) =>
          spokenLanguageName(a.locale).localeCompare(spokenLanguageName(b.locale)) ||
          voiceDisplayName(a.name).localeCompare(voiceDisplayName(b.name)),
      );
  }, [voices, languageFilter, gender, country, query]);

  /* ---------------------------------------------------------------------- */
  /* Preview playback                                                        */
  /* ---------------------------------------------------------------------- */

  const audioRef = React.useRef<HTMLAudioElement | null>(null);
  // One object URL per voice: hearing the same voice twice should not cost a
  // second synthesis, and the clips are tiny.
  const clipsRef = React.useRef(new Map<string, string>());
  const [loadingPreview, setLoadingPreview] = React.useState<string | null>(null);
  const [playing, setPlaying] = React.useState<string | null>(null);

  const playClip = React.useCallback((url: string, name: string) => {
    const audio = audioRef.current ?? new Audio();
    audioRef.current = audio;
    audio.onended = () => setPlaying(null);
    audio.onerror = () => {
      setPlaying(null);
      toast.error('That preview could not be played.');
    };
    audio.src = url;
    setPlaying(name);
    void audio.play().catch(() => {
      setPlaying(null);
      toast.error('Your browser blocked audio playback. Try pressing preview again.');
    });
  }, []);

  const stopPreview = React.useCallback(() => {
    audioRef.current?.pause();
    setPlaying(null);
  }, []);

  const preview = React.useCallback(
    async (voice: TtsVoice) => {
      if (playing === voice.name) {
        stopPreview();
        return;
      }
      stopPreview();

      const cached = clipsRef.current.get(voice.name);
      if (cached) {
        playClip(cached, voice.name);
        return;
      }

      setLoadingPreview(voice.name);
      try {
        const response = await fetch('/api/tts/preview', {
          method: 'POST',
          headers: { 'content-type': 'application/json' },
          body: JSON.stringify({ voice: voice.name }),
        });
        if (!response.ok) {
          const body = (await response.json().catch(() => null)) as {
            error?: { message?: string };
          } | null;
          throw new Error(body?.error?.message ?? 'The preview could not be generated.');
        }
        const blob = await response.blob();
        const url = URL.createObjectURL(blob);
        clipsRef.current.set(voice.name, url);
        playClip(url, voice.name);
      } catch (error) {
        toast.error(errorMessage(error, 'The preview could not be generated.'));
      } finally {
        setLoadingPreview(null);
      }
    },
    [playClip, playing, stopPreview],
  );

  // Stopping on unmount frees both the audio element and the clips it played.
  React.useEffect(() => {
    const clips = clipsRef.current;
    return () => {
      audioRef.current?.pause();
      audioRef.current = null;
      for (const url of clips.values()) URL.revokeObjectURL(url);
      clips.clear();
    };
  }, []);

  /* ---------------------------------------------------------------------- */
  /* Rendering                                                               */
  /* ---------------------------------------------------------------------- */

  if (failed) {
    return (
      <div
        className={cn('border border-hairline bg-surface px-4 py-3 text-sm text-ink-muted', className)}
      >
        <p>Could not load the voice list. The voice service may be unavailable.</p>
        <Button variant="subtle" size="sm" className="mt-2" onClick={reload}>
          Try again
        </Button>
      </div>
    );
  }

  const selected = voices.find((voice) => voice.name === value) ?? null;

  return (
    <div className={cn('space-y-3', className)}>
      <div className="grid gap-2 sm:grid-cols-2">
        <div className="relative sm:col-span-2">
          <Search
            className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-ink-subtle"
            aria-hidden
          />
          <Input
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Search voices, languages, countries…"
            aria-label="Search voices"
            className="h-9 pl-9 text-[13px]"
            autoFocus={autoFocus}
            disabled={loading}
          />
        </div>

        <div className="space-y-1">
          <Label htmlFor="voice-picker-language" className="text-[9.5px]">
            Language
          </Label>
          <Select
            value={languageFilter}
            onValueChange={setLanguageFilter}
            disabled={loading || Boolean(lockedLanguage)}
          >
            <SelectTrigger id="voice-picker-language">
              <SelectValue>
                {languageFilter === 'all'
                  ? 'All languages'
                  : spokenLanguageName(languageFilter)}
              </SelectValue>
            </SelectTrigger>
            <SelectContent className="max-h-72">
              {!lockedLanguage && <SelectItem value="all">All languages</SelectItem>}
              {languagesShown.map((item) => (
                <SelectItem key={item.code} value={item.code}>
                  {item.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>

        <div className="space-y-1">
          <Label htmlFor="voice-picker-country" className="text-[9.5px]">
            Country
          </Label>
          <Select value={country} onValueChange={setCountry} disabled={loading}>
            <SelectTrigger id="voice-picker-country">
              <SelectValue>
                {country === 'all' ? 'All countries' : regionName(country)}
              </SelectValue>
            </SelectTrigger>
            <SelectContent className="max-h-72">
              <SelectItem value="all">All countries</SelectItem>
              {countries.map((item) => (
                <SelectItem key={item.code} value={item.code}>
                  {item.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>

        <div className="space-y-1 sm:col-span-2">
          <Label className="text-[9.5px]">Voice</Label>
          <div
            role="radiogroup"
            aria-label="Voice gender"
            className="grid grid-cols-3 gap-1"
          >
            {(
              [
                { value: 'all', label: 'Any' },
                { value: 'female', label: 'Female' },
                { value: 'male', label: 'Male' },
              ] as const
            ).map((option) => (
              <button
                key={option.value}
                type="button"
                role="radio"
                aria-checked={gender === option.value}
                onClick={() => setGender(option.value)}
                className={cn(
                  'border px-2 py-1.5 font-mono text-[10px] uppercase tracking-[0.08em] transition-colors',
                  gender === option.value
                    ? 'border-accent bg-accent-wash text-accent'
                    : 'border-hairline text-ink-muted hover:border-hairline-strong hover:text-ink',
                )}
              >
                {option.label}
              </button>
            ))}
          </div>
        </div>
      </div>

      <div className="flex items-baseline justify-between gap-3 border-t border-hairline pt-2">
        <p className="font-mono text-[10px] uppercase tracking-[0.08em] text-ink-subtle">
          {loading ? 'Loading voices…' : `${filtered.length} voice${filtered.length === 1 ? '' : 's'}`}
        </p>
        {selected && !loading && (
          <p className="truncate text-right text-[11px] text-ink-muted">
            Selected: <span className="text-ink">{voiceDisplayName(selected.name)}</span>
            {' · '}
            {voiceDescription(selected)}
          </p>
        )}
      </div>

      {loading ? (
        <div className="space-y-1.5" aria-hidden>
          {Array.from({ length: 5 }).map((_, index) => (
            <div
              key={index}
              className="h-11 animate-pulse rounded-[2px] border border-hairline bg-surface"
            />
          ))}
        </div>
      ) : filtered.length === 0 ? (
        <p className="border border-dashed border-hairline px-4 py-6 text-center text-[13px] text-ink-muted">
          No voice matches those filters. Try a different language or clear the search.
        </p>
      ) : (
        <ul
          role="radiogroup"
          aria-label="Voice"
          className={cn(
            'overflow-y-auto overscroll-contain border-y border-hairline',
            compact ? 'max-h-56' : 'max-h-80',
          )}
        >
          {filtered.map((voice) => {
            const isSelected = voice.name === value;
            const isPlaying = playing === voice.name;
            const isLoading = loadingPreview === voice.name;
            return (
              <li key={voice.name}>
                <div
                  className={cn(
                    'flex items-center gap-2 border-b border-hairline/70 px-2 py-1.5 last:border-b-0',
                    isSelected ? 'bg-accent-wash' : 'hover:bg-accent-wash/40',
                  )}
                >
                  <button
                    type="button"
                    role="radio"
                    aria-checked={isSelected}
                    onClick={() => onChange(voice)}
                    className="flex min-w-0 flex-1 items-center gap-2.5 py-0.5 text-left focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent"
                  >
                    <span
                      aria-hidden
                      className={cn(
                        'grid size-4 shrink-0 place-items-center rounded-full border transition-colors',
                        isSelected ? 'border-accent bg-accent text-[#fffaf1]' : 'border-hairline-strong',
                      )}
                    >
                      {isSelected && <Check className="size-2.5" />}
                    </span>
                    <span className="min-w-0">
                      <span
                        className={cn(
                          'block truncate text-[13px]',
                          isSelected ? 'font-medium text-ink' : 'text-ink-muted',
                        )}
                      >
                        {voiceDisplayName(voice.name)}
                      </span>
                      <span className="block truncate font-mono text-[9.5px] uppercase tracking-[0.06em] text-ink-subtle">
                        {voiceDescription(voice)}
                      </span>
                    </span>
                  </button>

                  <TooltipWrap label={isPlaying ? 'Stop preview' : `Preview ${voiceDisplayName(voice.name)}`}>
                    <button
                      type="button"
                      onClick={() => void preview(voice)}
                      disabled={isLoading}
                      aria-label={
                        isPlaying
                          ? `Stop preview of ${voice.name}`
                          : `Preview how ${voice.name} sounds`
                      }
                      className={cn(
                        'grid size-8 shrink-0 place-items-center rounded-[2px] border transition-colors',
                        isPlaying
                          ? 'border-accent text-accent'
                          : 'border-hairline text-ink-subtle hover:border-accent hover:text-accent',
                        'disabled:opacity-60',
                      )}
                    >
                      {isLoading ? (
                        <Loader2 className="size-3.5 animate-spin" aria-hidden />
                      ) : isPlaying ? (
                        <Square className="size-3 fill-current" aria-hidden />
                      ) : (
                        <Play className="size-3.5 translate-x-px fill-current" aria-hidden />
                      )}
                    </button>
                  </TooltipWrap>
                </div>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
