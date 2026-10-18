import * as React from 'react';
import { toast } from 'sonner';
import { FileAudio, Loader2, UploadCloud, X, Sparkles } from 'lucide-react';

import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { FieldDescription, FieldError } from '@/components/ui/inputs';
import { ProgressBar } from '@/components/book/book-card';
import { api, errorMessage } from '@/hooks/use-api';
import {
  ACCEPTED_AUDIO_EXTENSIONS,
  ACCEPTED_AUDIO_MIME,
  AUDIO_ACCEPT_ATTRIBUTE} from '@/utils/constants';
import { formatBytes, formatDuration } from '@/utils/format';
import { cn } from '@/utils/cn';

interface AudioPreview {
  file: File;
  name: string;
  size: number;
  duration: number | null;
  mime: string;
}

export interface UploadAudioDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  chapterId: string;
  chapterNumber: number;
  chapterTitle: string;
  bookTitle: string;
  onUploaded?: () => void;
  /** Offered as an alternative to uploading a file. */
  onGenerateTts?: () => void;
  generatingTts?: boolean;
}

/**
 * Drag-and-drop audio upload.
 *
 * Client-side validation is just a hint (extension/MIME). The server
 * re-validates and also probes the actual audio stream via music-metadata to
 * reject corrupted files and to fill in the duration.
 */
export function UploadAudioDialog({
  open,
  onOpenChange,
  chapterId,
  chapterNumber,
  chapterTitle,
  bookTitle,
  onUploaded,
  onGenerateTts,
  generatingTts = false,
}: UploadAudioDialogProps) {
  const [file, setFile] = React.useState<AudioPreview | null>(null);
  const [progress, setProgress] = React.useState(0);
  const [state, setState] = React.useState<'idle' | 'uploading' | 'error'>('idle');
  const [error, setError] = React.useState<string | null>(null);
  const inputRef = React.useRef<HTMLInputElement>(null);
  const controller = React.useRef<AbortController | null>(null);
  const [durationHint, setDurationHint] = React.useState<number | null>(null);

  const reset = () => {
    setFile(null);
    setProgress(0);
    setState('idle');
    setError(null);
    setDurationHint(null);
    if (controller.current) controller.current.abort();
  };

  const close = (next: boolean) => {
    if (!next) reset();
    onOpenChange(next);
  };

  const pick = (event: React.ChangeEvent<HTMLInputElement>) => {
    const selected = event.target.files?.[0];
    if (selected) {
      void probe(selected);
    }
    if (inputRef.current) inputRef.current.value = '';
  };

  const probe = async (raw: File) => {
    setError(null);
    const lowerName = raw.name.toLowerCase();
    const hasExt = ACCEPTED_AUDIO_EXTENSIONS.some((ext) => lowerName.endsWith(ext));
    if (!hasExt && !ACCEPTED_AUDIO_MIME.includes(raw.type)) {
      setError('Unsupported audio format. MP3, M4A, WAV, OGG or FLAC are accepted.');
      return;
    }

    let duration: number | null = null;
    try {
      // Use HTML5 to sniff duration if the browser can decode a small slice.
      // This is a nice UX hint; the server re-measures.
      const url = URL.createObjectURL(raw);
      const audio = new Audio();
      audio.src = url;
      await new Promise<void>((resolve) => {
        const onMetadata = () => {
          duration = isFinite(audio.duration) && audio.duration > 0 ? audio.duration : null;
          URL.revokeObjectURL(url);
          audio.removeEventListener('loadedmetadata', onMetadata);
          resolve();
        };
        audio.addEventListener('loadedmetadata', onMetadata, { once: true });
        setTimeout(() => {
          URL.revokeObjectURL(url);
          resolve();
        }, 2000);
      });
    } catch {
      duration = null;
    }

    setFile({
      file: raw,
      name: raw.name,
      size: raw.size,
      duration,
      mime: raw.type || 'application/octet-stream'});
    setDurationHint(duration);
    setProgress(0);
    setState('idle');
  };

  const onDrop = (event: React.DragEvent) => {
    event.preventDefault();
    event.stopPropagation();
    const dropped = event.dataTransfer.files?.[0];
    if (dropped) void probe(dropped);
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
        audio: {
          id: string;
          duration: number;
          fileSize: number;
        };
        replaced: boolean;
      }>(
        `/api/chapters/${chapterId}/audio`,
        file.file,
        {},
        (p) => setProgress(p),
        controllerInstance.signal,
      );
      toast.success(
        result.replaced ? 'Audio replaced successfully.' : `Audio uploaded for Ch. ${chapterNumber}.`,
      );
      onUploaded?.();
      close(false);
    } catch (caught) {
      if (controllerInstance.signal.aborted) return;
      const msg = errorMessage(caught, 'The audio upload failed.');
      setError(msg);
      setState('error');
      toast.error(msg);
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
          <DialogTitle>Import audio for Chapter {chapterNumber}</DialogTitle>
          <DialogDescription>
            Upload an audio file for “{chapterTitle}” from {bookTitle}. Uploading again will
            replace the existing audio.
          </DialogDescription>
        </DialogHeader>

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
            <h3 className="mt-4 text-base font-semibold text-ink">Drag and drop your audio file here</h3>
            <p className="mt-1 text-sm text-ink-muted">
              or choose a file — MP3, M4A/AAC, WAV, OGG/Opus, FLAC
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
              accept={AUDIO_ACCEPT_ATTRIBUTE}
              className="sr-only-focusable"
              onChange={pick}
            />
            {onGenerateTts && (
              <Button
                type="button"
                variant="outline"
                disabled={generatingTts}
                icon={
                  generatingTts ? (
                    <Loader2 className="size-4 animate-spin" aria-hidden />
                  ) : (
                    <Sparkles className="size-4" aria-hidden />
                  )
                }
                onClick={() => {
                  onGenerateTts();
                  close(false);
                }}
              >
                {generatingTts ? 'Generating…' : 'Narrate with TTS'}
              </Button>
            )}
            {error && (
              <FieldError className="mt-4 max-w-sm text-center">{error}</FieldError>
            )}
          </div>
        ) : (
          <div className="space-y-5">
            <div className="flex items-start gap-3 rounded-2xl border border-hairline bg-raised p-4">
              <span className="grid size-10 shrink-0 place-items-center rounded-xl bg-overlay text-accent-soft">
                <FileAudio className="size-5" aria-hidden />
              </span>
              <div className="min-w-0 flex-1">
                <p className="truncate text-sm font-medium text-ink">{file.name}</p>
                <p className="mt-0.5 text-xs text-ink-subtle">
                  {formatBytes(file.size)}
                  {durationHint ? ` · ~${formatDuration(durationHint)}` : ''}
                  {file.mime ? ` · ${file.mime}` : ''}
                </p>
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
                <FieldDescription>Uploading… this may take a moment for large files.</FieldDescription>
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
              Upload audio
            </Button>
          )}
          {state === 'uploading' && (
            <Button type="button" variant="outline" onClick={cancel} icon={<Loader2 className="size-4 animate-spin" aria-hidden />}>
              Cancel upload
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
