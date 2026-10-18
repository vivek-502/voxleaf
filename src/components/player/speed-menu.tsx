import * as React from 'react';
import { Gauge } from 'lucide-react';

import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { Button } from '@/components/ui/button';
import { TooltipWrap } from '@/components/ui/tooltip';
import { usePlayer } from '@/components/providers/player-provider';
import { PLAYBACK_SPEEDS } from '@/utils/constants';
import { cn } from '@/utils/cn';

/**
 * Playback-rate menu.
 *
 * Selecting a rate snaps to the nearest supported value via the settings
 * provider, so what the menu highlights is always what the engine plays.
 */
export function SpeedMenu({ variant = 'ghost' }: { variant?: 'ghost' | 'outline' }) {
  const { playbackRate, setPlaybackRate } = usePlayer();

  return (
    <DropdownMenu>
      <TooltipWrap label={`Speed ${playbackRate}×`}>
        <DropdownMenuTrigger asChild>
          <Button variant={variant} size="sm" className="font-mono tabular-nums">
            <Gauge className="size-4" aria-hidden />
            {playbackRate}×
          </Button>
        </DropdownMenuTrigger>
      </TooltipWrap>
      <DropdownMenuContent align="end" className="w-40">
        <DropdownMenuLabel>Playback speed</DropdownMenuLabel>
        <DropdownMenuSeparator />
        {PLAYBACK_SPEEDS.map((speed) => (
          <DropdownMenuItem
            key={speed}
            onSelect={() => setPlaybackRate(speed)}
            className={cn('justify-between gap-4 font-mono tabular-nums', speed === playbackRate && 'text-accent-soft')}
          >
            {speed}×
            {speed === playbackRate && <span aria-hidden>✓</span>}
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
