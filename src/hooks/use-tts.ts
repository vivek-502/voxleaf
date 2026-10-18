import * as React from 'react';
import { toast } from 'sonner';

import { api, errorMessage } from '@/hooks/use-api';
import type { TtsJobStatus } from '@/types';

/**
 * Driving a chapter's TTS generation from the browser.
 *
 * Generation takes minutes, so the server takes the job and returns at once.
 * This hook starts it, then polls for progress until it finishes, reporting both
 * through a toast. Shared by every place that offers the button — the reader's
 * section footer, the chapter row and the upload dialog — so they all behave the
 * same way and none of them can forget to stop polling.
 *
 * The polling loop lives at module level, not in the component, because
 * generation outlasts the page that started it. A per-component timer chain
 * died with its page on navigation — clearing the timers but leaving the toast
 * behind — so the toast froze at its last count while the audio kept being
 * built and the chapter finished elsewhere in the app. As a module the loop
 * survives route changes, two pages narrating the same chapter share one job
 * and one toast, and completion runs the `onDone` of every *mounted* watcher:
 * whichever view is on screen when the audio lands is the one that refreshes.
 *
 * `generate` resolves when the chapter reaches a terminal state, so a caller
 * that needs to know when it can stop showing a spinner can await it instead of
 * tracking a second copy of the same state.
 */

export type { TtsJobStatus };

/** Fast enough to feel live, slow enough not to hammer the API on a long chapter. */
const POLL_INTERVAL_MS = 2000;

interface RunningJob {
  toastId: string | number;
  timer: ReturnType<typeof setTimeout> | null;
  /** Resolves once the chapter reaches a terminal state. */
  done: Promise<void>;
  finish: () => void;
}

/**
 * Chapters being generated right now, app-wide, with the loop watching each.
 * Keyed by chapter so a start from any page joins the job already running
 * instead of double-polling it.
 */
const runningJobs = new Map<string, RunningJob>();

/** Mounted hook instances, re-rendered when `runningJobs` changes. */
const busyListeners = new Set<() => void>();

/** Mounted hook instances' completion callbacks — the views worth refreshing. */
const doneListeners = new Set<() => void | Promise<void>>();

function notifyBusy(): void {
  for (const listener of busyListeners) listener();
}

async function notifyDone(): Promise<void> {
  for (const listener of [...doneListeners]) {
    try {
      await listener();
    } catch {
      // A refresh that fails must not stop the toast from settling.
    }
  }
}

export interface UseTtsGeneration {
  /** Chapters currently generating, so buttons can disable themselves. */
  busyChapters: ReadonlySet<string>;
  /**
   * Start generating, resolving once the chapter has audio (or has failed).
   *
   * Pass the language the chapter is being read in — `null` for the original —
   * so the audio matches what the reader sees. Resolves rather than rejects on
   * failure: the reason has already been shown as a toast, and a rejection here
   * would only produce a second, unhandled one.
   */
  generate: (chapterId: string, label: string, language?: string | null) => Promise<void>;
}

export function useTtsGeneration(onDone?: () => void | Promise<void>): UseTtsGeneration {
  const [busyTick, rerender] = React.useReducer((count: number) => count + 1, 0);
  // Kept in a ref so the poll loop does not have to be rebuilt when a caller's
  // onDone changes identity between renders.
  const onDoneRef = React.useRef(onDone);
  React.useEffect(() => {
    onDoneRef.current = onDone;
  }, [onDone]);

  React.useEffect(() => {
    busyListeners.add(rerender);
    // A stable wrapper so the module loop always reaches the latest callback,
    // and so an unmounted page stops being a completion target.
    const watcher = () => onDoneRef.current?.();
    doneListeners.add(watcher);
    return () => {
      busyListeners.delete(rerender);
      doneListeners.delete(watcher);
    };
  }, [rerender]);

  const generate = React.useCallback(
    async (chapterId: string, label: string, language: string | null = null) => {
      // A job is already watching this chapter: join it rather than starting a
      // second poll — and a second toast — for the same audio.
      const existing = runningJobs.get(chapterId);
      if (existing) {
        await existing.done;
        return;
      }

      let job: TtsJobStatus;
      try {
        job = await api.post<TtsJobStatus>(`/api/chapters/${chapterId}/tts`, { language });
      } catch (error) {
        toast.error(errorMessage(error, 'Could not start generating audio.'));
        return;
      }

      if (job.state === 'failed') {
        toast.error(job.error ?? 'Could not generate audio.');
        return;
      }
      if (job.state === 'done') {
        toast.success(`Audio ready for ${label}.`);
        await notifyDone();
        return;
      }

      // Another page may have started watching while our POST was in flight.
      const raced = runningJobs.get(chapterId);
      if (raced) {
        await raced.done;
        return;
      }

      const toastId = toast.loading(`Narrating ${label}…`, {
        description: job.total > 1 ? `0 of ${job.total} parts` : 'Almost there',
      });

      const entry: RunningJob = {
        toastId,
        timer: null,
        // Replaced below; initialised so `finish` can close over the entry.
        done: Promise.resolve(),
        finish: () => {},
      };
      let resolveDone!: () => void;
      entry.done = new Promise<void>((resolve) => {
        resolveDone = resolve;
      });
      entry.finish = () => {
        if (entry.timer) {
          clearTimeout(entry.timer);
          entry.timer = null;
        }
        if (runningJobs.get(chapterId) === entry) {
          runningJobs.delete(chapterId);
          notifyBusy();
        }
        resolveDone();
      };
      const finish = entry.finish;

      const poll = async (): Promise<void> => {
        let next: TtsJobStatus | null;
        try {
          next = await api.get<TtsJobStatus | null>(`/api/chapters/${chapterId}/tts`);
        } catch (error) {
          toast.error(errorMessage(error, 'Lost contact with the server.'), { id: toastId });
          finish();
          return;
        }

        // The job is gone — a server restart dropped it. Say so rather than
        // spinning forever on a generation that is no longer running.
        if (!next) {
          toast.error('Generation was interrupted. Please try again.', { id: toastId });
          finish();
          return;
        }

        if (next.state === 'failed') {
          toast.error(next.error ?? 'Could not generate audio.', { id: toastId });
          finish();
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
              id: toastId,
            });
          } else {
            toast.success(
              note
                ? `Audio ready for ${label} in ${narration?.languageName ?? 'another language'}.`
                : `Audio ready for ${label}${narration?.languageName ? ` (${narration.languageName})` : ''}.`,
              { id: toastId },
            );
          }
          finish();
          await notifyDone();
          return;
        }

        const parts =
          next.total > 1 ? `${next.completed} of ${next.total} parts` : 'Almost there';
        const via = next.narration?.languageName;
        toast.loading(`Narrating ${label}…`, {
          id: toastId,
          description: via ? `${via} · ${parts}` : parts,
        });

        entry.timer = setTimeout(() => void poll(), POLL_INTERVAL_MS);
      };

      runningJobs.set(chapterId, entry);
      notifyBusy();
      entry.timer = setTimeout(() => void poll(), POLL_INTERVAL_MS);

      // Resolved when the loop above reaches a terminal state, so the caller
      // can await completion instead of mirroring the busy set.
      await entry.done;
    },
    [],
  );

  const busyChapters = React.useMemo(() => new Set(runningJobs.keys()), [busyTick]);

  return { busyChapters, generate };
}
