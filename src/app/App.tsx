import * as React from 'react';

import { AppProviders } from '@/components/providers/app-providers';
import { AppShell } from '@/components/layout/app-shell';
import { PageSkeleton } from '@/components/common/states';
import { useApiQuery } from '@/hooks/use-async';
import { AppRoutes } from './routes';

import type { AppSettingsInfo } from '@/types';

/**
 * The desktop root layout.
 *
 * Durable preferences (speed, volume, theme) are resolved before the shell is
 * mounted, exactly as the web app rendered them server-side: it means the very
 * first paint already matches the user's setup instead of flashing defaults and
 * then correcting itself. A settings failure is non-fatal — the app still comes
 * up on sensible defaults, and the player will re-read them when it saves.
 */
export function App() {
  const { data: settings, loading } = useApiQuery<AppSettingsInfo>('/settings');

  if (loading) return <PageSkeleton />;

  return (
    <AppProviders initialSettings={settings ?? undefined}>
      <AppShell>
        <AppRoutes />
      </AppShell>
    </AppProviders>
  );
}
