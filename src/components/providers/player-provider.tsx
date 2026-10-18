import * as React from 'react';
import { toast } from 'sonner';

import { api, errorMessage } from '@/hooks/use-api';
import { useSleepTimer } from '@/hooks/use-sleep-timer';
import { useSettings } from '@/components/providers/settings-provider';
import { clamp, debounce } from '@/utils/format';
import type { PlaybackTrack, SleepTimerId } from '@/types';

/* -------------------------------------------------------------------------- */
/* Types                                                                      */
/* -------------------------------------------------------------------------- */

/** The next chapter, and whether it can actually be played. */
export interface AdjacentChapter {
  id: string;
  chapterNumber: number;
  hasAudio: boolean;
  title: string;
}

export interface PlaybackResolution extends PlaybackTrack {
  next: AdjacentChapter | null;
  previous: AdjacentChapter | null;
}

/** Why playback stopped at the end of a chapter. */
export interface BlockedAdvance {
  bookId: string;
  libraryId: string;
  chapterId: string;
  chapterNumber: number;
  chapterTitle: string;
  nextChapterId: string;
  nextChapterNumber: number;
  nextChapterTitle: string;
  message: string;
}

export type PlayerStatus = 'idle' | 'loading' | 'ready' | 'error';

export interface PlayerContextValue {
  /** The single `<audio>` element, mounted for the whole app session. */
  audioRef: React.RefObject<HTMLAudioElement | null>;
  track: PlaybackTrack | null;
  status: PlayerStatus;
  isPlaying: boolean;
  isLoading: boolean;
  error: string | null;
  position: number;
  duration: number;
  buffered: number;
  playbackRate: number;
  volume: number;
  muted: boolean;
  /** True while the full-screen player is open. */
  isExpanded: boolean;
  /** Set when the next chapter has no audio and playback stopped. */
  blocked: BlockedAdvance | null;

  hasTrack: boolean;
  hasNext: boolean;
  hasPrevious: boolean;
  nextHasAudio: boolean;

  load: (resolution: PlaybackResolution, options?: { autoplay?: boolean }) => Promise<void>;
  play: () => Promise<void>;
  pause: () => void;
  toggle: () => Promise<void>;
  seek: (seconds: number) => void;
  seekTo: (seconds: number) => void;
  skipForward: () => void;
  skipBackward: () => void;
  setPlaybackRate: (rate: number) => void;
  setVolume: (volume: number) => void;
  toggleMute: () => void;
  playChapter: (chapterId: string, bookId?: string) => Promise<void>;
  playNext: () => Promise<void>;
  playPrevious: () => Promise<void>;
  stop: () => void;
  expand: () => void;
  collapse: () => void;
  toggleExpand: () => void;
  setFavorite: (favorite: boolean) => Promise<void>;
  dismissBlocked: () => void;
  sleepTimer: ReturnType<typeof useSleepTimer>;
  setSleepTimer: (id: SleepTimerId) => void;
  /** Called by a chapter row's play button when the chapter has no audio. */
  notifyNoAudio: (chapter: { id: string; chapterNumber: number; title: string; bookId: string; libraryId: string }) => void;
}

/* -------------------------------------------------------------------------- */
/* Context                                                                    */
/* -------------------------------------------------------------------------- */

const PlayerContext = React.createContext<PlayerContextValue | null>(null);

/* -------------------------------------------------------------------------- */
/* Provider                                                                   */
/* -------------------------------------------------------------------------- */

const PROGRESS_FLUSH_MS = 5000;

export function PlayerProvider({ children }: { children: React.ReactNode }) {
  const audioRef = React.useRef<HTMLAudioElement | null>(null);
  const [track, setTrack] = React.useState<PlaybackTrack | null>(null);
  const [status, setStatus] = React.useState<PlayerStatus>('idle');
  const [isPlaying, setIsPlaying] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const [position, setPosition] = React.useState(0);
  const [duration, setDuration] = React.useState(0);
  const [buffered, setBuffered] = React.useState(0);
  const [isExpanded, setIsExpanded] = React.useState(false);
  const [blocked, setBlocked] = React.useState<BlockedAdvance | null>(null);
  const [adjacent, setAdjacent] = React.useState<{ next: AdjacentChapter | null; previous: AdjacentChapter | null }>({
    next: null,
    previous: null,
  });

  const {
    playbackSpeed,
    volume,
    muted,
    autoAdvance,
    setPlaybackSpeed: persistSpeed,
    setVolume: persistVolume,
    toggleMute: persistMute,
  } = useSettings();

  // Refs mirror state for use inside audio event handlers, which close over
  // stale values otherwise.
  const trackRef = React.useRef<PlaybackTrack | null>(null);
  const positionRef = React.useRef(0);
  const durationRef = React.useRef(0);
  const adjacentRef = React.useRef(adjacent);
  const lastSavedRef = React.useRef(0);
  const playCountedRef = React.useRef(false);
  const pendingSeekRef = React.useRef<number | null>(null);
  const autoAdvanceRef = React.useRef(autoAdvance);
  autoAdvanceRef.current = autoAdvance;

  // `loadedmetadata` and `ended` are wired up in the mount effect below but
  // implemented further down, because they close over the navigation helpers.
  // Stable refs let the effect keep an empty dependency array.
  const onLoadedMetadataRef = React.useRef<() => void>(() => undefined);
  const onEndedRef = React.useRef<() => void>(() => undefined);

  adjacentRef.current = adjacent;

  /* ---------------------------------------------------------------------- */
  /* Audio element                                                          */
  /* ---------------------------------------------------------------------- */

  React.useEffect(() => {
    const audio = new Audio();
    // Everything is same-origin and token-gated; do not leak referrer data to
    // the (optional) third-party storage host.
    audio.preload = 'metadata';
    audio.crossOrigin = 'anonymous';
    audioRef.current = audio;

    const onTimeUpdate = () => {
      positionRef.current = audio.currentTime;
      setPosition(audio.currentTime);
      if (audio.buffered.length > 0) {
        setBuffered(audio.buffered.end(audio.buffered.length - 1));
      }
    };
    const onDurationChange = () => {
      const next = Number.isFinite(audio.duration) ? audio.duration : 0;
      durationRef.current = next;
      setDuration(next);
    };
    const onPlay = () => setIsPlaying(true);
    const onPause = () => setIsPlaying(false);
    const onWaiting = () => setStatus('loading');
    const onPlaying = () => setStatus('ready');

    const onError = () => {
      const code = audio.error?.code;
      const message =
        code === MediaError.MEDIA_ERR_NETWORK
          ? 'This audio could not be reached. Check your connection.'
          : code === MediaError.MEDIA_ERR_DECODE
            ? 'This audio format could not be played by your browser.'
            : code === MediaError.MEDIA_ERR_SRC_NOT_SUPPORTED
              ? 'This audio format is not supported by your browser.'
              : 'Playback failed. Please try again.';
      setError(message);
      setStatus('error');
      setIsPlaying(false);
      toast.error(message);
    };

    // `loadedmetadata` / `ended` are declared further down (they need the
    // navigation helpers). Register stable trampolines that dispatch through
    // the refs at event time — registering the ref's value directly would
    // capture the mount-time no-op stub, and this effect never re-runs.
    const onLoadedMetadata = () => onLoadedMetadataRef.current();
    const onEnded = () => onEndedRef.current();

    audio.addEventListener('timeupdate', onTimeUpdate);
    audio.addEventListener('durationchange', onDurationChange);
    audio.addEventListener('loadedmetadata', onLoadedMetadata);
    audio.addEventListener('play', onPlay);
    audio.addEventListener('pause', onPause);
    audio.addEventListener('waiting', onWaiting);
    audio.addEventListener('playing', onPlaying);
    audio.addEventListener('ended', onEnded);
    audio.addEventListener('error', onError);

    return () => {
      audio.removeEventListener('timeupdate', onTimeUpdate);
      audio.removeEventListener('durationchange', onDurationChange);
      audio.removeEventListener('loadedmetadata', onLoadedMetadata);
      audio.removeEventListener('play', onPlay);
      audio.removeEventListener('pause', onPause);
      audio.removeEventListener('waiting', onWaiting);
      audio.removeEventListener('playing', onPlaying);
      audio.removeEventListener('ended', onEnded);
      audio.removeEventListener('error', onError);
      audio.pause();
      audio.src = '';
    };
  }, []);

  /* ---------------------------------------------------------------------- */
  /* Persist listening position                                             */
  /* ---------------------------------------------------------------------- */

  const flushProgress = React.useCallback((force = false) => {
    const current = trackRef.current;
    if (!current) return;
    const now = positionRef.current;
    const total = durationRef.current;
    if (!force && now - lastSavedRef.current < PROGRESS_FLUSH_MS) return;
    lastSavedRef.current = now;
    api
      .put('/api/progress', {
        chapterId: current.chapterId,
        position: Number(now.toFixed(2)),
        duration: Number(total.toFixed(2)),
      })
      .catch(() => {
        // Progress is best-effort; never interrupt playback for a failed write.
      });
  }, []);

  const saveNow = React.useCallback(
    (options: { incrementPlayCount?: boolean } = {}) => {
      const current = trackRef.current;
      if (!current) return;
      lastSavedRef.current = positionRef.current;
      api
        .put('/api/progress', {
          chapterId: current.chapterId,
          position: Number(positionRef.current.toFixed(2)),
          duration: Number(durationRef.current.toFixed(2)),
          ...(options.incrementPlayCount ? { incrementPlayCount: true } : {}),
        })
        .catch(() => undefined);
    },
    [],
  );

  // Periodic checkpoint while playing.
  React.useEffect(() => {
    if (!isPlaying) return;
    const id = setInterval(() => flushProgress(), PROGRESS_FLUSH_MS);
    return () => clearInterval(id);
  }, [isPlaying, flushProgress]);

  // Final checkpoint when the tab is hidden or the app is being closed.
  React.useEffect(() => {
    const onHide = () => {
      if (document.visibilityState === 'hidden') flushProgress(true);
    };
    const onPageHide = () => flushProgress(true);
    document.addEventListener('visibilitychange', onHide);
    window.addEventListener('pagehide', onPageHide);
    return () => {
      document.removeEventListener('visibilitychange', onHide);
      window.removeEventListener('pagehide', onPageHide);
    };
  }, [flushProgress]);

  /* ---------------------------------------------------------------------- */
  /* Engine controls                                                        */
  /* ---------------------------------------------------------------------- */

  const applySettingsToElement = React.useCallback(() => {
    const audio = audioRef.current;
    if (!audio) return;
    audio.playbackRate = playbackSpeed;
    audio.volume = volume;
    audio.muted = muted;
  }, [playbackSpeed, volume, muted]);

  React.useEffect(() => {
    applySettingsToElement();
  }, [applySettingsToElement, track]);

  const play = React.useCallback(async () => {
    const audio = audioRef.current;
    if (!audio || !trackRef.current) return;
    try {
      await audio.play();
    } catch (error) {
      // Autoplay policies reject programmatic play without a user gesture.
      const name = (error as Error).name;
      if (name !== 'AbortError' && name !== 'NotAllowedError') {
        setError('Playback could not start. Please try pressing play again.');
        setStatus('error');
      }
    }
  }, []);

  const pause = React.useCallback(() => {
    audioRef.current?.pause();
    flushProgress(true);
  }, [flushProgress]);

  const toggle = React.useCallback(async () => {
    const audio = audioRef.current;
    if (!audio) return;
    if (audio.paused) await play();
    else pause();
  }, [play, pause]);

  const seekTo = React.useCallback((seconds: number) => {
    const audio = audioRef.current;
    if (!audio) return;
    const total = durationRef.current || audio.duration || 0;
    const next = clamp(seconds, 0, Number.isFinite(total) && total > 0 ? total : Number.MAX_SAFE_INTEGER);
    audio.currentTime = next;
    positionRef.current = next;
    setPosition(next);
  }, []);

  const seek = React.useCallback(
    (seconds: number) => {
      seekTo((audioRef.current?.currentTime ?? 0) + seconds);
    },
    [seekTo],
  );

  const skipForward = React.useCallback(() => seek(30), [seek]);
  const skipBackward = React.useCallback(() => seek(-15), [seek]);

  const changeSpeed = React.useCallback(
    (rate: number) => {
      persistSpeed(rate);
      const audio = audioRef.current;
      if (audio) audio.playbackRate = rate;
    },
    [persistSpeed],
  );

  const changeVolume = React.useCallback(
    (next: number) => {
      persistVolume(next);
      const audio = audioRef.current;
      if (audio) {
        audio.volume = next;
        // Raising the slider on a muted track should unmute it.
        if (next > 0 && audio.muted) persistMute();
      }
    },
    [persistVolume, persistMute],
  );

  const toggleMute = React.useCallback(() => {
    persistMute();
  }, [persistMute]);

  /* ---------------------------------------------------------------------- */
  /* Loading a track                                                        */
  /* ---------------------------------------------------------------------- */

  const load = React.useCallback(
    async (resolution: PlaybackResolution, options: { autoplay?: boolean } = {}) => {
      const audio = audioRef.current;
      if (!audio) return;

      // Leaving a chapter: checkpoint the old one first.
      if (trackRef.current && trackRef.current.chapterId !== resolution.chapterId) {
        flushProgress(true);
      }

      const isSameChapter = trackRef.current?.chapterId === resolution.chapterId;
      if (!isSameChapter) {
        playCountedRef.current = false;
        setBlocked(null);
        pendingSeekRef.current = resolution.startPosition > 0 ? resolution.startPosition : null;
      }

      setTrack(resolution);
      trackRef.current = resolution;
      setAdjacent({ next: resolution.next ?? null, previous: resolution.previous ?? null });
      setError(null);
      setStatus('loading');
      positionRef.current = 0;
      setPosition(0);

      if (audio.src !== resolution.streamUrl) {
        audio.src = resolution.streamUrl;
        audio.load();
      }
      applySettingsToElement();

      if (options.autoplay) {
        await play();
      }
    },
    [applySettingsToElement, flushProgress, play],
  );

  /** Seek to the pending offset once metadata (and therefore duration) exists. */
  const handleLoadedMetadata = React.useCallback(() => {
    const audio = audioRef.current;
    if (!audio) return;
    const total = Number.isFinite(audio.duration) ? audio.duration : 0;
    durationRef.current = total;
    setDuration(total);
    if (pendingSeekRef.current !== null) {
      const target = clamp(pendingSeekRef.current, 0, total > 0 ? total - 0.5 : Number.MAX_SAFE_INTEGER);
      audio.currentTime = target;
      positionRef.current = target;
      setPosition(target);
      pendingSeekRef.current = null;
    }
    setStatus('ready');
  }, []);
  onLoadedMetadataRef.current = handleLoadedMetadata;

  /* ---------------------------------------------------------------------- */
  /* Chapter navigation                                                     */
  /* ---------------------------------------------------------------------- */

  /**
   * Toast + "Import audio" action when the user explicitly asks to play a
   * chapter that has no audio file yet.
   *
   * Declared before `resolveAndPlay` so that hook can list it as a dependency
   * instead of closing over it.
   */
  const notifyNoAudioLocal = React.useCallback(
    (chapter: { id: string; chapterNumber: number; title: string; bookId: string; libraryId: string }) => {
      toast.message(`Chapter ${chapter.chapterNumber} has no audio yet`, {
        description: 'Import an audio file to start listening to it.',
        action: {
          label: 'Import audio',
          onClick: () => {
            window.dispatchEvent(
              new CustomEvent('voxleaf:import-audio', {
                detail: { chapterId: chapter.id, bookId: chapter.bookId, libraryId: chapter.libraryId },
              }),
            );
          },
        },
      });
    },
    [],
  );

  const resolveAndPlay = React.useCallback(
    async (params: { bookId?: string; chapterId?: string; libraryId?: string; autoplay?: boolean }) => {
      setStatus('loading');
      setError(null);
      try {
        const resolution = await api.get<PlaybackResolution & { playable: boolean; reason?: string }>(
          '/api/playback/resolve',
          {
            bookId: params.bookId,
            chapterId: params.chapterId,
            libraryId: params.libraryId,
          },
        );

        if (!('playable' in resolution) || resolution.playable === false) {
          const message = 'This chapter does not have any audio yet.';
          setError(message);
          setStatus('idle');
          if (params.chapterId) {
            notifyNoAudioLocal({
              id: params.chapterId,
              chapterNumber: (resolution as { chapter?: { chapterNumber: number } }).chapter?.chapterNumber ?? 0,
              title: (resolution as { chapter?: { title: string } }).chapter?.title ?? 'Chapter',
              bookId: params.bookId ?? '',
              libraryId: params.libraryId ?? '',
            });
          }
          return;
        }

        if (params.chapterId && trackRef.current?.chapterId === params.chapterId && audioRef.current?.src) {
          // Same chapter, already loaded: just resume.
          if (params.autoplay) await play();
          return;
        }

        await load(resolution as PlaybackResolution, { autoplay: params.autoplay ?? true });
        if (params.autoplay && !playCountedRef.current) {
          playCountedRef.current = true;
          saveNow({ incrementPlayCount: true });
        }
      } catch (caught) {
        setError(errorMessage(caught, 'That chapter could not be loaded.'));
        setStatus('error');
        toast.error(errorMessage(caught, 'That chapter could not be loaded.'));
      }
    },
    [load, play, saveNow, notifyNoAudioLocal],
  );

  const playChapter = React.useCallback(
    async (chapterId: string, bookId?: string) => {
      const current = trackRef.current;
      const targetBook = bookId ?? current?.bookId;
      if (!targetBook) return;
      await resolveAndPlay({ bookId: targetBook, chapterId, autoplay: true });
    },
    [resolveAndPlay],
  );

  const playNext = React.useCallback(async () => {
    const next = adjacentRef.current.next;
    if (!next) {
      toast.message('That is the last chapter.');
      return;
    }
    if (!next.hasAudio) {
      const current = trackRef.current;
      setBlocked({
        bookId: current?.bookId ?? '',
        libraryId: current?.libraryId ?? '',
        chapterId: current?.chapterId ?? '',
        chapterNumber: current?.chapterNumber ?? 0,
        chapterTitle: current?.chapterTitle ?? '',
        nextChapterId: next.id,
        nextChapterNumber: next.chapterNumber,
        nextChapterTitle: next.title,
        message: `Chapter ${next.chapterNumber} doesn’t have audio yet.`,
      });
      return;
    }
    await resolveAndPlay({ bookId: trackRef.current?.bookId, chapterId: next.id, autoplay: true });
  }, [resolveAndPlay]);

  const playPrevious = React.useCallback(async () => {
    const previous = adjacentRef.current.previous;
    if (!previous) {
      // Common player behaviour: restart the current chapter.
      seekTo(0);
      return;
    }
    if (!previous.hasAudio) {
      const current = trackRef.current;
      setBlocked({
        bookId: current?.bookId ?? '',
        libraryId: current?.libraryId ?? '',
        chapterId: current?.chapterId ?? '',
        chapterNumber: current?.chapterNumber ?? 0,
        chapterTitle: current?.chapterTitle ?? '',
        nextChapterId: previous.id,
        nextChapterNumber: previous.chapterNumber,
        nextChapterTitle: previous.title,
        message: `Chapter ${previous.chapterNumber} doesn’t have audio yet.`,
      });
      return;
    }
    await resolveAndPlay({ bookId: trackRef.current?.bookId, chapterId: previous.id, autoplay: true });
  }, [resolveAndPlay, seekTo]);

  /* ---------------------------------------------------------------------- */
  /* Sleep timer                                                            */
  /* ---------------------------------------------------------------------- */

  const sleepTimer = useSleepTimer(
    React.useCallback(
      (reason) => {
        // Both modes end in a pause; the distinction is only in the message.
        if (reason === 'chapter') {
          audioRef.current?.pause();
          flushProgress(true);
        }
      },
      [flushProgress],
    ),
  );

  const handleEnded = React.useCallback(() => {
    flushProgress(true);

    if (sleepTimer.onChapterEnded()) return;

    if (!autoAdvanceRef.current) return;

    const next = adjacentRef.current.next;
    if (!next) return;

    if (!next.hasAudio) {
      const current = trackRef.current;
      setBlocked({
        bookId: current?.bookId ?? '',
        libraryId: current?.libraryId ?? '',
        chapterId: current?.chapterId ?? '',
        chapterNumber: current?.chapterNumber ?? 0,
        chapterTitle: current?.chapterTitle ?? '',
        nextChapterId: next.id,
        nextChapterNumber: next.chapterNumber,
        nextChapterTitle: next.title,
        message: `Chapter ${next.chapterNumber} doesn’t have audio yet.`,
      });
      return;
    }

    void resolveAndPlay({ bookId: trackRef.current?.bookId, chapterId: next.id, autoplay: true });
  }, [flushProgress, resolveAndPlay, sleepTimer]);

  // The `ended` listener is bound once at mount, long before `handleEnded`
  // exists, so the event dispatches through a trampoline (`onEndedRef`).
  // Re-assigning on every render keeps that trampoline pointing at the current
  // handler — and must happen after the declaration above, not during the
  // render pass that reaches it first.
  onEndedRef.current = handleEnded;

  const stop = React.useCallback(() => {
    flushProgress(true);
    const audio = audioRef.current;
    if (!audio) return;
    audio.pause();
    audio.removeAttribute('src');
    audio.load();
    trackRef.current = null;
    setTrack(null);
    setPosition(0);
    setDuration(0);
    setStatus('idle');
    setBlocked(null);
    setAdjacent({ next: null, previous: null });
    setIsExpanded(false);
  }, [flushProgress]);

  const setFavorite = React.useCallback(async (favorite: boolean) => {
    const current = trackRef.current;
    if (!current) return;
    setTrack((existing) => (existing ? { ...existing, favorite } : existing));
    try {
      await api.patch(`/api/library/${current.libraryId}`, { favorite });
    } catch (caught) {
      setTrack((existing) => (existing ? { ...existing, favorite: !favorite } : existing));
      toast.error(errorMessage(caught, 'Could not update your favourite.'));
    }
  }, []);

  /* ---------------------------------------------------------------------- */
  /* Keyboard shortcuts                                                     */
  /* ---------------------------------------------------------------------- */

  React.useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      // `event.target` is an Element for any real keystroke, but it can be
      // `document` or `window` for a programmatically dispatched event. This
      // listener sees every keypress in the app, so a throw here would silently
      // break the player's shortcuts everywhere.
      const target =
        event.target instanceof HTMLElement ? event.target : null;
      // Never hijack keys while the user is typing or in a control that
      // handles its own keyboard interaction.
      if (
        target &&
        (target.isContentEditable ||
          ['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName) ||
          target.getAttribute('role') === 'slider' ||
          target.closest('[role="dialog"]'))
      ) {
        return;
      }
      if (!trackRef.current) return;

      switch (event.key) {
        case ' ':
        case 'k':
          event.preventDefault();
          void toggle();
          break;
        case 'ArrowRight':
          if (event.metaKey || event.ctrlKey) {
            event.preventDefault();
            void playNext();
          } else {
            event.preventDefault();
            seek(10);
          }
          break;
        case 'ArrowLeft':
          if (event.metaKey || event.ctrlKey) {
            event.preventDefault();
            void playPrevious();
          } else {
            event.preventDefault();
            seek(-10);
          }
          break;
        case 'j':
          event.preventDefault();
          skipBackward();
          break;
        case 'l':
          event.preventDefault();
          skipForward();
          break;
        case 'm':
          event.preventDefault();
          toggleMute();
          break;
        case 'Escape':
          if (isExpanded) setIsExpanded(false);
          break;
        default:
          break;
      }
    };

    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [isExpanded, playNext, playPrevious, seek, skipBackward, skipForward, toggle, toggleMute]);

  /* ---------------------------------------------------------------------- */
  /* Value                                                                  */
  /* ---------------------------------------------------------------------- */

  const value = React.useMemo<PlayerContextValue>(
    () => ({
      audioRef,
      track,
      status,
      isPlaying,
      isLoading: status === 'loading',
      error,
      position,
      duration: duration || track?.duration || 0,
      buffered,
      playbackRate: playbackSpeed,
      volume,
      muted,
      isExpanded,
      blocked,
      hasTrack: Boolean(track),
      hasNext: Boolean(adjacent.next),
      hasPrevious: Boolean(adjacent.previous),
      nextHasAudio: Boolean(adjacent.next?.hasAudio),
      load,
      play,
      pause,
      toggle,
      seek,
      seekTo,
      skipForward,
      skipBackward,
      setPlaybackRate: changeSpeed,
      setVolume: changeVolume,
      toggleMute,
      playChapter,
      playNext,
      playPrevious,
      stop,
      expand: () => setIsExpanded(true),
      collapse: () => setIsExpanded(false),
      toggleExpand: () => setIsExpanded((open) => !open),
      setFavorite,
      dismissBlocked: () => setBlocked(null),
      sleepTimer,
      setSleepTimer: sleepTimer.setTimer,
      notifyNoAudio: notifyNoAudioLocal,
    }),
    [
      track, status, isPlaying, error, position, duration, buffered, playbackSpeed, volume, muted,
      isExpanded, blocked, adjacent, load, play, pause, toggle, seek, seekTo, skipForward,
      skipBackward, changeSpeed, changeVolume, toggleMute, playChapter, playNext, playPrevious,
      stop, setFavorite, sleepTimer, notifyNoAudioLocal,
    ],
  );

  return <PlayerContext.Provider value={value}>{children}</PlayerContext.Provider>;
}

export function usePlayer(): PlayerContextValue {
  const context = React.useContext(PlayerContext);
  if (!context) throw new Error('usePlayer must be used inside <PlayerProvider>');
  return context;
}

/** A debounced progress flusher for components that own the position slider. */
export function useDebouncedProgressFlush(fn: (force: boolean) => void) {
  const debounced = React.useMemo(() => debounce(fn, 400), [fn]);
  React.useEffect(() => () => debounced.flush(), [debounced]);
  return debounced;
}
