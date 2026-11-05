'use client';

import * as React from 'react';
import { toast } from 'sonner';

import { ApiError, api, errorMessage } from '@/hooks/use-api';
import { useSettings } from './settings-provider';
import { voiceDisplayName } from '@/utils/voice';
import type { TtsJobStatus, TtsVoice } from '@/types';

/**
 * Chapter narration, followed from anywhere in the app.
 *
 * Synthesis is a server job that takes minutes. It used to be watched by the
 * component that started it, which meant leaving the reader in the middle of a
 * chapter stopped the polling but not the job: the loading toast stayed at
 * "3 of 14 parts" forever, because nothing was left to update or dismiss it.
 *
 * The watcher therefore lives above the router outlet. Navigation does not end
 * it, every screen sees the same busy chapters, and the toast always reaches an
 * outcome — ready, failed, or interrupted.
 */

const POLL_INTERVAL_MS = 2000;

type Completion = () => void | Promise<void>;

interface Watch {
  timer: ReturnType<typeof setTimeout> | null;
  toastId: string | number;
  /** Called once when the job ends, whoever asked for the generation. */
  completions: Set<Completion>;
  /** Resolves the promises of callers awaiting this chapter. */
  waiters: Set<() => void>;
}

export interface TtsGeneration {
  busyChapters: ReadonlySet<string>;
  /**
   * Start generating, resolving once the chapter has audio (or has failed).
   *
   * `language` selects the text — `null` for the original — and `onDone` runs
   * when the job finishes, whichever screen is showing at the time.
   */
  generate: (
    chapterId: string,
    label: string,
    language?: string | null,
    onDone?: Completion,
  ) => Promise<void>;
}

const TtsContext = React.createContext<TtsGeneration | null>(null);

export function TtsProvider({ children }: { children: React.ReactNode }) {
  const settings = useSettings();
  const [busy, setBusy] = React.useState<ReadonlySet<string>>(() => new Set());
  const watches = React.useRef(new Map<string, Watch>());

  const stop = React.useCallback((chapterId: string) => {
    const watch = watches.current.get(chapterId);
    if (!watch) return;
    if (watch.timer) clearTimeout(watch.timer);
    watches.current.delete(chapterId);
    setBusy((current) => {
      if (!current.has(chapterId)) return current;
      const next = new Set(current);
      next.delete(chapterId);
      return next;
    });
  }, []);

  /**
   * End a watch and tell everyone waiting.
   *
   * Completion callbacks run after the watch is removed, so a callback that
   * starts another generation for the same chapter — the voice-mismatch retry
   * does — finds a clean slate.
   */
  const settle = React.useCallback(
    (chapterId: string) => {
      const watch = watches.current.get(chapterId);
      if (!watch) return;
      stop(chapterId);
      for (const resolve of watch.waiters) resolve();
      for (const completion of watch.completions) {
        void Promise.resolve()
          .then(completion)
          .catch(() => undefined);
      }
    },
    [stop],
  );

  // A closing tab has nothing left to report, but a timer must not outlive it.
  React.useEffect(() => {
    const current = watches.current;
    return () => {
      for (const watch of current.values()) {
        if (watch.timer) clearTimeout(watch.timer);
      }
      current.clear();
    };
  }, []);

  const generateRef = React.useRef<TtsGeneration['generate'] | null>(null);

  const generate = React.useCallback(
    async (
      chapterId: string,
      label: string,
      language: string | null = null,
      onDone?: Completion,
    ) => {
      // Already watching this chapter: join the existing watch. Pressing
      // Narrate again from another screen must not open a second toast for the
      // same job, and the server would only return the running one anyway.
      const existing = watches.current.get(chapterId);
      if (existing) {
        if (onDone) existing.completions.add(onDone);
        await new Promise<void>((resolve) => existing.waiters.add(resolve));
        return;
      }

      let job: TtsJobStatus;
      try {
        job = await api.post<TtsJobStatus>(`/api/chapters/${chapterId}/tts`, { language });
      } catch (error) {
        // The server refuses rather than reading Hindi with an English voice.
        // Offer the compatible voices it sent back, and switch on the spot.
        if (error instanceof ApiError && error.code === 'voice_language_mismatch') {
          const details = (error.details ?? {}) as {
            languageName?: string;
            suggestions?: TtsVoice[];
          };
          const suggestion = details.suggestions?.[0];
          if (suggestion) {
            toast.error(error.message, {
              duration: 15_000,
              description: `Switch to a ${details.languageName ?? 'compatible'} voice and try again.`,
              action: {
                label: `Use ${voiceDisplayName(suggestion.name)}`,
                onClick: () => {
                  void (async () => {
                    try {
                      // Saved before retrying: the next request reads the
                      // voice from the database, not from this tab.
                      await api.put('/api/settings', { ttsVoice: suggestion.name });
                      settings.setTtsVoice(suggestion.name);
                      toast.success(`Voice changed to ${voiceDisplayName(suggestion.name)}.`);
                      await generateRef.current?.(chapterId, label, language, onDone);
                    } catch (caught) {
                      toast.error(errorMessage(caught, 'Could not change the voice.'));
                    }
                  })();
                },
              },
            });
            return;
          }
        }
        toast.error(errorMessage(error, 'Could not start generating audio.'));
        return;
      }

      if (job.state === 'failed') {
        toast.error(job.error ?? 'Could not generate audio.');
        return;
      }
      if (job.state === 'done') {
        toast.success(`Audio ready for ${label}.`);
        await onDone?.();
        return;
      }

      const watch: Watch = {
        timer: null,
        toastId: toast.loading(`Narrating ${label}…`, {
          description: job.total > 1 ? `0 of ${job.total} parts` : 'Almost there',
        }),
        completions: new Set(onDone ? [onDone] : []),
        waiters: new Set(),
      };
      watches.current.set(chapterId, watch);
      setBusy((current) => new Set(current).add(chapterId));

      await new Promise<void>((resolve) => {
        watch.waiters.add(resolve);

        const poll = async (): Promise<void> => {
          // A newer watch replaced this one (or the provider is tearing down):
          // its toast and its timers belong to it now.
          if (watches.current.get(chapterId) !== watch) return;

          let next: TtsJobStatus | null;
          try {
            next = await api.get<TtsJobStatus | null>(`/api/chapters/${chapterId}/tts`);
          } catch (error) {
            // A terminal toast keeps the description it was given here; without
            // one it would still read "3 of 14 parts" and look stuck.
            toast.error(errorMessage(error, 'Lost contact with the server.'), {
              id: watch.toastId,
              description: 'Check your connection; the job may still be running.',
            });
            settle(chapterId);
            return;
          }

          // The job is gone — a server restart dropped it. Say so rather than
          // spinning forever on a generation that is no longer running.
          if (!next) {
            toast.error('Generation was interrupted. Please try again.', {
              id: watch.toastId,
              description: 'Open the section menu to restart it.',
            });
            settle(chapterId);
            return;
          }

          if (next.state === 'failed') {
            toast.error(next.error ?? 'Could not generate audio.', {
              id: watch.toastId,
              description: 'Open the section menu to try again.',
            });
            settle(chapterId);
            return;
          }

          if (next.state === 'done') {
            // Say what was actually narrated: a translation that was missing or
            // stale falls back to the original, and silently serving English for
            // a chapter being read in Hindi is exactly the bug worth reporting.
            const narration = next.narration;
            const note = narration?.reason;
            if (narration?.fellBackToOriginal) {
              toast.warning(note ?? `Audio ready for ${label}, in the original language.`, {
                id: watch.toastId,
                description: 'The saved translation was not used.',
              });
            } else {
              toast.success(
                note
                  ? `Audio ready for ${label} in ${narration?.languageName ?? 'another language'}.`
                  : `Audio ready for ${label}${narration?.languageName ? ` (${narration.languageName})` : ''}.`,
                {
                  id: watch.toastId,
                  description: 'Ready in the player.',
                },
              );
            }
            settle(chapterId);
            return;
          }

          const parts =
            next.total > 1 ? `${next.completed} of ${next.total} parts` : 'Almost there';
          const via = next.narration?.languageName;
          toast.loading(`Narrating ${label}…`, {
            id: watch.toastId,
            description: via ? `${via} · ${parts}` : parts,
          });

          watch.timer = setTimeout(() => void poll(), POLL_INTERVAL_MS);
        };

        watch.timer = setTimeout(() => void poll(), POLL_INTERVAL_MS);
      });
    },
    [settings, settle],
  );

  React.useEffect(() => {
    generateRef.current = generate;
  }, [generate]);

  const value = React.useMemo<TtsGeneration>(
    () => ({ busyChapters: busy, generate }),
    [busy, generate],
  );

  return <TtsContext.Provider value={value}>{children}</TtsContext.Provider>;
}

export function useTtsGenerationContext(): TtsGeneration {
  const context = React.useContext(TtsContext);
  if (!context) {
    throw new Error('useTtsGeneration must be used inside TtsProvider.');
  }
  return context;
}
