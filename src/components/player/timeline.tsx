import * as React from 'react';

import { Slider } from '@/components/ui/slider';
import { usePlayer } from '@/components/providers/player-provider';
import { formatDuration, formatRemaining } from '@/utils/format';
import { cn } from '@/utils/cn';

/**
 * Scrubbable chapter timeline.
 *
 * While the pointer is down the thumb follows local drag state instead of the
 * audio element, so the handle tracks the cursor exactly instead of snapping
 * back to the last `timeupdate` tick. The seek is committed on release.
 */
export function Timeline({
  className,
  showLabels = true,
}: {
  className?: string;
  showLabels?: boolean;
}) {
  const { position, duration, buffered, seekTo, isLoading } = usePlayer();

  const [dragging, setDragging] = React.useState(false);
  const [dragValue, setDragValue] = React.useState(0);

  const total = duration > 0 ? duration : 0;
  const shown = dragging ? Math.min(dragValue, total || dragValue) : position;

  // Reset the drag preview whenever the chapter changes underneath us.
  React.useEffect(() => {
    if (!dragging) setDragValue(position);
  }, [position, dragging]);

  const disabled = total <= 0 || isLoading;

  const bufferedPct = total > 0 ? Math.min(100, (buffered / total) * 100) : 0;
  const playedPct = total > 0 ? Math.min(100, (shown / total) * 100) : 0;

  const slider = (
    <div className={cn('relative', disabled && 'pointer-events-none opacity-60')}>
      {/* Buffered bar. Mirrors the track geometry exactly (track is w-full and
          vertically centred in the root) so it sits behind the filled range. */}
      <div
        aria-hidden
        className="pointer-events-none absolute inset-x-0 top-1/2 h-1.5 -translate-y-1/2 overflow-hidden rounded-full"
      >
        <div
          className="h-full rounded-full bg-hairline-strong/70 transition-[width] duration-200"
          style={{ width: `${bufferedPct}%` }}
        />
        <div
          className="absolute inset-y-0 left-0 rounded-full bg-accent transition-[width] duration-100"
          style={{ width: `${playedPct}%` }}
        />
      </div>

      <Slider
        value={[Math.min(shown, total || shown)]}
        min={0}
        max={total || 1}
        step={1}
        disabled={disabled}
        onValueChange={([value]) => {
          setDragging(true);
          setDragValue(value ?? 0);
        }}
        onValueCommit={([value]) => {
          setDragging(false);
          seekTo(value ?? 0);
        }}
        aria-label="Seek within chapter"
        aria-valuetext={`${formatDuration(shown)} of ${formatDuration(total)}`}
        className="[&_[data-slot=track]]:bg-transparent [&_[data-slot=range]]:hidden"
      />
    </div>
  );

  if (!showLabels) return slider;

  return (
    <div className={cn('w-full', className)}>
      {slider}
      <div className="mt-0.5 flex items-center justify-between font-mono text-[11px] tabular-nums text-ink-subtle">
        <span>{formatDuration(shown)}</span>
        <span>
          {total > 0 && (
            <span className="mr-2 hidden sm:inline">
              −{formatRemaining(shown, total)}
            </span>
          )}
          {formatDuration(total)}
        </span>
      </div>
    </div>
  );
}
