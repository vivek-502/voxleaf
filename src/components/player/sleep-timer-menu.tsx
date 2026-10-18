import * as React from 'react';
import { Moon, MoonStar } from 'lucide-react';

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
import { describeSleepTimer } from '@/hooks/use-sleep-timer';
import { SLEEP_TIMER_OPTIONS } from '@/utils/constants';
import { cn } from '@/utils/cn';

/**
 * Sleep-timer menu: countdown minutes or "stop at the end of this chapter".
 * The active choice is also shown as a chip so it stays visible once picked.
 */
export function SleepTimerMenu({ variant = 'ghost' }: { variant?: 'ghost' | 'outline' }) {
  const { sleepTimer, setSleepTimer } = usePlayer();
  const description = describeSleepTimer(sleepTimer);
  const active = sleepTimer.active;

  return (
    <DropdownMenu>
      <TooltipWrap label={description ? `Sleep timer: ${description}` : 'Sleep timer'}>
        <DropdownMenuTrigger asChild>
          <Button
            variant={variant}
            size="icon"
            aria-label={description ? `Sleep timer, ${description}` : 'Set a sleep timer'}
            className={cn(description && 'border-accent-line bg-accent-wash text-accent-soft')}
          >
            {active === 'end-of-chapter' ? (
              <MoonStar className="size-[18px]" aria-hidden />
            ) : (
              <Moon className="size-[18px]" aria-hidden />
            )}
            {sleepTimer.remaining !== null && (
              <span className="absolute -top-1 -right-1 rounded-full bg-accent px-1.5 text-[10px] font-semibold tabular-nums text-white">
                {sleepTimer.remaining < 60 ? sleepTimer.remaining : Math.ceil(sleepTimer.remaining / 60)}
              </span>
            )}
          </Button>
        </DropdownMenuTrigger>
      </TooltipWrap>

      <DropdownMenuContent align="end" className="w-52">
        <DropdownMenuLabel>Sleep timer</DropdownMenuLabel>
        <DropdownMenuSeparator />
        {SLEEP_TIMER_OPTIONS.map((option) => {
          const selected = option.value === active;
          return (
            <DropdownMenuItem
              key={option.value}
              onSelect={() => setSleepTimer(option.value)}
              className={cn('justify-between gap-4', selected && 'text-accent-soft')}
            >
              {option.label}
              {selected && <span aria-hidden>✓</span>}
            </DropdownMenuItem>
          );
        })}
        {active !== 'off' && (
          <>
            <DropdownMenuSeparator />
            <DropdownMenuItem onSelect={() => setSleepTimer('off')}>Cancel timer</DropdownMenuItem>
          </>
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
