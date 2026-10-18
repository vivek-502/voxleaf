import * as React from 'react';
import { Toaster } from 'sonner';

import { useTheme } from './theme-provider';

/**
 * Toast host. Themed to match the surface palette rather than Sonner's
 * defaults so notifications feel part of the app.
 */
export function ToastViewport() {
  const { resolved } = useTheme();
  return (
    <Toaster
      theme={resolved}
      position="bottom-center"
      offset={96}
      duration={4200}
      closeButton
      toastOptions={{
        classNames: {
          toast:
            'rounded-2xl border border-hairline bg-surface text-ink shadow-[var(--shadow-lift)] text-sm',
          description: 'text-ink-muted',
          actionButton: 'bg-accent text-white rounded-full',
          cancelButton: 'bg-overlay text-ink-muted rounded-full',
        },
      }}
    />
  );
}
