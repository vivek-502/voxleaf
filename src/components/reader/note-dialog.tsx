'use client';

import * as React from 'react';
import { Trash2 } from 'lucide-react';

import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Textarea } from '@/components/ui/inputs';
import { cn } from '@/utils/cn';
import type { AnnotationColor } from '@/types';

const COLORS: { value: AnnotationColor; label: string; className: string }[] = [
  { value: 'yellow', label: 'Yellow', className: 'bg-[#f2c14e]' },
  { value: 'green', label: 'Green', className: 'bg-[#7fb069]' },
  { value: 'blue', label: 'Blue', className: 'bg-[#6a9fd8]' },
  { value: 'pink', label: 'Pink', className: 'bg-[#e58fa8]' },
  { value: 'violet', label: 'Violet', className: 'bg-[#a48bd4]' },
];

export interface NoteDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  mode: 'create' | 'edit';
  quote: string;
  note: string;
  color: AnnotationColor;
  busy?: boolean;
  onSave: (note: string, color: AnnotationColor) => void;
  onDelete?: () => void;
}

/**
 * Writing a note on a passage.
 *
 * The quote is shown above the field so a reader editing an old note knows what
 * it was about without closing the dialog and hunting for the highlight. Blank
 * notes are allowed: a colour mark with no words is a legitimate thing to leave
 * in a book.
 */
export function NoteDialog({
  open,
  onOpenChange,
  mode,
  quote,
  note,
  color,
  busy = false,
  onSave,
  onDelete,
}: NoteDialogProps) {
  const [value, setValue] = React.useState(note);
  const [chosen, setChosen] = React.useState<AnnotationColor>(color);

  React.useEffect(() => {
    if (!open) return;
    setValue(note);
    setChosen(color);
  }, [color, note, open]);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-xl">
        <DialogHeader>
          <DialogTitle>{mode === 'edit' ? 'Edit note' : 'Add a note'}</DialogTitle>
          <DialogDescription>
            {mode === 'edit'
              ? 'Change the words or the colour of this mark.'
              : 'The highlight keeps your place even when the section is read in another language.'}
          </DialogDescription>
        </DialogHeader>

        <blockquote className="mb-4 max-h-40 overflow-y-auto border-l-2 border-accent/50 bg-overlay/60 px-3 py-2 text-[13px] leading-relaxed text-ink-muted">
          {quote}
        </blockquote>

        <Textarea
          value={value}
          onChange={(event) => setValue(event.target.value)}
          placeholder="What do you want to remember about this passage?"
          rows={5}
          maxLength={8000}
          autoFocus
          aria-label="Note"
        />

        <div className="mt-4 flex items-center gap-2">
          <span className="font-mono text-[10px] uppercase tracking-[0.1em] text-ink-subtle">
            Colour
          </span>
          {COLORS.map((option) => (
            <button
              key={option.value}
              type="button"
              onClick={() => setChosen(option.value)}
              aria-label={option.label}
              aria-pressed={chosen === option.value}
              title={option.label}
              className={cn(
                'size-6 rounded-full border transition-transform hover:scale-110',
                option.className,
                chosen === option.value
                  ? 'border-ink ring-2 ring-ink/25 ring-offset-2 ring-offset-raised'
                  : 'border-black/20',
              )}
            />
          ))}
        </div>

        <DialogFooter className="sm:justify-between">
          {mode === 'edit' && onDelete ? (
            <Button
              type="button"
              variant="danger"
              icon={<Trash2 className="size-4" aria-hidden />}
              onClick={onDelete}
            >
              Delete
            </Button>
          ) : (
            <span />
          )}
          <div className="flex flex-col-reverse gap-2 sm:flex-row">
            <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <Button
              type="button"
              loading={busy}
              onClick={() => onSave(value.trim(), chosen)}
            >
              Save note
            </Button>
          </div>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
