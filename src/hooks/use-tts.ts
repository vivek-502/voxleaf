'use client';

import * as React from 'react';

import { useTtsGenerationContext } from '@/components/providers/tts-provider';
import type { TtsJobStatus } from '@/types';

/**
 * Driving a chapter's TTS generation from the browser.
 *
 * The work itself is a server job, and the watch over it lives in
 * `TtsProvider`, above the router outlet. That is what lets a generation
 * started in the reader finish — and say so — after the reader has been closed,
 * instead of leaving a loading toast frozen at its last poll.
 *
 * The hook keeps the per-screen concerns: the busy set every button reads, and
 * an `onDone` that refreshes the screen that asked. Several screens can watch
 * the same chapter without starting a second job or a second toast.
 */

export type { TtsJobStatus };

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
  const { busyChapters, generate } = useTtsGenerationContext();
  // Kept in a ref so the watch is not rebuilt when a caller's onDone changes
  // identity between renders.
  const onDoneRef = React.useRef(onDone);
  React.useEffect(() => {
    onDoneRef.current = onDone;
  }, [onDone]);

  const start = React.useCallback(
    (chapterId: string, label: string, language: string | null = null) =>
      generate(chapterId, label, language, () => onDoneRef.current?.()),
    [generate],
  );

  return { busyChapters, generate: start };
}
