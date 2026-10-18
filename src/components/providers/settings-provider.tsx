import * as React from 'react';
import { toast } from 'sonner';

import { api, errorMessage } from '@/hooks/use-api';
import { clamp } from '@/utils/format';
import { PLAYBACK_SPEEDS } from '@/utils/constants';
import type { AppSettingsInfo, SleepTimerId, ThemeId } from '@/types';
import { useTheme } from './theme-provider';

interface SettingsContextValue extends AppSettingsInfo {
  ready: boolean;
  setPlaybackSpeed: (speed: number) => void;
  setVolume: (volume: number) => void;
  toggleMute: () => void;
  setSleepTimerPreference: (value: SleepTimerId) => void;
  setSkipForwardSeconds: (seconds: number) => void;
  setSkipBackwardSeconds: (seconds: number) => void;
  setCompletionThreshold: (value: number) => void;
  setAutoAdvance: (value: boolean) => void;
  setTtsVoice: (voice: string) => void;
  setTheme: (theme: ThemeId) => void;
  refresh: () => Promise<void>;
}

const SettingsContext = React.createContext<SettingsContextValue | null>(null);

/**
 * Durable playback preferences.
 *
 * Values are written optimistically and persisted to the database, so a
 * restored playback speed or sleep-timer default survives a server restart —
 * not just a browser refresh. The DB is the source of truth; localStorage is
 * only a first-paint hint (applied in the settings page's provider chain).
 */
export function SettingsProvider({
  children,
  initialSettings,
}: {
  children: React.ReactNode;
  initialSettings: AppSettingsInfo;
}) {
  const [settings, setSettings] = React.useState<AppSettingsInfo>(initialSettings);
  const [ready, setReady] = React.useState(false);
  const { setTheme: setUiTheme } = useTheme();
  const saveTimer = React.useRef<ReturnType<typeof setTimeout> | null>(null);

  React.useEffect(() => {
    let cancelled = false;
    // Re-read on mount so a second tab's changes are picked up on navigation.
    api
      .get<AppSettingsInfo>('/api/settings')
      .then((fresh) => {
        if (!cancelled) setSettings(fresh);
      })
      .catch(() => {
        // Keep the server-rendered defaults; the app still works.
      })
      .finally(() => {
        if (!cancelled) setReady(true);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const persist = React.useCallback((patch: Partial<AppSettingsInfo>) => {
    setSettings((current) => ({ ...current, ...patch }));
    if (saveTimer.current) clearTimeout(saveTimer.current);
    saveTimer.current = setTimeout(() => {
      api.put('/api/settings', patch).catch((error) => {
        toast.error(errorMessage(error, 'Could not save your preferences.'));
      });
    }, 400);
  }, []);

  React.useEffect(
    () => () => {
      if (saveTimer.current) clearTimeout(saveTimer.current);
    },
    [],
  );

  const value = React.useMemo<SettingsContextValue>(
    () => ({
      ...settings,
      ready,
      setPlaybackSpeed: (speed) => {
        // Snap to a supported rate so the UI and the engine never disagree.
        let snapped: number = PLAYBACK_SPEEDS[0];
        for (const candidate of PLAYBACK_SPEEDS) {
          if (Math.abs(candidate - speed) < Math.abs(snapped - speed)) snapped = candidate;
        }
        persist({ playbackSpeed: snapped });
      },
      setVolume: (volume) => persist({ volume: clamp(volume, 0, 1) }),
      toggleMute: () => persist({ muted: !settings.muted }),
      setSleepTimerPreference: (sleepTimerPreference) => persist({ sleepTimerPreference }),
      setSkipForwardSeconds: (skipForwardSeconds) => persist({ skipForwardSeconds }),
      setSkipBackwardSeconds: (skipBackwardSeconds) => persist({ skipBackwardSeconds }),
      setCompletionThreshold: (completionThreshold) => persist({ completionThreshold }),
      setAutoAdvance: (autoAdvance) => persist({ autoAdvance }),
      setTtsVoice: (ttsVoice) => persist({ ttsVoice }),
      setTheme: (theme) => {
        setUiTheme(theme);
        persist({ theme });
      },
      refresh: async () => {
        const fresh = await api.get<AppSettingsInfo>('/api/settings');
        setSettings(fresh);
      },
    }),
    [settings, ready, persist, setUiTheme],
  );

  return <SettingsContext.Provider value={value}>{children}</SettingsContext.Provider>;
}

export function useSettings(): SettingsContextValue {
  const context = React.useContext(SettingsContext);
  if (!context) throw new Error('useSettings must be used inside <SettingsProvider>');
  return context;
}
