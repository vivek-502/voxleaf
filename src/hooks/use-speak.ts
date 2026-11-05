'use client';

import * as React from 'react';
import { toast } from 'sonner';

import { errorMessage } from '@/hooks/use-api';

/**
 * Read a selection or a word aloud.
 *
 * The audio comes from the server's Edge voice — the one chapters are narrated
 * with — rather than the browser's `speechSynthesis`, which only has whatever
 * voices the visitor installed. A Hindi word read by an English-only set comes
 * out as noise, and on many systems there is no voice at all.
 *
 * `speechSynthesis` is still the fallback when the server cannot be reached, so
 * a reader offline still hears something. Clips are cached by text and language
 * because "Say it" on the same word is a normal thing to do twice.
 */

/** A silent frame used to unlock the audio element inside the click gesture. */
const SILENT_CLIP =
  'data:audio/wav;base64,UklGRiQAAABXQVZFZm10IBAAAAABAAEARKwAAIhYAQACABAAZGF0YQAAAAA=';

const MAX_CHARS = 1500;
const MAX_CACHED_CLIPS = 24;

export interface Speaker {
  /** Read `text`; a new call interrupts whatever is already playing. */
  speak: (text: string, language?: string | null) => void;
  /** Stop playback and any request still in flight. */
  stop: () => void;
  /** True while a clip is loading or playing. */
  speaking: boolean;
}

export function useSpeak(): Speaker {
  const [speaking, setSpeaking] = React.useState(false);
  const audioRef = React.useRef<HTMLAudioElement | null>(null);
  const abortRef = React.useRef<AbortController | null>(null);
  const cacheRef = React.useRef(new Map<string, string>());
  const unlockedRef = React.useRef(false);
  // Every clip gets a number; a late response or an `ended` from the previous
  // clip cannot touch the state of the one playing now.
  const seqRef = React.useRef(0);

  const stop = React.useCallback(() => {
    seqRef.current += 1;
    abortRef.current?.abort();
    abortRef.current = null;
    const audio = audioRef.current;
    if (audio) {
      audio.pause();
      audio.removeAttribute('src');
    }
    if (typeof window !== 'undefined' && 'speechSynthesis' in window) {
      window.speechSynthesis.cancel();
    }
    setSpeaking(false);
  }, []);

  const play = React.useCallback((url: string, seq: number) => {
    let audio = audioRef.current;
    if (!audio) {
      audio = new Audio();
      audio.preload = 'auto';
      audioRef.current = audio;
    }
    audio.onended = () => {
      if (seqRef.current === seq) setSpeaking(false);
    };
    audio.onerror = () => {
      if (seqRef.current !== seq) return;
      setSpeaking(false);
      toast.error('That clip could not be played.');
    };
    audio.src = url;
    setSpeaking(true);
    void audio.play().catch(() => {
      if (seqRef.current !== seq) return;
      setSpeaking(false);
      toast.error('Your browser blocked playback. Tap again to hear it.');
    });
  }, []);

  const speak = React.useCallback(
    (raw: string, language?: string | null) => {
      const text = raw.replace(/\s+/g, ' ').trim().slice(0, MAX_CHARS);
      if (!text) return;

      stop();
      const seq = seqRef.current;

      // Unlock playback while the click's user activation is still valid: the
      // real clip arrives after a round trip to the voice service, by which
      // time some browsers have forgotten the tap.
      let audio = audioRef.current;
      if (!audio) {
        audio = new Audio();
        audio.preload = 'auto';
        audioRef.current = audio;
      }
      if (!unlockedRef.current) {
        unlockedRef.current = true;
        audio.src = SILENT_CLIP;
        void audio.play().catch(() => undefined);
      }

      const key = `${language ?? ''}|${text}`;
      const cache = cacheRef.current;
      const cached = cache.get(key);
      if (cached) {
        // Refresh its place in the insertion order.
        cache.delete(key);
        cache.set(key, cached);
        play(cached, seq);
        return;
      }

      const controller = new AbortController();
      abortRef.current = controller;
      setSpeaking(true);

      void (async () => {
        try {
          const response = await fetch('/api/tts/speak', {
            method: 'POST',
            headers: { 'content-type': 'application/json' },
            body: JSON.stringify({ text, ...(language ? { language } : {}) }),
            signal: controller.signal,
          });
          if (!response.ok) {
            const body = (await response.json().catch(() => null)) as {
              error?: { message?: string };
            } | null;
            throw new Error(body?.error?.message ?? 'That could not be read aloud.');
          }

          const blob = await response.blob();
          if (seqRef.current !== seq) return;

          const url = URL.createObjectURL(blob);
          cache.set(key, url);
          while (cache.size > MAX_CACHED_CLIPS) {
            const oldest = cache.keys().next().value;
            if (oldest === undefined) break;
            const oldUrl = cache.get(oldest);
            cache.delete(oldest);
            if (oldUrl) URL.revokeObjectURL(oldUrl);
          }
          play(url, seq);
        } catch (error) {
          if ((error as Error).name === 'AbortError' || seqRef.current !== seq) return;
          setSpeaking(false);
          if (!browserSpeak(text, language)) {
            toast.error(errorMessage(error, 'That could not be read aloud.'));
          }
        } finally {
          if (abortRef.current === controller) abortRef.current = null;
        }
      })();
    },
    [play, stop],
  );

  React.useEffect(() => {
    const cache = cacheRef.current;
    return () => {
      for (const url of cache.values()) URL.revokeObjectURL(url);
      cache.clear();
    };
  }, []);

  return { speak, stop, speaking };
}

/** The browser's own voice, for when the server is unreachable or offline. */
function browserSpeak(text: string, language?: string | null): boolean {
  if (typeof window === 'undefined' || !('speechSynthesis' in window)) return false;
  const utterance = new SpeechSynthesisUtterance(text);
  utterance.lang = language || 'en';
  window.speechSynthesis.cancel();
  window.speechSynthesis.speak(utterance);
  return true;
}
