'use client';

import * as React from 'react';
import { Moon, RotateCcw, Sun, Type } from 'lucide-react';

import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { cn } from '@/utils/cn';
import type { EpubFont, EpubTheme } from '@/types';

/**
 * What a reader may change without disturbing the page.
 *
 * Size, spacing and measure are absent on purpose: pages are cut from a fixed
 * typographic measure, so letting any of the three move would refit every page
 * against the reader's hand instead of the book.
 */
export interface ReaderPreferences {
  theme: EpubTheme;
  fontFamily: EpubFont;
}

export const DEFAULT_PREFERENCES: ReaderPreferences = {
  theme: 'paper',
  fontFamily: 'serif',
};

const THEMES: Array<{ value: EpubTheme; label: string; icon: typeof Sun }> = [
  { value: 'paper', label: 'Paper', icon: Sun },
  { value: 'sepia', label: 'Sepia', icon: Sun },
  { value: 'ink', label: 'Ink', icon: Moon },
  { value: 'night', label: 'Night', icon: Moon },
];

const FONTS: Array<{ value: EpubFont; label: string; sample: string; className: string }> = [
  { value: 'serif', label: 'Serif', sample: 'Aa', className: 'font-serif' },
  { value: 'sans', label: 'Sans', sample: 'Aa', className: 'font-sans' },
  { value: 'mono', label: 'Mono', sample: 'Aa', className: 'font-mono' },
];

/** Theme -> the CSS font stack the `reader-prose` utility reads. */
export const READER_FONT_STACKS: Record<EpubFont, { body: string; heading: string }> = {
  serif: {
    body: "Fraunces, Georgia, 'Iowan Old Style', serif",
    heading: "Fraunces, Georgia, 'Iowan Old Style', serif",
  },
  sans: {
    body: "'Inter Tight', Inter, ui-sans-serif, system-ui, sans-serif",
    heading: "'Inter Tight', Inter, ui-sans-serif, system-ui, sans-serif",
  },
  mono: {
    body: "'IBM Plex Mono', ui-monospace, 'SF Mono', Menlo, monospace",
    heading: "'Inter Tight', Inter, ui-sans-serif, system-ui, sans-serif",
  },
};

export interface ReaderSettingsProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  preferences: ReaderPreferences;
  onChange: (patch: Partial<ReaderPreferences>) => void;
}

/**
 * Appearance panel.
 *
 * Preferences are applied instantly and persisted by the caller, so there is
 * no "apply" button — the page itself is the preview.
 */
export function ReaderSettings({
  open,
  onOpenChange,
  preferences,
  onChange,
}: ReaderSettingsProps) {
  const isDirty =
    preferences.theme !== DEFAULT_PREFERENCES.theme ||
    preferences.fontFamily !== DEFAULT_PREFERENCES.fontFamily;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Type className="size-4" aria-hidden />
            Reading settings
          </DialogTitle>
          <DialogDescription>
            The page keeps the book&rsquo;s own type measure; choose how it is drawn.
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-6 py-2">
          {/* Theme */}
          <section>
            <h3 className="mb-2 font-mono text-[10px] uppercase tracking-[0.12em] text-ink-subtle">
              Theme
            </h3>
            <div className="grid grid-cols-4 gap-2">
              {THEMES.map(({ value, label, icon: Icon }) => {
                const active = preferences.theme === value;
                return (
                  <button
                    key={value}
                    type="button"
                    onClick={() => onChange({ theme: value })}
                    aria-pressed={active}
                    className={cn(
                      'flex flex-col items-center gap-2 border px-2 py-2.5 font-mono text-[10px] uppercase tracking-[0.08em] transition-colors',
                      active
                        ? 'border-accent bg-accent-wash text-ink'
                        : 'border-hairline text-ink-muted hover:border-hairline-strong hover:bg-overlay',
                    )}
                  >
                    <span
                      aria-hidden
                      className="flex size-7 items-center justify-center border border-black/15"
                      style={{
                        background: THEME_SWATCH[value].bg,
                        color: THEME_SWATCH[value].fg,
                      }}
                    >
                      <Icon className="size-3.5" />
                    </span>
                    {label}
                  </button>
                );
              })}
            </div>
          </section>

          {/* Typeface */}
          <section>
            <h3 className="mb-2 font-mono text-[10px] uppercase tracking-[0.12em] text-ink-subtle">
              Typeface
            </h3>
            <div className="grid grid-cols-3 gap-2">
              {FONTS.map(({ value, label, sample, className }) => {
                const active = preferences.fontFamily === value;
                return (
                  <button
                    key={value}
                    type="button"
                    onClick={() => onChange({ fontFamily: value })}
                    aria-pressed={active}
                    className={cn(
                      'flex flex-col items-center gap-1 border py-2.5 font-mono text-[10px] uppercase tracking-[0.08em] transition-colors',
                      active
                        ? 'border-accent bg-accent-wash text-ink'
                        : 'border-hairline text-ink-muted hover:border-hairline-strong hover:bg-overlay',
                    )}
                  >
                    <span className={cn('text-lg leading-none', className)}>{sample}</span>
                    {label}
                  </button>
                );
              })}
            </div>
          </section>

          <div className="flex justify-end gap-2 border-t border-hairline pt-4">
            <Button
              variant="ghost"
              size="sm"
              disabled={!isDirty}
              onClick={() => onChange(DEFAULT_PREFERENCES)}
              icon={<RotateCcw className="size-3.5" aria-hidden />}
            >
              Reset
            </Button>
            <Button variant="secondary" size="sm" onClick={() => onOpenChange(false)}>
              Done
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}

const THEME_SWATCH: Record<EpubTheme, { bg: string; fg: string }> = {
  paper: { bg: '#fdfcf8', fg: '#17161a' },
  sepia: { bg: '#f4e9d4', fg: '#3a2d1c' },
  ink: { bg: '#101014', fg: '#e9e9ee' },
  night: { bg: '#07080f', fg: '#cfd6f5' },
};
