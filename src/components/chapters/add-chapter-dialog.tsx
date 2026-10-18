import * as React from 'react';
import { toast } from 'sonner';
import { ListPlus, Plus } from 'lucide-react';

import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Field, FieldDescription, FieldError, Input, Label, Textarea } from '@/components/ui/inputs';
import { api, errorMessage } from '@/hooks/use-api';
import { MAX_CHAPTER_TITLE, MAX_CHAPTERS_PER_BOOK } from '@/utils/constants';
import { sanitizeText } from '@/utils/sanitize';

interface ParsedChapter {
  chapterNumber: number | null;
  title: string;
}

/**
 * Parse bulk chapter text.
 *
 * Accepts one entry per line and understands the shapes a table of contents
 * actually arrives in:
 *   `Chapter 4`                       → number 4,   title "Chapter 4"
 *   `4. The Title`                    → number 4,   title "The Title"
 *   `4 — The Title` / `4: The Title`  → number 4,   title "The Title"
 *   `Prologue`                        → no number,  title "Prologue"
 *
 * Anything that does not yield a title is rejected so a bad paste surfaces as
 * an error rather than silently creating a chapter called "undefined".
 */
export function parseChapterLines(text: string): { chapters: ParsedChapter[]; invalid: string[] } {
  const chapters: ParsedChapter[] = [];
  const invalid: string[] = [];

  for (const rawLine of text.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line) continue;

    // Leading label, e.g. "Chapter 4: The Title".
    const labelled = line.match(/^(chapter|part|section|episode)\s+([0-9ivxlcdm]+)\s*[:.—–-]?\s*(.*)$/i);
    if (labelled) {
      const title = labelled[3].trim();
      chapters.push({
        chapterNumber: parseRomanOrNumber(labelled[2]),
        title: title || line,
      });
      continue;
    }

    // Leading number, e.g. "4. The Title" or "4 - The Title".
    const numbered = line.match(/^([0-9]+)\s*[:.)\-—–]\s*(.+)$/);
    if (numbered) {
      const title = numbered[2].trim();
      if (!title) {
        invalid.push(line);
        continue;
      }
      chapters.push({ chapterNumber: Number(numbered[1]), title });
      continue;
    }

    if (line.length > MAX_CHAPTER_TITLE) {
      invalid.push(line);
      continue;
    }

    // A line with no letters or digits at all ("???", "---", "•••") is debris
    // from a mangled paste, not a chapter title.
    if (!/[\p{L}\p{N}]/u.test(line)) {
      invalid.push(line);
      continue;
    }

    chapters.push({ chapterNumber: null, title: line });
  }

  return { chapters, invalid };
}

function parseRomanOrNumber(token: string): number | null {
  if (/^\d+$/.test(token)) return Number(token);
  // Small roman-numeral table — enough for front matter through ~40.
  const numerals: Record<string, number> = {
    i: 1, ii: 2, iii: 3, iv: 4, v: 5, vi: 6, vii: 7, viii: 8, ix: 9, x: 10,
    xi: 11, xii: 12, xiii: 13, xiv: 14, xv: 15, xvi: 16, xvii: 17, xviii: 18,
    xix: 19, xx: 20, xxx: 30, xl: 40, l: 50, c: 100,
  };
  return numerals[token.toLowerCase()] ?? null;
}

export interface AddChapterDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  libraryId: string;
  /** Existing count, used to cap the bulk path at MAX_CHAPTERS_PER_BOOK. */
  existingCount?: number;
  onCreated?: () => void;
}

/**
 * Create chapters manually, one at a time or by pasting a whole table of
 * contents.
 *
 * Manual creation is the primary path: most book metadata APIs expose no
 * chapter list, so the user is expected to supply their own.
 */
export function AddChapterDialog({
  open,
  onOpenChange,
  libraryId,
  existingCount = 0,
  onCreated,
}: AddChapterDialogProps) {
  const [mode, setMode] = React.useState<'single' | 'bulk'>('single');
  const [title, setTitle] = React.useState('');
  const [number, setNumber] = React.useState('');
  const [bulk, setBulk] = React.useState('');
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);

  const reset = () => {
    setTitle('');
    setNumber('');
    setBulk('');
    setError(null);
    setMode('single');
  };

  const close = (next: boolean) => {
    if (!next) reset();
    onOpenChange(next);
  };

  const parsed = React.useMemo(() => (mode === 'bulk' ? parseChapterLines(bulk) : null), [bulk, mode]);
  const parsedCount = parsed?.chapters.length ?? 0;

  const createSingle = async () => {
    const clean = sanitizeText(title, MAX_CHAPTER_TITLE);
    if (!clean) {
      setError('Enter a chapter title.');
      return;
    }
    const explicit = number.trim() ? Number(number) : null;
    setBusy(true);
    setError(null);
    try {
      await api.post(`/api/library/${libraryId}/chapters`, {
        title: clean,
        chapterNumber: explicit !== null && Number.isFinite(explicit) ? explicit : undefined,
      });
      toast.success(`Added “${clean}”`);
      reset();
      onCreated?.();
      onOpenChange(false);
    } catch (caught) {
      setError(errorMessage(caught, 'Could not add that chapter.'));
    } finally {
      setBusy(false);
    }
  };

  const createBulk = async () => {
    if (!parsed || parsedCount === 0) {
      setError('Paste at least one chapter title, one per line.');
      return;
    }
    if (existingCount + parsedCount > MAX_CHAPTERS_PER_BOOK) {
      setError(`That would exceed the ${MAX_CHAPTERS_PER_BOOK}-chapter limit for one book.`);
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await api.post(`/api/library/${libraryId}/chapters`, {
        chapters: parsed.chapters.map((entry) => ({
          title: sanitizeText(entry.title, MAX_CHAPTER_TITLE),
          chapterNumber: entry.chapterNumber ?? undefined,
        })),
      });
      toast.success(`Added ${parsedCount} ${parsedCount === 1 ? 'chapter' : 'chapters'}`);
      reset();
      onCreated?.();
      onOpenChange(false);
    } catch (caught) {
      setError(errorMessage(caught, 'Could not add those chapters.'));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={close}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>Add chapters</DialogTitle>
          <DialogDescription>
            Most book metadata has no chapter list, so add the chapters yourself. You can reorder
            them afterwards and attach audio to each one.
          </DialogDescription>
        </DialogHeader>

        <div
          role="tablist"
          aria-label="Add chapters mode"
          className="mb-5 grid grid-cols-2 gap-1 rounded-xl bg-overlay p-1"
        >
          <ModeTab
            active={mode === 'single'}
            onSelect={() => setMode('single')}
            icon={<Plus className="size-4" aria-hidden />}
          >
            One chapter
          </ModeTab>
          <ModeTab
            active={mode === 'bulk'}
            onSelect={() => setMode('bulk')}
            icon={<ListPlus className="size-4" aria-hidden />}
          >
            Paste a list
          </ModeTab>
        </div>

        {mode === 'single' ? (
          <form
            className="space-y-4"
            onSubmit={(event) => {
              event.preventDefault();
              void createSingle();
            }}
          >
            <Field
              label="Chapter title"
              htmlFor="chapter-title"
              error={error}
              hint="Shown in the chapter list and the player."
            >
              <Input
                id="chapter-title"
                value={title}
                onChange={(event) => {
                  setTitle(event.target.value);
                  setError(null);
                }}
                placeholder="Chapter 1 — The Beginning"
                autoFocus
                maxLength={MAX_CHAPTER_TITLE}
              />
            </Field>

            <Field
              label="Number (optional)"
              htmlFor="chapter-number"
              hint="Leave blank to append after the last chapter."
            >
              <Input
                id="chapter-number"
                type="number"
                min={1}
                inputMode="numeric"
                value={number}
                onChange={(event) => setNumber(event.target.value)}
                placeholder={String(existingCount + 1)}
                className="w-28"
              />
            </Field>

            <DialogFooter>
              <Button type="button" variant="ghost" onClick={() => close(false)} disabled={busy}>
                Cancel
              </Button>
              <Button type="submit" loading={busy}>
                Add chapter
              </Button>
            </DialogFooter>
          </form>
        ) : (
          <form
            className="space-y-4"
            onSubmit={(event) => {
              event.preventDefault();
              void createBulk();
            }}
          >
            <div className="space-y-2">
              <Label htmlFor="chapter-bulk">One chapter per line</Label>
              <Textarea
                id="chapter-bulk"
                value={bulk}
                onChange={(event) => {
                  setBulk(event.target.value);
                  setError(null);
                }}
                rows={9}
                spellCheck={false}
                placeholder={'Prologue\n1. The Lighthouse\n2. Salt and Rope\nChapter 4: The Crossing'}
                className="font-mono text-[13px]"
              />
              {parsed && parsedCount > 0 && (
                <FieldDescription>
                  Detected {parsedCount} {parsedCount === 1 ? 'chapter' : 'chapters'}
                  {parsed.invalid.length > 0 && ` · ${parsed.invalid.length} skipped as unrecognised`}
                </FieldDescription>
              )}
              {error && <FieldError>{error}</FieldError>}
            </div>

            <DialogFooter>
              <Button type="button" variant="ghost" onClick={() => close(false)} disabled={busy}>
                Cancel
              </Button>
              <Button type="submit" loading={busy} disabled={parsedCount === 0}>
                Add {parsedCount > 0 ? `${parsedCount} ` : ''}chapters
              </Button>
            </DialogFooter>
          </form>
        )}
      </DialogContent>
    </Dialog>
  );
}

function ModeTab({
  active,
  onSelect,
  icon,
  children,
}: {
  active: boolean;
  onSelect: () => void;
  icon: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      role="tab"
      aria-selected={active}
      onClick={onSelect}
      className={[
        'flex items-center justify-center gap-2 rounded-lg px-3 py-2 text-sm font-medium transition-colors',
        active
          ? 'bg-raised text-ink shadow-[var(--shadow-card)]'
          : 'text-ink-muted hover:text-ink',
      ].join(' ')}
    >
      {icon}
      {children}
    </button>
  );
}
