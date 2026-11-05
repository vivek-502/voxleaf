'use client';

import * as React from 'react';
import * as SliderPrimitive from '@radix-ui/react-slider';

import { cn } from '@/utils/cn';

/**
 * Slider primitive. Radix provides full keyboard support (arrows, Home/End,
 * PageUp/PageDown) and correct ARIA, which the player timeline relies on.
 */
export const Slider = React.forwardRef<
  React.ComponentRef<typeof SliderPrimitive.Root>,
  React.ComponentPropsWithoutRef<typeof SliderPrimitive.Root>
>(function Slider({ className, ...props }, ref) {
  const thumbCount = Array.isArray(props.value ?? props.defaultValue)
    ? (props.value ?? props.defaultValue)!.length
    : 1;

  return (
    <SliderPrimitive.Root
      ref={ref}
      className={cn(
        'relative flex w-full touch-none select-none items-center py-2',
        'data-[disabled]:opacity-50',
        className,
      )}
      {...props}
    >
      <SliderPrimitive.Track
        data-slot="track"
        className="relative h-[3px] w-full grow overflow-hidden bg-hairline"
      >
        <SliderPrimitive.Range data-slot="range" className="absolute h-full bg-accent" />
      </SliderPrimitive.Track>
      {Array.from({ length: thumbCount }, (_, index) => (
        <SliderPrimitive.Thumb
          key={index}
          data-slot="thumb"
          className={cn(
            'block size-3.5 rounded-[1px] border border-accent-deep bg-accent shadow-[0_1px_4px_rgba(0,0,0,0.25)]',
            'transition-[transform] duration-150',
            'hover:scale-110 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent',
            'disabled:pointer-events-none',
          )}
        />
      ))}
    </SliderPrimitive.Root>
  );
});
