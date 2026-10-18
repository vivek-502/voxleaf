import * as React from 'react';
import { Volume1, Volume2, VolumeX } from 'lucide-react';

import { Slider } from '@/components/ui/slider';
import { TooltipWrap } from '@/components/ui/tooltip';
import { usePlayer } from '@/components/providers/player-provider';
import { cn } from '@/utils/cn';

/**
 * Volume control.
 *
 * The slider collapses to an icon-only button under `sm` so the player bar
 * stays usable on narrow phones. Muting preserves the underlying volume, so
 * unmuting restores the previous level rather than jumping to 100%.
 */
export function VolumeControl({ className }: { className?: string }) {
  const { volume, muted, setVolume, toggleMute } = usePlayer();

  const Icon = muted || volume === 0 ? VolumeX : volume < 0.5 ? Volume1 : Volume2;
  const percent = Math.round((muted ? 0 : volume) * 100);

  return (
    // The slider reveal is pure CSS (`group-hover:` / `group-focus-within:`), so
    // no JS state is needed for it.
    <div className={cn('group flex items-center', className)}>
      <TooltipWrap label={muted ? 'Unmute' : 'Mute'}>
        <button
          type="button"
          onClick={toggleMute}
          aria-label={muted ? 'Unmute' : 'Mute'}
          aria-pressed={muted}
          className="grid size-9 shrink-0 place-items-center rounded-full text-ink-muted transition-colors hover:bg-overlay hover:text-ink"
        >
          <Icon className="size-[18px]" aria-hidden />
        </button>
      </TooltipWrap>

      <div
        className={cn(
          'w-0 overflow-hidden opacity-0 transition-[width,opacity,margin] duration-200 ease-[var(--ease-out-quint)]',
          'group-hover:w-24 group-hover:opacity-100 group-focus-within:w-24 group-focus-within:opacity-100',
          'sm:w-24 sm:opacity-100',
        )}
      >
        <Slider
          value={[muted ? 0 : volume * 100]}
          max={100}
          step={1}
          onValueChange={([value]) => setVolume((value ?? 0) / 100)}
          aria-label="Volume"
          aria-valuetext={`${percent}%`}
        />
      </div>
    </div>
  );
}
