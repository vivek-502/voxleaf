'use client';

import * as React from 'react';
import { BookOpen, Copy, Search, StickyNote, Volume2 } from 'lucide-react';

import { cn } from '@/utils/cn';
import type { AnnotationColor } from '@/types';

const COLORS: { value: AnnotationColor; label: string; className: string }[] = [
  { value: 'yellow', label: 'Yellow highlight', className: 'bg-[#f2c14e]' },
  { value: 'green', label: 'Green highlight', className: 'bg-[#7fb069]' },
  { value: 'blue', label: 'Blue highlight', className: 'bg-[#6a9fd8]' },
  { value: 'pink', label: 'Pink highlight', className: 'bg-[#e58fa8]' },
  { value: 'violet', label: 'Violet highlight', className: 'bg-[#a48bd4]' },
];

export interface SelectionDraft {
  /** The words on screen. */
  text: string;
  /** The source-language words, for a highlight made while reading a translation. */
  quote: string;
  start: { block: number; offset: number };
  end: { block: number; offset: number };
  rect: DOMRect;
  language: string | null;
  translated: boolean;
}

export interface SelectionToolbarProps {
  selection: SelectionDraft;
  onHighlight: (color: AnnotationColor) => void;
  onNote: () => void;
  onDefine: () => void;
  onCopy: () => void;
  onSearch: () => void;
  onSpeak: () => void;
}

/**
 * The small strip that appears over selected text.
 *
 * Foliate's model, adapted for a web page: highlight in one of five colours,
 * keep a note, look the word up, copy, search the book, or hear it read aloud.
 * The selection itself is captured before the strip opens, so clicking a control
 * does not need the browser to still be holding the text.
 */
export function SelectionToolbar({
  selection,
  onHighlight,
  onNote,
  onDefine,
  onCopy,
  onSearch,
  onSpeak,
}: SelectionToolbarProps) {
  const [mounted, setMounted] = React.useState(false);
  React.useEffect(() => {
    const frame = requestAnimationFrame(() => setMounted(true));
    return () => cancelAnimationFrame(frame);
  }, []);

  // The strip is sized by its controls, not by a guess: five swatches and five
  // buttons are wider than the old constant, and on a phone a few pixels is the
  // difference between fitting and hanging off the edge.
  const ref = React.useRef<HTMLDivElement>(null);
  const [width, setWidth] = React.useState(300);
  React.useLayoutEffect(() => {
    const measured = ref.current?.offsetWidth;
    if (measured && measured !== width) setWidth(measured);
  }, [width]);

  const height = 46;
  const margin = 10;
  const viewportWidth = typeof window === 'undefined' ? 1024 : window.innerWidth;
  const center = selection.rect.left + selection.rect.width / 2;
  const left = Math.min(Math.max(center, margin + width / 2), viewportWidth - margin - width / 2);
  const above = selection.rect.top > height + margin + 8;
  const top = above ? selection.rect.top - margin : selection.rect.bottom + margin;

  return (
    <div
      ref={ref}
      role="toolbar"
      aria-label="Selection actions"
      className={cn(
        'fixed z-[70] flex items-center gap-0.5 border px-1.5 py-1 shadow-[0_14px_34px_-12px_rgb(0_0_0/0.55)] transition-opacity duration-150',
        mounted ? 'opacity-100' : 'opacity-0',
      )}
      style={{
        left,
        top,
        transform: above ? 'translate(-50%, -100%)' : 'translate(-50%, 0)',
        background: 'var(--reader-chrome)',
        backdropFilter: 'blur(18px) saturate(150%)',
        WebkitBackdropFilter: 'blur(18px) saturate(150%)',
        borderColor: 'var(--reader-rule)',
        color: 'var(--reader-fg)',
      }}
      onMouseDown={(event) => event.preventDefault()}
    >
      {COLORS.map((color) => (
        <button
          key={color.value}
          type="button"
          onClick={() => onHighlight(color.value)}
          aria-label={color.label}
          title={color.label}
          className={cn(
            'mx-0.5 size-5 rounded-full border border-black/20 transition-transform hover:scale-110',
            color.className,
          )}
        />
      ))}

      <span className="mx-1 h-5 w-px" style={{ background: 'var(--reader-rule)' }} aria-hidden />

      <ToolButton label="Add a note" onClick={onNote}>
        <StickyNote className="size-4" aria-hidden />
      </ToolButton>
      <ToolButton label="Define" onClick={onDefine}>
        <BookOpen className="size-4" aria-hidden />
      </ToolButton>
      <ToolButton label="Copy" onClick={onCopy}>
        <Copy className="size-4" aria-hidden />
      </ToolButton>
      <ToolButton label="Search this book" onClick={onSearch}>
        <Search className="size-4" aria-hidden />
      </ToolButton>
      <ToolButton label="Read aloud" onClick={onSpeak}>
        <Volume2 className="size-4" aria-hidden />
      </ToolButton>
    </div>
  );
}

function ToolButton({
  label,
  onClick,
  children,
}: {
  label: string;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-label={label}
      title={label}
      className="grid size-8 place-items-center transition-opacity hover:opacity-65 focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-[var(--reader-link)]"
      style={{ color: 'var(--reader-fg)' }}
    >
      {children}
    </button>
  );
}

export { COLORS as HIGHLIGHT_COLORS };
