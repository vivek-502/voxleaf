import * as React from 'react';
import {
  AlignJustify,
  Columns2,
  Moon,
  Minus,
  Plus,
  RotateCcw,
  Sun,
  Type,
} from 'lucide-react';

import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Slider } from '@/components/ui/slider';
import { cn } from '@/utils/cn';
import type { EpubFont, EpubTheme } from '@/types';

export interface ReaderPreferences {
  fontSize: number;
  lineHeight: number;
  columnWidth: number;
  theme: EpubTheme;
  fontFamily: EpubFont;
}

export const DEFAULT_PREFERENCES: ReaderPreferences = {
  fontSize: 19,
  lineHeight: 1.7,
  columnWidth: 680,
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
    body: "Georgia, 'Iowan Old Style', 'Times New Roman', serif",
    heading: "Georgia, 'Iowan Old Style', 'Times New Roman', serif",
  },
  sans: {
    body: "'Inter var', Inter, ui-sans-serif, system-ui, sans-serif",
    heading: "'Inter var', Inter, ui-sans-serif, system-ui, sans-serif",
  },
  mono: {
    body: "ui-monospace, 'SF Mono', 'JetBrains Mono', Menlo, monospace",
    heading: "'Inter var', Inter, ui-sans-serif, system-ui, sans-serif",
  },
};

export interface ReaderSettingsProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  preferences: ReaderPreferences;
  onChange: (patch: Partial<ReaderPreferences>) => void;
}

/**
 * Typography panel.
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
    preferences.fontSize !== DEFAULT_PREFERENCES.fontSize ||
    preferences.lineHeight !== DEFAULT_PREFERENCES.lineHeight ||
    preferences.columnWidth !== DEFAULT_PREFERENCES.columnWidth ||
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
          <DialogDescription>Changes apply as you adjust them.</DialogDescription>
        </DialogHeader>

        <div className="space-y-6 py-2">
          {/* Theme */}
          <section>
            <h3 className="mb-2 text-xs font-semibold uppercase tracking-wider text-ink-subtle">
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
                      'flex flex-col items-center gap-1.5 rounded-xl border p-2.5 text-xs font-medium transition-colors',
                      active
                        ? 'border-accent bg-accent-wash text-ink'
                        : 'border-hairline text-ink-muted hover:bg-overlay',
                    )}
                  >
                    <span
                      aria-hidden
                      className="flex size-7 items-center justify-center rounded-md border border-black/10"
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
            <h3 className="mb-2 text-xs font-semibold uppercase tracking-wider text-ink-subtle">
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
                      'flex flex-col items-center gap-0.5 rounded-xl border py-2.5 text-xs font-medium transition-colors',
                      active
                        ? 'border-accent bg-accent-wash text-ink'
                        : 'border-hairline text-ink-muted hover:bg-overlay',
                    )}
                  >
                    <span className={cn('text-lg leading-none', className)}>{sample}</span>
                    {label}
                  </button>
                );
              })}
            </div>
          </section>

          {/* Text size */}
          <section>
            <div className="mb-2 flex items-baseline justify-between">
              <h3 className="text-xs font-semibold uppercase tracking-wider text-ink-subtle">
                Text size
              </h3>
              <span className="text-xs tabular-nums text-ink-subtle">{preferences.fontSize}px</span>
            </div>
            <div className="flex items-center gap-3">
              <Button
                variant="outline"
                size="icon"
                aria-label="Decrease text size"
                disabled={preferences.fontSize <= 13}
                onClick={() => onChange({ fontSize: preferences.fontSize - 1 })}
                icon={<Minus className="size-4" aria-hidden />}
              />
              <Slider
                className="flex-1"
                min={13}
                max={30}
                step={1}
                value={[preferences.fontSize]}
                onValueChange={([value]) => onChange({ fontSize: value ?? preferences.fontSize })}
                aria-label="Text size in pixels"
              />
              <Button
                variant="outline"
                size="icon"
                aria-label="Increase text size"
                disabled={preferences.fontSize >= 30}
                onClick={() => onChange({ fontSize: preferences.fontSize + 1 })}
                icon={<Plus className="size-4" aria-hidden />}
              />
            </div>
          </section>

          {/* Line spacing */}
          <section>
            <div className="mb-2 flex items-baseline justify-between">
              <h3 className="text-xs font-semibold uppercase tracking-wider text-ink-subtle">
                Line spacing
              </h3>
              <span className="text-xs tabular-nums text-ink-subtle">
                {preferences.lineHeight.toFixed(2)}
              </span>
            </div>
            <div className="flex items-center gap-3">
              <AlignJustify className="size-4 shrink-0 text-ink-subtle" aria-hidden />
              <Slider
                className="flex-1"
                min={1.3}
                max={2.2}
                step={0.05}
                value={[preferences.lineHeight]}
                onValueChange={([value]) => onChange({ lineHeight: value ?? preferences.lineHeight })}
                aria-label="Line spacing"
              />
              <AlignJustify className="size-5 shrink-0 text-ink-subtle" aria-hidden />
            </div>
          </section>

          {/* Column width */}
          <section>
            <div className="mb-2 flex items-baseline justify-between">
              <h3 className="text-xs font-semibold uppercase tracking-wider text-ink-subtle">
                Page width
              </h3>
              <span className="text-xs tabular-nums text-ink-subtle">{preferences.columnWidth}px</span>
            </div>
            <div className="flex items-center gap-3">
              <Columns2 className="size-3.5 shrink-0 text-ink-subtle" aria-hidden />
              <Slider
                className="flex-1"
                min={420}
                max={1000}
                step={20}
                value={[preferences.columnWidth]}
                onValueChange={([value]) => onChange({ columnWidth: value ?? preferences.columnWidth })}
                aria-label="Page width in pixels"
              />
              <Columns2 className="size-5 shrink-0 text-ink-subtle" aria-hidden />
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
