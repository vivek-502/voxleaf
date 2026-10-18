import * as React from 'react';
import { BookOpen, ChevronDown, FileText, Loader2, RefreshCw, Trash2, Upload } from 'lucide-react';
import { toast } from 'sonner';

import { Button } from '@/components/ui/button';
import { ConfirmDialog } from '@/components/common/confirm-dialog';
import { UploadEpubDialog } from '@/components/book/upload-epub-dialog';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { useEpubStatus } from '@/hooks/use-epub';
import { api, errorMessage } from '@/hooks/use-api';
import { formatBytes, pluralize } from '@/utils/format';
import { cn } from '@/utils/cn';

/**
 * The book-file control for the book page.
 *
 * VoxLeaf ships no catalogue and fetches nothing, so a book's text is the EPUB
 * the reader uploads. That gives this control two shapes:
 *
 *  - While there is no usable file, it is a full-width card asking for one,
 *    because that is the one thing that makes the book readable.
 *  - Once the text is ready, the card goes away. It only ever repeated the
 *    "Read" button already in the header, so what is left is a small menu for
 *    the occasional times the file needs managing: replace, rebuild, remove.
 */
export function EpubStatusPanel({ bookId }: { bookId: string }) {
  const { status, loading, remove, refresh } = useEpubStatus(bookId);
  const [removeOpen, setRemoveOpen] = React.useState(false);
  const [reparsing, setReparsing] = React.useState(false);
  const [uploadOpen, setUploadOpen] = React.useState(false);
  // Only meaningful for the ready state, where the upload is a replacement.
  const [replacing, setReplacing] = React.useState(false);

  const openUpload = (asReplacement: boolean) => {
    setReplacing(asReplacement);
    setUploadOpen(true);
  };

  const reparse = async () => {
    setReparsing(true);
    try {
      const result = await api.post<{ error: string | null }>(`/api/books/${bookId}/epub`, {
        action: 'reparse',
      });
      if (result.error) {
        toast.error(result.error);
      } else {
        toast.success('Book text rebuilt from the stored file.');
      }
      refresh();
    } catch (err) {
      toast.error(errorMessage(err, 'Could not rebuild the text.'));
    } finally {
      setReparsing(false);
    }
  };

  const dialogs = (
    <>
      <UploadEpubDialog
        open={uploadOpen}
        onOpenChange={setUploadOpen}
        bookId={bookId}
        bookTitle={status?.title ?? 'this book'}
        replacing={replacing}
        onUploaded={refresh}
      />
      <ConfirmDialog
        open={removeOpen}
        onOpenChange={setRemoveOpen}
        title="Remove the book file?"
        description="The EPUB, its sections, images and reading position are deleted. Your chapters and audio are untouched, and you can add the file again at any time."
        confirmLabel="Remove book file"
        destructive
        onConfirm={remove}
      />
    </>
  );

  if (loading && !status) {
    return (
      <div className="card-surface flex w-full items-center gap-3 p-4">
        <Loader2 className="size-4 shrink-0 animate-spin text-ink-subtle" aria-hidden />
        <p className="text-sm text-ink-muted">Checking for the book file…</p>
      </div>
    );
  }

  if (status?.enabled === false) {
    return (
      <p className="w-full text-sm text-ink-subtle">
        Book files are turned off on this instance, so only audio is available.
      </p>
    );
  }

  /* Ready: no card, just the management menu. */

  if (status?.hasEpub) {
    return (
      <>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="outline" icon={<FileText className="size-4" aria-hidden />}>
              Book file
              <ChevronDown className="size-3.5 text-ink-subtle" aria-hidden />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="w-64">
            <DropdownMenuLabel>
              <span className="block truncate text-sm font-medium normal-case tracking-normal text-ink">
                {status.title ?? 'Book file'}
              </span>
              <span className="mt-0.5 block truncate text-xs font-normal normal-case tracking-normal text-ink-subtle">
                {pluralize(status.chapterCount, 'section')}
                {status.imageCount > 0 && `, ${pluralize(status.imageCount, 'image')}`}
                {status.fileSize ? ` · ${formatBytes(status.fileSize)}` : ''}
              </span>
            </DropdownMenuLabel>
            <DropdownMenuSeparator />
            <DropdownMenuItem onSelect={() => openUpload(true)}>
              <Upload aria-hidden /> Replace EPUB…
            </DropdownMenuItem>
            <DropdownMenuItem onSelect={() => void reparse()} disabled={reparsing}>
              <RefreshCw className={cn(reparsing && 'animate-spin')} aria-hidden /> Rebuild text
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem destructive onSelect={() => setRemoveOpen(true)}>
              <Trash2 aria-hidden /> Remove file
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
        {dialogs}
      </>
    );
  }

  /* No usable file: the one action that makes the book readable. */

  return (
    <div className="card-surface flex w-full flex-wrap items-center gap-3 p-4">
      <BookOpen className="size-5 shrink-0 text-ink-subtle" aria-hidden />
      <div className="min-w-0 flex-1">
        <p className="text-sm font-medium text-ink">
          {status?.status === 'failed' ? 'Book file unavailable' : 'No book file yet'}
        </p>
        <p className="mt-0.5 text-xs leading-relaxed text-ink-subtle">
          {status?.error ??
            'Upload this book’s EPUB to read it here, with its layout and images intact.'}
        </p>
      </div>
      <Button
        size="sm"
        onClick={() => openUpload(false)}
        icon={<Upload className="size-3.5" aria-hidden />}
      >
        Upload EPUB
      </Button>
      {dialogs}
    </div>
  );
}
