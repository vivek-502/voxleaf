import * as React from 'react';
import { Monitor, Moon, Sun, Trash2 } from 'lucide-react';

import { PageHeader } from '@/components/common/section-header';
import { Button } from '@/components/ui/button';
import { Field, FieldDescription, Input, Label } from '@/components/ui/inputs';
import { Slider } from '@/components/ui/slider';
import { Switch } from '@/components/ui/switch';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { ConfirmDialog } from '@/components/common/confirm-dialog';
import { useSettings } from '@/components/providers/settings-provider';
import { useTheme } from '@/components/providers/theme-provider';
import { api, errorMessage } from '@/hooks/use-api';
import { toast } from 'sonner';
import { PLAYBACK_SPEEDS, SLEEP_TIMER_OPTIONS } from '@/utils/constants';
import { cn } from '@/utils/cn';
import type { ThemeId, TtsVoice } from '@/types';

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
    <div className="container-page max-w-3xl py-8 sm:py-10">
      <PageHeader
        title="Settings"
        description="Playback preferences are saved to the database and apply immediately."
      />

      <div className="flex flex-col gap-6">
        {/* Appearance */}
        <section aria-labelledby="settings-appearance" className="card-surface p-5 sm:p-6">
          <h2 id="settings-appearance" className="text-base font-semibold text-ink">
            Appearance
          </h2>
          <p className="mt-1 text-sm text-ink-muted">
            Choose how VoxLeaf looks. The choice is remembered across visits.
          </p>

          <div className="mt-5 grid grid-cols-3 gap-2" role="radiogroup" aria-label="Theme">
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
                    'flex flex-col items-center gap-2 rounded-xl border px-3 py-4 text-sm font-medium transition-colors',
                    active
                      ? 'border-accent-line bg-accent-wash text-accent-soft'
                      : 'border-hairline text-ink-muted hover:border-hairline-strong hover:text-ink',
                  )}
                >
                  <Icon className="size-5" aria-hidden />
                  {option.label}
                </button>
              );
            })}
          </div>
        </section>

        {/* Playback */}
        <section aria-labelledby="settings-playback" className="card-surface p-5 sm:p-6">
          <h2 id="settings-playback" className="text-base font-semibold text-ink">
            Playback
          </h2>
          <p className="mt-1 text-sm text-ink-muted">
            Defaults applied to every chapter you start.
          </p>

          <div className="mt-6 space-y-6">
            <div>
              <div className="flex items-baseline justify-between">
                <Label htmlFor="settings-speed">Default playback speed</Label>
                <span className="font-mono text-sm text-accent-soft tabular-nums">
                  {settings.playbackSpeed}×
                </span>
              </div>
              <div className="mt-3 flex flex-wrap gap-2" role="radiogroup" aria-label="Playback speed">
                {PLAYBACK_SPEEDS.map((speed) => (
                  <button
                    key={speed}
                    type="button"
                    role="radio"
                    aria-checked={settings.playbackSpeed === speed}
                    onClick={() => settings.setPlaybackSpeed(speed)}
                    className={cn(
                      'rounded-full border px-3.5 py-1.5 font-mono text-sm tabular-nums transition-colors',
                      settings.playbackSpeed === speed
                        ? 'border-accent-line bg-accent-wash text-accent-soft'
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
                <span className="font-mono text-sm text-accent-soft tabular-nums">
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

            <div className="flex items-center justify-between gap-4">
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
                <span className="font-mono text-sm text-accent-soft tabular-nums">
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
        </section>

        {/* Skip intervals */}
        <section aria-labelledby="settings-skip" className="card-surface p-5 sm:p-6">
          <h2 id="settings-skip" className="text-base font-semibold text-ink">
            Skip intervals
          </h2>
          <p className="mt-1 text-sm text-ink-muted">
            How far the back and forward buttons jump.
          </p>

          <div className="mt-5 grid gap-5 sm:grid-cols-2">
            <Field label="Back" htmlFor="settings-back">
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
            <Field label="Forward" htmlFor="settings-forward">
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
        </section>

        {/* TTS */}
        <section aria-labelledby="settings-tts" className="card-surface p-5 sm:p-6">
          <h2 id="settings-tts" className="text-base font-semibold text-ink">
            Text-to-Speech
          </h2>
          <p className="mt-1 text-sm text-ink-muted">
            Choose the Edge TTS voice for generating chapter audio.
          </p>

          <div className="mt-5 space-y-4">
            <Field label="Voice" htmlFor="settings-tts-voice">
              <TtsVoiceSelect />
            </Field>
            <FieldDescription>
              Voices are loaded from Edge TTS. Changes take effect for the next generation.
            </FieldDescription>
          </div>
        </section>

        {/* Sleep timer default */}
        <section aria-labelledby="settings-sleep" className="card-surface p-5 sm:p-6">
          <h2 id="settings-sleep" className="text-base font-semibold text-ink">
            Sleep timer
          </h2>
          <p className="mt-1 text-sm text-ink-muted">
            The default selection when you open the player.
          </p>

          <div className="mt-5 grid grid-cols-2 gap-2 sm:grid-cols-3" role="radiogroup" aria-label="Sleep timer default">
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
                    'rounded-xl border px-3 py-2.5 text-sm font-medium transition-colors',
                    active
                      ? 'border-accent-line bg-accent-wash text-accent-soft'
                      : 'border-hairline text-ink-muted hover:border-hairline-strong hover:text-ink',
                  )}
                >
                  {option.label}
                </button>
              );
            })}
          </div>
        </section>

        {/* Data */}
        <section aria-labelledby="settings-data" className="card-surface border-danger/25 p-5 sm:p-6">
          <h2 id="settings-data" className="text-base font-semibold text-ink">
            Data
          </h2>
          <p className="mt-1 text-sm text-ink-muted">
            VoxLeaf stores everything locally in a single SQLite database. There is no account and
            no cloud sync.
          </p>

          <div className="mt-5 flex flex-wrap items-center gap-3">
            <Button
              variant="danger"
              icon={<Trash2 className="size-4" aria-hidden />}
              onClick={() => setResetOpen(true)}
            >
              Clear all library data
            </Button>
            <p className="text-xs text-ink-subtle">
              Removes every book, chapter, bookmark and progress record.
            </p>
          </div>
        </section>
      </div>

      <ConfirmDialog
        open={resetOpen}
        onOpenChange={setResetOpen}
        title="Clear all library data?"
        description="Every book, chapter, audio file, bookmark and listening progress will be permanently deleted. This cannot be undone."
        confirmLabel="Delete everything"
        destructive
        onConfirm={clearAllData}
      />
    </div>
  );
}


function TtsVoiceSelect() {
  const settings = useSettings();
  const [voices, setVoices] = React.useState<TtsVoice[]>([]);
  const [loading, setLoading] = React.useState(true);
  const [failed, setFailed] = React.useState(false);

  React.useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        // `api.get` already unwraps the `data` envelope, so this is the array.
        const res = await api.get<TtsVoice[]>('/api/tts/voices');
        if (!cancelled) setVoices(Array.isArray(res) ? res : []);
      } catch {
        if (!cancelled) {
          setVoices([]);
          setFailed(true);
        }
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <>
      <Select
        value={settings.ttsVoice}
        onValueChange={(v) => settings.setTtsVoice(v)}
        disabled={loading || voices.length === 0}
      >
        <SelectTrigger>
          <SelectValue placeholder={loading ? 'Loading voices…' : 'Select a voice'} />
        </SelectTrigger>
        <SelectContent className="max-h-96">
          {voices.map((voice) => (
            <SelectItem key={voice.name} value={voice.name}>
              {voice.name} ({voice.locale} · {voice.gender})
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      {/* An empty dropdown looks identical to "nothing to pick", so say which it is. */}
      {failed && (
        <p className="mt-2 text-sm text-danger">
          Could not load the voice list. The voice service may be unavailable.
        </p>
      )}
    </>
  );
}
