'use client';

import * as React from 'react';
import { BookOpen } from 'lucide-react';

import { cn } from '@/utils/cn';
import { initialsFromTitle } from '@/utils/format';

/**
 * Cover sizes.
 *
 * `xs`/`sm` are deliberately square — they sit in tight rows (mini player,
 * list affordances) where a portrait box would be taller than the row.
 *
 * `md` and up are *portrait*: they set only a width plus `aspect-[2/3]` and
 * never a height. A `size-*` (which sets width *and* height) would silently
 * beat a caller's `aspect-*`/`w-*`, because Tailwind emits `size-*` before
 * `w-*` in the stylesheet — the cover would render as a wide, squat bar
 * instead of a portrait cover.
 */
const SIZE_CLASSES: Record<string, string> = {
  xs: 'size-10',
  sm: 'size-12',
  md: 'aspect-[2/3] w-16',
  lg: 'aspect-[2/3] w-24',
  xl: 'aspect-[2/3] w-40',
  hero: 'aspect-[2/3] w-48 sm:w-60 lg:w-72',
};

export interface BookCoverProps extends React.HTMLAttributes<HTMLDivElement> {
  src: string | null | undefined;
  title: string;
  size?: keyof typeof SIZE_CLASSES | string;
  /** Slightly larger rounded corners for the hero cover. */
  emphasis?: boolean;
}

/**
 * Book artwork with a deterministic generated fallback.
 *
 * Third-party covers are served through `/api/covers`, which allow-lists the
 * host. When artwork is missing or fails to load we derive a stable gradient
 * from the title, so a shelf never shows a broken-image box.
 */
export function BookCover({
  src,
  title,
  size = 'md',
  emphasis = false,
  className,
  ...props
}: BookCoverProps) {
  const [failed, setFailed] = React.useState(false);
  const sizeClass = SIZE_CLASSES[size] ?? SIZE_CLASSES.md;

  // Re-attempt when the book changes (a placeholder URL can resolve later).
  React.useEffect(() => setFailed(false), [src]);

  const showImage = Boolean(src) && !failed;

  return (
    <div
      className={cn(
        'relative shrink-0 overflow-hidden bg-overlay ring-1 ring-ink/10',
        sizeClass,
        'rounded-[2px]',
        emphasis && 'ring-1 ring-ink/15',
        className,
      )}
      {...props}
    >
      {showImage ? (
        // A plain <img>: next/image would add an optimizer round-trip for
        // externally-hosted covers that we already proxy ourselves.
        // eslint-disable-next-line @next/next/no-img-element
        <img
          src={src as string}
          alt=""
          loading="lazy"
          decoding="async"
          onError={() => setFailed(true)}
          className="size-full object-cover"
        />
      ) : (
        <FallbackCover title={title} />
      )}
    </div>
  );
}

/**
 * Deterministic cloth-binding placeholder: a deep, stable colour with a ruled
 * inset and serif initials, so an unjacketed book still looks like a book.
 */
function FallbackCover({ title }: { title: string }) {
  const hue = React.useMemo(() => {
    let hash = 0;
    for (let i = 0; i < title.length; i += 1) {
      hash = (hash * 31 + title.charCodeAt(i)) % 360;
    }
    return hash;
  }, [title]);

  return (
    <div
      className="grid size-full place-items-center p-[7%]"
      style={{
        backgroundImage: `linear-gradient(150deg, oklch(0.34 0.055 ${hue}), oklch(0.23 0.04 ${(hue + 35) % 360}))`,
      }}
      role="img"
      aria-label={`No cover available for ${title}`}
    >
      <span className="flex size-full flex-col items-center justify-center gap-[8%] border border-white/20 text-center">
        <BookOpen className="size-[18%] min-w-3 max-w-5 text-white/40" aria-hidden />
        <span className="line-clamp-2-safe font-display text-[13px] leading-tight font-medium tracking-[0.06em] text-white/80">
          {initialsFromTitle(title)}
        </span>
      </span>
    </div>
  );
}
