import * as React from 'react';
import { ThemeProvider, useTheme } from './theme-provider';
import { PlayerProvider } from './player-provider';
import { SettingsProvider } from './settings-provider';
import { ToastViewport } from './toast-provider';
import { TooltipProvider } from '@/components/ui/tooltip';

import { api } from '@/hooks/use-api';
import type { AppSettingsInfo } from '@/types';

/** Keeps the `<html data-theme>` attribute and the toast theme in sync. */
function ThemeSync() {
  const { theme } = useTheme();
  React.useEffect(() => {
    document.documentElement.dataset.theme = theme;
  }, [theme]);
  return null;
}

const DEFAULT_SETTINGS: AppSettingsInfo = {
  playbackSpeed: 1,
  theme: 'dark',
  volume: 1,
  muted: false,
  sleepTimerPreference: 'off',
  skipForwardSeconds: 30,
  skipBackwardSeconds: 15,
  completionThreshold: 0.95,
  autoAdvance: true,
  ttsVoice: 'en-US-GuyNeural',
};

export interface AppProvidersProps {
  children: React.ReactNode;
  /** Server-rendered settings, so the first paint is already correct. */
  initialSettings?: AppSettingsInfo;
}

/**
 * Root provider stack.
 *
 * Order matters: settings (durable playback preferences) wrap the player,
 * because the player persists speed and volume through them. Both live above
 * the router outlet, which is what keeps audio playing across navigations.
 */
export function AppProviders({ children, initialSettings }: AppProvidersProps) {
  return (
    <ThemeProvider initialTheme={initialSettings?.theme ?? 'dark'}>
      <ThemeSync />
      <SettingsProvider initialSettings={initialSettings ?? DEFAULT_SETTINGS}>
        <PlayerProvider>
          <TooltipProvider delayDuration={250} skipDelayDuration={400}>
            {children}
            <ToastViewport />
          </TooltipProvider>
        </PlayerProvider>
      </SettingsProvider>
    </ThemeProvider>
  );
}

/** Standalone toaster so non-provider code can raise a toast. */
export const toast = {
  async info(message: string) {
    const { toast: sonner } = await import('sonner');
    sonner.info(message);
  },
  success: async (message: string) => {
    const { toast: sonner } = await import('sonner');
    sonner.success(message);
  },
  error: async (message: string) => {
    const { toast: sonner } = await import('sonner');
    sonner.error(message);
  },
};

export { api };
