import * as React from 'react';
import * as ProgressPrimitive from '@radix-ui/react-progress';

import { cn } from '@/utils/cn';
import { clamp } from '@/utils/format';

/**
 * Progress bar. Also exposes a draggable/seekable variant used by the player
 * timeline, where the fill must follow the audio element in real time.
 */
export const Progress = React.forwardRef<
  React.ComponentRef<typeof ProgressPrimitive.Root>,
  React.ComponentPropsWithoutRef<typeof ProgressPrimitive.Root> & { indicatorClassName?: string }
>(function Progress({ className, value, indicatorClassName, ...props }, ref) {
  const percent = clamp(value ?? 0, 0, 1) * 100;
  return (
    <ProgressPrimitive.Root
      ref={ref}
      className={cn('relative h-1 w-full overflow-hidden rounded-full bg-hairline', className)}
      value={clamp(value ?? 0, 0, 1)}
      {...props}
    >
      <ProgressPrimitive.Indicator
        className={cn(
          'h-full rounded-full bg-accent transition-[width] duration-200 ease-out',
          indicatorClassName,
        )}
        style={{ width: `${percent}%` }}
      />
    </ProgressPrimitive.Root>
  );
});
