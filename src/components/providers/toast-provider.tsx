'use client';

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
            'rounded-[3px] border border-hairline-strong bg-raised text-ink shadow-[var(--shadow-lift)] text-sm',
          description: 'text-ink-muted',
          actionButton: 'bg-accent text-[#fffaf1] rounded-[2px] font-mono text-[11px] uppercase tracking-[0.08em]',
          cancelButton: 'bg-overlay text-ink-muted rounded-[2px]',
        },
      }}
    />
  );
}
