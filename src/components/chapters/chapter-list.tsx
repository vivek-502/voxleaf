'use client';

import * as React from 'react';
import { toast } from 'sonner';
import {
  DndContext,
  KeyboardSensor,
  PointerSensor,
  closestCenter,
  useSensor,
  useSensors,
  type DragEndEvent,
} from '@dnd-kit/core';
import {
  SortableContext,
  arrayMove,
  sortableKeyboardCoordinates,
  useSortable,
  verticalListSortingStrategy,
} from '@dnd-kit/sortable';
import { CSS } from '@dnd-kit/utilities';
import { Plus, UploadCloud } from 'lucide-react';

import { useTtsGeneration } from '@/hooks/use-tts';
import { ChapterRow } from './chapter-row';
import { AddChapterDialog } from './add-chapter-dialog';
import { UploadAudioDialog } from './upload-audio-dialog';
import { EditChapterDialog } from './edit-chapter-dialog';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/primitives';
import { EmptyState, ChapterListSkeleton, ErrorState } from '@/components/common/states';
import { SectionHeader } from '@/components/common/section-header';
import { NarrateDialog } from '@/components/tts/narrate-dialog';
import { api, errorMessage } from '@/hooks/use-api';
import { useEpubStatus } from '@/hooks/use-epub';
import { useNarrateChapter, type NarrateTarget } from '@/hooks/use-narrate-chapter';
import { formatDuration, pluralize } from '@/utils/format';
import { cn } from '@/utils/cn';
import type { ChapterInfo } from '@/types';

export interface ChapterListProps {
  bookId: string;
  libraryId: string;
  bookTitle: string;
  chapters: ChapterInfo[];
  loading?: boolean;
  error?: string | null;
  onRetry?: () => void;
  /** Any unsaved-changes guard the parent wants to apply. */
  emptyAction?: React.ReactNode;
}

/**
 * The chapter management surface for one book.
 *
 * Reordering is drag-and-drop via dnd-kit, with a keyboard path (space to
 * lift, arrows to move, space to drop) provided by its sortable keyboard
 * sensor — reordering is not a pointer-only capability.
 */
export function ChapterList({
  bookId,
  libraryId,
  bookTitle,
  chapters,
  loading = false,
  error = null,
  onRetry,
}: ChapterListProps) {
  const [addOpen, setAddOpen] = React.useState(false);
  const [uploadTarget, setUploadTarget] = React.useState<ChapterInfo | null>(null);
  const [editTarget, setEditTarget] = React.useState<ChapterInfo | null>(null);
  const [reloadToken, setReloadToken] = React.useState(0);
  const [autoSyncing, setAutoSyncing] = React.useState(false);
  // Only the `hasEpub` flag is needed here; the book page owns the file itself.
  const { status: epubStatus, loading: epubLoading } = useEpubStatus(bookId);
  const hasBookFile = epubStatus?.hasEpub === true;

  // One button, one question. The chooser fetches the saved translations of the
  // chapter that was pressed and skips the dialog entirely when there are none
  // and the configured voice can read the original, so "narrate this chapter"
  // never means "narrate the original by surprise" for a chapter that exists in
  // another language — or read it in a voice that cannot pronounce it.
  const narration = useNarrateChapter(bookId);

  // Local order mirrors the server order while dragging; the server is the
  // source of truth once a drop commits.
  const [order, setOrder] = React.useState<string[]>(() => chapters.map((c) => c.id));

  React.useEffect(() => {
    setOrder(chapters.map((c) => c.id));
  }, [chapters]);

  const sensors = useSensors(
    // A small activation distance keeps click-to-play working on the row.
    useSensor(PointerSensor, { activationConstraint: { distance: 6 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  );

  const byId = React.useMemo(() => new Map(chapters.map((c) => [c.id, c])), [chapters]);
  const ordered = order
    .map((id) => byId.get(id))
    .filter((c): c is ChapterInfo => Boolean(c));

  const withAudio = chapters.filter((c) => c.audio).length;
  const totalDuration = chapters.reduce((sum, c) => sum + (c.audio?.duration ?? 0), 0);

  const handleDragEnd = async (event: DragEndEvent) => {
    const { active, over } = event;
    if (!over || active.id === over.id) return;

    const oldIndex = order.indexOf(String(active.id));
    const newIndex = order.indexOf(String(over.id));
    if (oldIndex < 0 || newIndex < 0) return;

    const previous = order;
    const next = arrayMove(order, oldIndex, newIndex);
    setOrder(next); // optimistic

    try {
      await api.post('/api/chapters/reorder', {
        libraryId,
        orderedChapterIds: next,
      });
    } catch (caught) {
      setOrder(previous); // roll back
      toast.error(errorMessage(caught, 'Could not save the new chapter order.'));
      setReloadToken((token) => token + 1);
    }
  };

  const refresh = React.useCallback(() => setReloadToken((token) => token + 1), []);

  // One generator for the whole list, so a row can show that its own chapter is
  // the one being narrated and the list refreshes once when any of them finish.
  const { busyChapters, generate } = useTtsGeneration(refresh);

  const narrate = (chapter: ChapterInfo) => {
    const target: NarrateTarget = {
      epubIndex: chapter.epubIndex,
      title: chapter.title,
      chapterNumber: chapter.chapterNumber,
    };
    void narration.request(target, (language) =>
      generate(chapter.id, `Chapter ${chapter.chapterNumber}`, language),
    );
  };

  /**
   * Fill an empty chapter list from the book's table of contents.
   *
   * Uploading an EPUB now does this on the server, so this is the safety net for
   * books whose file was stored before that shipped, and for the moment the
   * upload response and the chapter query race. It runs only when the list is
   * empty — there is nothing to destroy — and at most once per book, so a book
   * whose contents turned out to be empty cannot spin here.
   */
  const autoSyncedRef = React.useRef(false);
  React.useEffect(() => {
    if (autoSyncedRef.current) return;
    if (epubLoading || !hasBookFile || chapters.length > 0) return;
    autoSyncedRef.current = true;
    setAutoSyncing(true);
    void (async () => {
      try {
        await api.post(`/api/epub/${bookId}/chapters`);
        refresh();
        onRetry?.();
      } catch {
        // No table of contents to read from; the manual empty state stays.
      } finally {
        setAutoSyncing(false);
      }
    })();
  }, [bookId, epubLoading, hasBookFile, chapters.length, refresh, onRetry]);

  // The player and the full player's "no audio" interstitial both dispatch
  // `voicebook:import-audio`; route it to this book's upload dialog. Declared
  // before the early returns below so hook order stays stable.
  React.useEffect(() => {
    function onImportAudio(event: Event) {
      const detail = (event as CustomEvent<{ bookId?: string; chapterId?: string }>).detail;
      if (!detail?.chapterId || detail.bookId !== bookId) return;
      const chapter = byId.get(detail.chapterId);
      // An unknown chapter id means the list is stale; refresh and let the user
      // retry rather than opening an upload form bound to nothing.
      if (!chapter) {
        refresh();
        return;
      }
      setUploadTarget(chapter);
    }

    window.addEventListener('voicebook:import-audio', onImportAudio);
    return () => window.removeEventListener('voicebook:import-audio', onImportAudio);
  }, [bookId, byId, refresh]);

  if (loading) return <ChapterListSkeleton count={Math.max(4, chapters.length || 6)} />;

  if (error) return <ErrorState title="Could not load chapters" message={error} onRetry={onRetry} />;

  // An empty list is about to be filled from the book's own contents, so show
  // the skeleton rather than an "add chapters" prompt that is about to vanish.
  if (chapters.length === 0 && (epubLoading || autoSyncing)) {
    return <ChapterListSkeleton count={6} />;
  }

  return (
    <section aria-labelledby="chapters-heading" data-reload={reloadToken}>
      <SectionHeader
        title="Chapters"
        description={
          chapters.length === 0
            ? undefined
            : `${pluralize(chapters.length, 'chapter')} · ${withAudio} with audio${
                totalDuration > 0 ? ` · ${formatDuration(totalDuration)}` : ''
              }`
        }
        action={
          <Button onClick={() => setAddOpen(true)} icon={<Plus className="size-4" aria-hidden />}>
            Add chapters
          </Button>
        }
      />

      {chapters.length === 0 ? (
        <EmptyState
          icon={UploadCloud}
          title="No chapters yet"
          description={
            hasBookFile
              ? 'This book has no table of contents to build chapters from. Add them by hand, then import an audio file for each one.'
              : 'Add the chapter list manually, then import an audio file for each chapter. Upload the book’s EPUB and its contents are listed here automatically.'
          }
          action={
            <Button onClick={() => setAddOpen(true)} icon={<Plus className="size-4" aria-hidden />}>
              Add your first chapter
            </Button>
          }
        />
      ) : (
        <>
          {withAudio < chapters.length && (
            <div className="mb-5 flex flex-wrap items-baseline gap-x-3 gap-y-1 border-l-2 border-caution/60 pl-3">
              <Badge variant="caution">{chapters.length - withAudio} missing audio</Badge>
              <p className="text-sm text-ink-muted">
                {withAudio === 0
                  ? 'Narrate at least one chapter, or import an audio file, to start listening.'
                  : 'Narrate or import the rest, or just play the ones that are ready.'}
              </p>
            </div>
          )}

          <DndContext
            sensors={sensors}
            collisionDetection={closestCenter}
            onDragEnd={(event) => void handleDragEnd(event)}
          >
            <SortableContext items={order} strategy={verticalListSortingStrategy}>
              <ul className="flex flex-col border-t border-ink">
                {ordered.map((chapter) => (
                  <SortableChapterRow
                    key={chapter.id}
                    chapter={chapter}
                    bookId={bookId}
                    onUpload={setUploadTarget}
                    onEdit={setEditTarget}
                    onGenerateTts={narrate}
                    generatingTts={busyChapters.has(chapter.id)}
                    onChanged={refresh}
                  />
                ))}
              </ul>
            </SortableContext>
          </DndContext>

          <p className="mt-3 font-mono text-[10px] uppercase tracking-[0.1em] text-ink-subtle">
            Drag the handle to reorder, or focus it and use the arrow keys.
          </p>
        </>
      )}

      <AddChapterDialog
        open={addOpen}
        onOpenChange={setAddOpen}
        libraryId={libraryId}
        existingCount={chapters.length}
        onCreated={refresh}
      />

      <UploadAudioDialog
        open={Boolean(uploadTarget)}
        onOpenChange={(open) => {
          if (!open) setUploadTarget(null);
        }}
        chapterId={uploadTarget?.id ?? ''}
        chapterNumber={uploadTarget?.chapterNumber ?? 0}
        chapterTitle={uploadTarget?.title ?? ''}
        bookTitle={bookTitle}
        onUploaded={refresh}
        onGenerateTts={
          uploadTarget
            ? () => {
                const target = uploadTarget;
                setUploadTarget(null);
                narrate(target);
              }
            : undefined
        }
        generatingTts={uploadTarget ? busyChapters.has(uploadTarget.id) : false}
      />

      <NarrateDialog
        target={narration.target}
        entries={narration.entries}
        source={narration.source}
        loading={narration.loading}
        preferred={narration.preferred}
        onOpenChange={(open) => {
          if (!open) narration.close();
        }}
        onStart={narration.start}
      />

      <EditChapterDialog
        chapter={editTarget}
        onOpenChange={(open) => {
          if (!open) setEditTarget(null);
        }}
        onSaved={refresh}
      />
    </section>
  );
}

/**
 * dnd-kit requires the item component to be a direct `useSortable` consumer;
 * it renders the presentational `ChapterRow` and forwards the drag handle.
 */
function SortableChapterRow({ chapter, ...rest }: Omit<ChapterRowProps, 'sortable' | 'dragHandleProps' | 'isDragging'>) {
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({
    id: chapter.id,
  });

  return (
    <div
      ref={setNodeRef}
      style={{ transform: CSS.Translate.toString(transform), transition }}
      className={cn('list-none', isDragging && 'z-10')}
    >
      <ChapterRow
        chapter={chapter}
        {...rest}
        sortable
        isDragging={isDragging}
        // Only the handle is the drag source, so the row's buttons keep working.
        dragHandleProps={{ ...attributes, ...listeners }}
      />
    </div>
  );
}

type ChapterRowProps = React.ComponentProps<typeof ChapterRow>;
