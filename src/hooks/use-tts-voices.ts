'use client';

import * as React from 'react';

import { api } from '@/hooks/use-api';
import type { TtsVoice } from '@/types';

/**
 * Every voice Edge TTS offers, loaded once per page.
 *
 * The list is a few hundred entries and it is read by more than one surface —
 * the settings picker, the narrate chooser, and the "this voice cannot read
 * that" panel — so a per-component fetch would download it repeatedly for the
 * same answer. A module-level cache means the second caller renders instantly;
 * `reload` exists for the case where the first attempt failed.
 */

let cached: TtsVoice[] | null = null;
let pending: Promise<TtsVoice[]> | null = null;

function loadVoices(): Promise<TtsVoice[]> {
  if (cached) return Promise.resolve(cached);
  if (!pending) {
    pending = api
      .get<TtsVoice[]>('/api/tts/voices')
      .then((voices) => {
        cached = Array.isArray(voices) ? voices : [];
        return cached;
      })
      .finally(() => {
        pending = null;
      });
  }
  return pending;
}

export interface UseTtsVoices {
  voices: TtsVoice[];
  loading: boolean;
  failed: boolean;
  /** Forget the cached list and ask again. */
  reload: () => void;
}

export function useTtsVoices(): UseTtsVoices {
  const [voices, setVoices] = React.useState<TtsVoice[]>(() => cached ?? []);
  const [loading, setLoading] = React.useState(cached === null);
  const [failed, setFailed] = React.useState(false);
  const [attempt, setAttempt] = React.useState(0);

  React.useEffect(() => {
    let cancelled = false;
    if (cached) {
      setVoices(cached);
      setLoading(false);
      setFailed(false);
      return;
    }

    setLoading(true);
    loadVoices()
      .then((list) => {
        if (cancelled) return;
        setVoices(list);
        setFailed(false);
      })
      .catch(() => {
        if (cancelled) return;
        setVoices([]);
        setFailed(true);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, [attempt]);

  const reload = React.useCallback(() => {
    cached = null;
    pending = null;
    setFailed(false);
    setAttempt((value) => value + 1);
  }, []);

  return { voices, loading, failed, reload };
}
