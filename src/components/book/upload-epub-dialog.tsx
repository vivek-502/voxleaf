import * as React from 'react';
import { toast } from 'sonner';
import { FileText, Loader2, UploadCloud, X } from 'lucide-react';

import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { FieldDescription, FieldError } from '@/components/ui/inputs';
import { ProgressBar } from '@/components/book/book-card';
import { api, errorMessage } from '@/hooks/use-api';
import { EPUB_ACCEPT_ATTRIBUTE, EPUB_MAX_SIZE_MB } from '@/utils/constants';
import { formatBytes } from '@/utils/format';
import { cn } from '@/utils/cn';

export interface UploadEpubDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  bookId: string;
  bookTitle: string;
  /** True when a file is already stored, which makes this a replacement. */
  replacing: boolean;
  onUploaded?: () => void;
}

/**
 * Drag-and-drop EPUB upload.
 *
 * The file is how a book's text arrives, so this is the primary path rather than
 * a fallback. Replacing is the only case that needs the consequences spelled out
 * before the upload starts.
 *
 * Client-side validation is a hint only. The server re-checks the real thing:
 * that the bytes are a zip and that the zip is an EPUB with a readable container.
 * A file called `.epub` that is a renamed PDF gets past a name check and dies at
 * the parser, so the size and extension here are there to catch the obvious
 * mistake early, not to stand in for the check that matters.
 */
export function UploadEpubDialog({
  open,
  onOpenChange,
  bookId,
  bookTitle,
  replacing,
  onUploaded,
}: UploadEpubDialogProps) {
  const [file, setFile] = React.useState<File | null>(null);
  const [progress, setProgress] = React.useState(0);
  const [state, setState] = React.useState<'idle' | 'uploading' | 'error'>('idle');
  const [error, setError] = React.useState<string | null>(null);
  const inputRef = React.useRef<HTMLInputElement>(null);
  const controller = React.useRef<AbortController | null>(null);

  const reset = () => {
    setFile(null);
    setProgress(0);
    setState('idle');
    setError(null);
    if (controller.current) controller.current.abort();
  };

  const close = (next: boolean) => {
    if (!next) reset();
    onOpenChange(next);
  };

  const accept = (raw: File | undefined) => {
    if (!raw) return;
    setError(null);

    if (raw.size > EPUB_MAX_SIZE_MB * 1024 * 1024) {
      setError(`That file is ${formatBytes(raw.size)}. The limit is ${EPUB_MAX_SIZE_MB} MB.`);
      return;
    }
    if (raw.size === 0) {
      setError('That file is empty.');
      return;
    }

    setFile(raw);
    setProgress(0);
    setState('idle');
  };

  const pick = (event: React.ChangeEvent<HTMLInputElement>) => {
    accept(event.target.files?.[0]);
    // Reset so choosing the same file twice in a row still fires a change.
    if (inputRef.current) inputRef.current.value = '';
  };

  const onDrop = (event: React.DragEvent) => {
    event.preventDefault();
    event.stopPropagation();
    accept(event.dataTransfer.files?.[0]);
  };

  const onDrag = (event: React.DragEvent) => {
    event.preventDefault();
    event.stopPropagation();
  };

  const upload = async () => {
    if (!file) return;
    setState('uploading');
    setProgress(0);
    setError(null);

    const controllerInstance = new AbortController();
    controller.current = controllerInstance;

    try {
      const result = await api.upload<{
        status: string;
        chapterCount?: number;
        replaced: boolean;
        error: string | null;
      }>(
        `/api/books/${bookId}/epub/upload`,
        file,
        {},
        (p) => setProgress(p),
        controllerInstance.signal,
      );

      // The route reports a rejected file as a success carrying the reason, the
      // same as the reparse route, so a bad file arrives here rather than as a
      // thrown error. It is the more useful case: the file picker is still open
      // and picking a different one is the obvious next move.
      if (result.status !== 'ready') {
        const message = result.error ?? 'That file could not be used as an EPUB.';
        setError(message);
        setState('error');
        toast.error(message);
        return;
      }

      const sections = result.chapterCount;
      toast.success(
        result.replaced
          ? `Book text replaced — ${sections ?? 0} ${sections === 1 ? 'section' : 'sections'} ready.`
          : `Book text added — ${sections ?? 0} ${sections === 1 ? 'section' : 'sections'} ready.`,
      );
      onUploaded?.();
      close(false);
    } catch (caught) {
      if (controllerInstance.signal.aborted) return;
      const message = errorMessage(caught, 'The EPUB upload failed.');
      setError(message);
      setState('error');
      toast.error(message);
    } finally {
      controller.current = null;
    }
  };

  const cancel = () => {
    if (controller.current) {
      controller.current.abort();
      controller.current = null;
    }
    setState('idle');
    setProgress(0);
  };

  return (
    <Dialog open={open} onOpenChange={close}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>{replacing ? 'Replace the book file' : 'Add the book file'}</DialogTitle>
          <DialogDescription>
            {replacing ? (
              <>
                This uploads your own EPUB for “{bookTitle}”, replacing the one that is already
                stored. Its sections, images and reading position are rebuilt from the new file.
              </>
            ) : (
              <>
                Upload an EPUB for “{bookTitle}”. VoxLeaf has no book files of its own, so this
                is where the text comes from — its formatting, images and reading order all come
                with it.
              </>
            )}
          </DialogDescription>
        </DialogHeader>

        {replacing && (
          <p className="rounded-card border border-caution/25 bg-caution/6 px-4 py-3 text-sm text-ink-muted">
            Your chapters and audio files are kept. Translations of sections that no longer
            match the new text are marked out of date rather than deleted.
          </p>
        )}

        {!file ? (
          <div
            onDrop={onDrop}
            onDragOver={onDrag}
            onDragEnter={onDrag}
            className={cn(
              'relative flex flex-col items-center justify-center rounded-2xl border-2 border-dashed border-hairline px-6 py-10 text-center',
              'transition-colors hover:border-accent-line hover:bg-accent-wash/30',
            )}
          >
            <UploadCloud className="size-10 text-ink-subtle" aria-hidden />
            <h3 className="mt-4 text-base font-semibold text-ink">
              Drag and drop your EPUB here
            </h3>
            <p className="mt-1 text-sm text-ink-muted">
              or choose a file — up to {EPUB_MAX_SIZE_MB} MB
            </p>
            <Button
              type="button"
              variant="subtle"
              className="mt-5"
              onClick={() => inputRef.current?.click()}
            >
              Choose file
            </Button>
            <input
              ref={inputRef}
              type="file"
              accept={EPUB_ACCEPT_ATTRIBUTE}
              className="sr-only-focusable"
              onChange={pick}
            />
            {error && (
              <FieldError className="mt-4 max-w-sm text-center">{error}</FieldError>
            )}
          </div>
        ) : (
          <div className="space-y-5">
            <div className="flex items-start gap-3 rounded-2xl border border-hairline bg-raised p-4">
              <span className="grid size-10 shrink-0 place-items-center rounded-xl bg-overlay text-accent-soft">
                <FileText className="size-5" aria-hidden />
              </span>
              <div className="min-w-0 flex-1">
                <p className="truncate text-sm font-medium text-ink">{file.name}</p>
                <p className="mt-0.5 text-xs text-ink-subtle">{formatBytes(file.size)}</p>
              </div>
              <Button
                type="button"
                variant="ghost"
                size="icon"
                onClick={() => reset()}
                aria-label="Remove file"
                disabled={state === 'uploading'}
              >
                <X className="size-4" aria-hidden />
              </Button>
            </div>

            {state === 'uploading' && (
              <div className="space-y-2">
                <ProgressBar value={progress / 100} showValue label="Upload progress" size="md" />
                <FieldDescription>
                  Uploading… a large file takes a moment on a slow connection.
                </FieldDescription>
              </div>
            )}

            {error && <FieldError>{error}</FieldError>}
          </div>
        )}

        <DialogFooter>
          <Button
            type="button"
            variant="ghost"
            onClick={() => close(false)}
            disabled={state === 'uploading'}
          >
            Cancel
          </Button>
          {file && state !== 'uploading' && (
            <Button type="button" onClick={() => void upload()}>
              {replacing ? 'Replace book file' : 'Add book file'}
            </Button>
          )}
          {state === 'uploading' && (
            <Button
              type="button"
              variant="outline"
              onClick={cancel}
              icon={<Loader2 className="size-4 animate-spin" aria-hidden />}
            >
              Cancel upload
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}