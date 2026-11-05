'use client';

import * as React from 'react';
import { Monitor, Moon, Sun, Trash2 } from 'lucide-react';

import { PageHeader } from '@/components/common/section-header';
import { Button } from '@/components/ui/button';
import { Field, FieldDescription, Input, Label } from '@/components/ui/inputs';
import { Slider } from '@/components/ui/slider';
import { Switch } from '@/components/ui/switch';
import { ConfirmDialog } from '@/components/common/confirm-dialog';
import { VoicePicker } from '@/components/tts/voice-picker';
import { useSettings } from '@/components/providers/settings-provider';
import { useTheme } from '@/components/providers/theme-provider';
import { api, errorMessage } from '@/hooks/use-api';
import { toast } from 'sonner';
import { PLAYBACK_SPEEDS, SLEEP_TIMER_OPTIONS } from '@/utils/constants';
import { cn } from '@/utils/cn';
import type { ThemeId } from '@/types';

/**
 * Durable preferences.
 *
 * Every control writes through the settings provider, which persists to the
 * database. Nothing here is localStorage-only, so a restored playback speed or
 * volume survives a server restart — not just a browser refresh.
 */
export function SettingsPage() {
  const settings = useSettings();
  const { theme, setTheme } = useTheme();
  const [resetOpen, setResetOpen] = React.useState(false);

  // Handed to `ConfirmDialog.onConfirm`, which shows its own busy state.
  const clearAllData = async () => {
    try {
      await api.delete('/api/library');
      toast.success('All library data has been cleared.');
      window.location.href = '/';
    } catch (error) {
      toast.error(errorMessage(error, 'Could not clear the library.'));
    }
  };

  return (
    <div className="container-page max-w-4xl py-8 sm:py-10">
      <PageHeader
        eyebrow="The press"
        title="Settings"
        description="Playback preferences are saved to the database and apply immediately."
      />

      <div className="mt-2">
        <Section index="01" title="Appearance" description="How VoiceBook looks. Remembered across visits.">
          <div className="grid grid-cols-3 gap-2" role="radiogroup" aria-label="Theme">
            {(
              [
                { value: 'light', label: 'Light', icon: Sun },
                { value: 'dark', label: 'Dark', icon: Moon },
                { value: 'system', label: 'System', icon: Monitor },
              ] as const
            ).map((option) => {
              const Icon = option.icon;
              const active = theme === option.value;
              return (
                <button
                  key={option.value}
                  type="button"
                  role="radio"
                  aria-checked={active}
                  onClick={() => setTheme(option.value as ThemeId)}
                  className={cn(
                    'flex flex-col items-center gap-2 border px-3 py-4 font-mono text-[10px] uppercase tracking-[0.1em] transition-colors',
                    active
                      ? 'border-accent bg-accent-wash text-accent'
                      : 'border-hairline text-ink-muted hover:border-hairline-strong hover:text-ink',
                  )}
                >
                  <Icon className="size-4" aria-hidden />
                  {option.label}
                </button>
              );
            })}
          </div>
        </Section>

        <Section index="02" title="Playback" description="Defaults applied to every chapter you start.">
          <div className="space-y-7">
            <div>
              <div className="flex items-baseline justify-between">
                <Label htmlFor="settings-speed">Default playback speed</Label>
                <span className="font-mono text-[11px] text-accent tabular-nums">
                  {settings.playbackSpeed}×
                </span>
              </div>
              <div className="mt-3 flex flex-wrap gap-1.5" role="radiogroup" aria-label="Playback speed">
                {PLAYBACK_SPEEDS.map((speed) => (
                  <button
                    key={speed}
                    type="button"
                    role="radio"
                    aria-checked={settings.playbackSpeed === speed}
                    onClick={() => settings.setPlaybackSpeed(speed)}
                    className={cn(
                      'min-w-12 border px-2.5 py-1.5 font-mono text-[11px] tabular-nums transition-colors',
                      settings.playbackSpeed === speed
                        ? 'border-accent bg-accent text-[#fffaf1]'
                        : 'border-hairline text-ink-muted hover:border-hairline-strong hover:text-ink',
                    )}
                  >
                    {speed}×
                  </button>
                ))}
              </div>
            </div>

            <div>
              <div className="flex items-baseline justify-between">
                <Label htmlFor="settings-volume">Default volume</Label>
                <span className="font-mono text-[11px] text-ink-subtle tabular-nums">
                  {Math.round(settings.volume * 100)}%
                </span>
              </div>
              <Slider
                id="settings-volume"
                value={[settings.volume * 100]}
                max={100}
                step={1}
                onValueChange={([value]) => settings.setVolume((value ?? 0) / 100)}
                aria-label="Default volume"
                className="mt-3"
              />
            </div>

            <div className="flex items-center justify-between gap-4 border-t border-hairline pt-5">
              <div>
                <Label htmlFor="settings-muted">Start muted</Label>
                <FieldDescription>Begin playback with the volume muted.</FieldDescription>
              </div>
              <Switch
                id="settings-muted"
                checked={settings.muted}
                onCheckedChange={settings.toggleMute}
              />
            </div>

            <div className="flex items-center justify-between gap-4">
              <div>
                <Label htmlFor="settings-autoadvance">Auto-advance chapters</Label>
                <FieldDescription>
                  Move to the next chapter when the current one finishes.
                </FieldDescription>
              </div>
              <Switch
                id="settings-autoadvance"
                checked={settings.autoAdvance}
                onCheckedChange={settings.setAutoAdvance}
              />
            </div>

            <div>
              <div className="flex items-baseline justify-between">
                <Label htmlFor="settings-threshold">Completion threshold</Label>
                <span className="font-mono text-[11px] text-ink-subtle tabular-nums">
                  {Math.round(settings.completionThreshold * 100)}%
                </span>
              </div>
              <Slider
                id="settings-threshold"
                value={[settings.completionThreshold * 100]}
                min={50}
                max={99}
                step={1}
                onValueChange={([value]) => settings.setCompletionThreshold((value ?? 95) / 100)}
                aria-label="Completion threshold"
                className="mt-3"
              />
              <FieldDescription>
                A chapter counts as finished once you have listened this far.
              </FieldDescription>
            </div>
          </div>
        </Section>

        <Section index="03" title="Skip intervals" description="How far the back and forward buttons jump.">
          <div className="grid gap-5 sm:grid-cols-2">
            <Field label="Back (seconds)" htmlFor="settings-back">
              <Input
                id="settings-back"
                type="number"
                min={5}
                max={120}
                value={settings.skipBackwardSeconds}
                onChange={(event) => {
                  const value = Number(event.target.value);
                  if (Number.isFinite(value)) settings.setSkipBackwardSeconds(value);
                }}
              />
            </Field>
            <Field label="Forward (seconds)" htmlFor="settings-forward">
              <Input
                id="settings-forward"
                type="number"
                min={5}
                max={120}
                value={settings.skipForwardSeconds}
                onChange={(event) => {
                  const value = Number(event.target.value);
                  if (Number.isFinite(value)) settings.setSkipForwardSeconds(value);
                }}
              />
            </Field>
          </div>
        </Section>

        <Section
          index="04"
          title="Text-to-speech"
          description="The Edge TTS voice used to generate chapter audio."
        >
          <div className="space-y-2">
            <Label>Voice</Label>
            <VoicePicker
              value={settings.ttsVoice}
              onChange={(voice) => settings.setTtsVoice(voice.name)}
            />
            <FieldDescription>
              Filter by language, gender, or country, and press play to hear a voice before
              choosing it. Changes take effect for the next generation.
            </FieldDescription>
          </div>
        </Section>

        <Section index="05" title="Sleep timer" description="The default selection when you open the player.">
          <div className="grid grid-cols-2 gap-2 sm:grid-cols-3" role="radiogroup" aria-label="Sleep timer default">
            {SLEEP_TIMER_OPTIONS.map((option) => {
              const active = settings.sleepTimerPreference === option.value;
              return (
                <button
                  key={option.value}
                  type="button"
                  role="radio"
                  aria-checked={active}
                  onClick={() => settings.setSleepTimerPreference(option.value)}
                  className={cn(
                    'border px-3 py-2.5 font-mono text-[10px] uppercase tracking-[0.08em] transition-colors',
                    active
                      ? 'border-accent bg-accent-wash text-accent'
                      : 'border-hairline text-ink-muted hover:border-hairline-strong hover:text-ink',
                  )}
                >
                  {option.label}
                </button>
              );
            })}
          </div>
        </Section>

        <Section index="06" title="Data" description="Everything is stored locally in a single SQLite database. No account, no cloud sync.">
          <div className="flex flex-wrap items-center gap-3">
            <Button
              variant="danger"
              icon={<Trash2 className="size-4" aria-hidden />}
              onClick={() => setResetOpen(true)}
            >
              Clear all library data
            </Button>
            <p className="text-xs text-ink-subtle">
              Removes every book, chapter, audio file, highlight, note, bookmark and saved word.
            </p>
          </div>
        </Section>
      </div>

      <ConfirmDialog
        open={resetOpen}
        onOpenChange={setResetOpen}
        title="Clear all library data?"
        description="Every book, chapter, audio file, highlight, note, bookmark, saved word and listening progress will be permanently deleted. This cannot be undone."
        confirmLabel="Delete everything"
        destructive
        onConfirm={clearAllData}
      />
    </div>
  );
}

function Section({
  index,
  title,
  description,
  children,
}: {
  index: string;
  title: string;
  description: string;
  children: React.ReactNode;
}) {
  return (
    <section
      aria-labelledby={`settings-${index}`}
      className="grid gap-4 border-t border-hairline py-7 sm:grid-cols-[10.5rem_1fr] sm:gap-10"
    >
      <div>
        <p className="font-mono text-[10px] tabular-nums text-accent">{index}</p>
        <h2
          id={`settings-${index}`}
          className="mt-1 font-display text-lg leading-tight text-ink"
        >
          {title}
        </h2>
        <p className="mt-1.5 text-xs leading-relaxed text-ink-muted">{description}</p>
      </div>
      <div className="min-w-0">{children}</div>
    </section>
  );
}

