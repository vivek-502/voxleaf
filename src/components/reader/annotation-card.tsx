'use client';

import * as React from 'react';
import { BookOpen, Pencil, Search, Trash2, X } from 'lucide-react';

import { cn } from '@/utils/cn';
import type { AnnotationColor, EpubAnnotation } from '@/types';

const COLORS: { value: AnnotationColor; label: string; className: string }[] = [
  { value: 'yellow', label: 'Yellow', className: 'bg-[#f2c14e]' },
  { value: 'green', label: 'Green', className: 'bg-[#7fb069]' },
  { value: 'blue', label: 'Blue', className: 'bg-[#6a9fd8]' },
  { value: 'pink', label: 'Pink', className: 'bg-[#e58fa8]' },
  { value: 'violet', label: 'Violet', className: 'bg-[#a48bd4]' },
];

export interface AnnotationCardProps {
  annotation: EpubAnnotation;
  /** Viewport coordinates of the highlight's first range. */
  anchor: { left: number; top: number; width: number; height: number };
  onClose: () => void;
  onColor: (color: AnnotationColor) => void;
  onEditNote: () => void;
  onDefine: () => void;
  onSearch: () => void;
  onDelete: () => void;
}

/**
 * What a highlight has to say for itself when it is clicked.
 *
 * The card is deliberately small: the quote, the note if there is one, the
 * colours, and the things a reader does next — edit, define a word, search the
 * book, remove. Everything else belongs in the study panel, which is where the
 * whole set is visible.
 */
export function AnnotationCard({
  annotation,
  anchor,
  onClose,
  onColor,
  onEditNote,
  onDefine,
  onSearch,
  onDelete,
}: AnnotationCardProps) {
  const width = 320;
  const margin = 12;
  const viewportWidth = typeof window === 'undefined' ? 1024 : window.innerWidth;
  const viewportHeight = typeof window === 'undefined' ? 768 : window.innerHeight;

  const left = Math.min(
    Math.max(anchor.left + anchor.width / 2, margin + width / 2),
    viewportWidth - margin - width / 2,
  );
  const above = anchor.top > 220;
  const top = above ? anchor.top - margin : anchor.top + anchor.height + margin;

  const visibleText = annotation.selectedText || annotation.quote;

  return (
    <div
      role="dialog"
      aria-label="Highlight"
      className="fixed z-[65] border p-3.5 shadow-[0_18px_44px_-16px_rgb(0_0_0/0.6)]"
      style={{
        left,
        top,
        width,
        transform: above ? 'translate(-50%, -100%)' : 'translate(-50%, 0)',
        maxHeight: Math.min(340, viewportHeight - 32),
        overflowY: 'auto',
        background: 'var(--reader-chrome)',
        backdropFilter: 'blur(20px) saturate(150%)',
        WebkitBackdropFilter: 'blur(20px) saturate(150%)',
        borderColor: 'var(--reader-rule)',
        color: 'var(--reader-fg)',
      }}
      onMouseDown={(event) => event.stopPropagation()}
    >
      <div className="flex items-start justify-between gap-2">
        <span
          className="font-mono text-[9px] uppercase tracking-[0.14em]"
          style={{ color: 'var(--reader-muted)' }}
        >
          {annotation.chapterTitle}
          {annotation.language ? ` · ${annotation.language.toUpperCase()}` : ''}
        </span>
        <button
          type="button"
          onClick={onClose}
          aria-label="Close highlight"
          className="shrink-0 p-0.5 transition-opacity hover:opacity-65"
          style={{ color: 'var(--reader-muted)' }}
        >
          <X className="size-3.5" aria-hidden />
        </button>
      </div>

      <blockquote
        className="mt-2 max-h-32 overflow-y-auto border-l-2 pl-2.5 text-[13.5px] leading-relaxed"
        style={{ borderColor: colorHex(annotation.color), color: 'var(--reader-fg)' }}
      >
        “{visibleText}”
      </blockquote>

      {annotation.note ? (
        <p className="mt-2.5 whitespace-pre-wrap text-[13px] leading-relaxed" style={{ color: 'var(--reader-fg)' }}>
          {annotation.note}
        </p>
      ) : null}

      <div className="mt-3 flex items-center gap-1.5">
        {COLORS.map((option) => (
          <button
            key={option.value}
            type="button"
            onClick={() => onColor(option.value)}
            aria-label={option.label}
            aria-pressed={annotation.color === option.value}
            title={option.label}
            className={cn(
              'size-5 rounded-full border transition-transform hover:scale-110',
              option.className,
              annotation.color === option.value
                ? 'border-[var(--reader-fg)] ring-1 ring-[var(--reader-fg)]/30 ring-offset-1'
                : 'border-black/20',
            )}
          />
        ))}
        <span className="flex-1" />
        <IconAction label="Edit note" onClick={onEditNote}>
          <Pencil className="size-4" aria-hidden />
        </IconAction>
        <IconAction label="Define a word" onClick={onDefine}>
          <BookOpen className="size-4" aria-hidden />
        </IconAction>
        <IconAction label="Search this book" onClick={onSearch}>
          <Search className="size-4" aria-hidden />
        </IconAction>
        <IconAction label="Delete highlight" onClick={onDelete} danger>
          <Trash2 className="size-4" aria-hidden />
        </IconAction>
      </div>
    </div>
  );
}

function IconAction({
  label,
  onClick,
  danger,
  children,
}: {
  label: string;
  onClick: () => void;
  danger?: boolean;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-label={label}
      title={label}
      className="grid size-7 place-items-center transition-opacity hover:opacity-65 focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-[var(--reader-link)]"
      style={{ color: danger ? '#c0492f' : 'var(--reader-fg)' }}
    >
      {children}
    </button>
  );
}

function colorHex(color: AnnotationColor): string {
  switch (color) {
    case 'green':
      return '#7fb069';
    case 'blue':
      return '#6a9fd8';
    case 'pink':
      return '#e58fa8';
    case 'violet':
      return '#a48bd4';
    default:
      return '#f2c14e';
  }
}
