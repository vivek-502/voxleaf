import * as React from 'react';
import { toast } from 'sonner';

import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Field, Input } from '@/components/ui/inputs';
import { api, errorMessage } from '@/hooks/use-api';
import { MAX_CHAPTER_TITLE } from '@/utils/constants';
import { sanitizeText } from '@/utils/sanitize';
import type { ChapterInfo } from '@/types';

/** Rename a chapter and change its displayed number. */
export function EditChapterDialog({
  chapter,
  onOpenChange,
  onSaved,
}: {
  chapter: ChapterInfo | null;
  onOpenChange: (open: boolean) => void;
  onSaved?: () => void;
}) {
  const [title, setTitle] = React.useState('');
  const [number, setNumber] = React.useState('');
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);

  React.useEffect(() => {
    if (chapter) {
      setTitle(chapter.title);
      setNumber(chapter.chapterNumber > 0 ? String(chapter.chapterNumber) : '');
      setError(null);
    }
  }, [chapter]);

  const save = async () => {
    if (!chapter) return;
    const clean = sanitizeText(title, MAX_CHAPTER_TITLE);
    if (!clean) {
      setError('Enter a chapter title.');
      return;
    }
    const parsed = number.trim() ? Number(number) : null;
    setBusy(true);
    setError(null);
    try {
      await api.patch(`/api/chapters/${chapter.id}`, {
        title: clean,
        ...(parsed !== null && Number.isFinite(parsed) ? { chapterNumber: parsed } : {}),
      });
      toast.success('Chapter updated.');
      onSaved?.();
      onOpenChange(false);
    } catch (caught) {
      setError(errorMessage(caught, 'Could not update that chapter.'));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={Boolean(chapter)} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>Edit chapter</DialogTitle>
          <DialogDescription>
            Update the title and number shown in the chapter list and player.
          </DialogDescription>
        </DialogHeader>

        <form
          className="space-y-4"
          onSubmit={(event) => {
            event.preventDefault();
            void save();
          }}
        >
          <Field label="Chapter title" htmlFor="edit-chapter-title" error={error}>
            <Input
              id="edit-chapter-title"
              value={title}
              onChange={(event) => {
                setTitle(event.target.value);
                setError(null);
              }}
              maxLength={MAX_CHAPTER_TITLE}
              autoFocus
            />
          </Field>

          <Field
            label="Number"
            htmlFor="edit-chapter-number"
            hint="The position shown in the player and chapter list."
          >
            <Input
              id="edit-chapter-number"
              type="number"
              min={0}
              inputMode="numeric"
              value={number}
              onChange={(event) => setNumber(event.target.value)}
              className="w-28"
            />
          </Field>

          <DialogFooter>
            <Button type="button" variant="ghost" onClick={() => onOpenChange(false)} disabled={busy}>
              Cancel
            </Button>
            <Button type="submit" loading={busy}>
              Save changes
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
