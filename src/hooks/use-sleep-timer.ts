import * as React from 'react';
import { toast } from 'sonner';

import { useSettings } from '@/components/providers/settings-provider';
import { formatDurationShort } from '@/utils/format';
import type { SleepTimerId } from '@/types';

/**
 * Sleep timer state machine.
 *
 * Two modes:
 *  - countdown: pause after N minutes, regardless of position
 *  - end-of-chapter: let the current chapter finish, then pause
 *
 * The countdown is driven by wall-clock time (not a tick count) so it stays
 * accurate if the tab is throttled in the background.
 */

export interface SleepTimerState {
  /** The active selection, or 'off'. */
  active: SleepTimerId;
  /** Seconds remaining, or null for off / end-of-chapter. */
  remaining: number | null;
  /** Set once the timer has been started at least once in this session. */
  enabled: boolean;
  setTimer: (id: SleepTimerId) => void;
  clear: () => void;
  /** Called by the player when a chapter ends. */
  onChapterEnded: () => boolean;
}

const TICK_MS = 1000;

export function useSleepTimer(onExpire: (reason: 'timer' | 'chapter') => void): SleepTimerState {
  const { sleepTimerPreference, setSleepTimerPreference } = useSettings();
  const [active, setActive] = React.useState<SleepTimerId>('off');
  const [deadline, setDeadline] = React.useState<number | null>(null);
  const [remaining, setRemaining] = React.useState<number | null>(null);
  const expireRef = React.useRef(onExpire);
  expireRef.current = onExpire;

  const start = React.useCallback((id: SleepTimerId) => {
    setActive(id);
    setSleepTimerPreference(id);
    if (id === 'off') {
      setDeadline(null);
      setRemaining(null);
      return;
    }
    if (id === 'end-of-chapter') {
      setDeadline(null);
      setRemaining(null);
      toast.message('Sleep timer set', { description: 'Playback will stop at the end of this chapter.' });
      return;
    }
    const minutes = Number(id);
    if (!Number.isFinite(minutes) || minutes <= 0) return;
    const expiresAt = Date.now() + minutes * 60_000;
    setDeadline(expiresAt);
    setRemaining(minutes * 60);
    toast.message(`Sleep timer set for ${minutes} minutes`, {
      description: 'Playback will pause automatically.',
    });
  }, [setSleepTimerPreference]);

  const clear = React.useCallback(() => {
    setActive('off');
    setDeadline(null);
    setRemaining(null);
  }, []);

  React.useEffect(() => {
    if (deadline === null) return;
    const tick = () => {
      const left = Math.max(0, Math.round((deadline - Date.now()) / 1000));
      setRemaining(left);
      if (left <= 0) {
        clear();
        expireRef.current('timer');
      }
    };
    tick();
    const id = setInterval(tick, TICK_MS);
    return () => clearInterval(id);
  }, [deadline, clear]);

  const onChapterEnded = React.useCallback(() => {
    if (active === 'end-of-chapter') {
      clear();
      return true; // the player should pause
    }
    return false;
  }, [active, clear]);

  // When the app is reloaded, honour the stored preference as the starting
  // selection without starting a countdown.
  React.useEffect(() => {
    setActive((current) => (current === 'off' && sleepTimerPreference !== 'off' ? 'off' : current));
  }, [sleepTimerPreference]);

  return {
    active,
    remaining: remaining !== null ? remaining : active === 'end-of-chapter' ? null : null,
    enabled: active !== 'off',
    setTimer: start,
    clear,
    onChapterEnded,
  };
}

/** Label for the timer chip shown in the player. */
export function describeSleepTimer(state: SleepTimerState): string | null {
  if (state.active === 'off') return null;
  if (state.active === 'end-of-chapter') return 'End of chapter';
  if (state.remaining === null) return `${state.active} min`;
  return formatDurationShort(state.remaining);
}
