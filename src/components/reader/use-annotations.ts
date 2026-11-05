'use client';

import * as React from 'react';
import { toast } from 'sonner';

import { api, errorMessage } from '@/hooks/use-api';
import type { AnnotationColor, EpubAnnotation } from '@/types';

/**
 * The book's highlights and notes.
 *
 * Loaded once per book: the reader needs them for the open section, the study
 * panel needs them grouped by chapter, and the header needs a count. Creating or
 * editing one updates the local list immediately, because a highlight that takes
 * a round trip to appear reads as a failed click.
 */

export interface AnnotationInput {
  chapterIndex: number;
  startBlock: number;
  startOffset: number;
  endBlock: number;
  endOffset: number;
  quote: string;
  selectedText: string;
  language: string | null;
  color: AnnotationColor;
  note?: string | null;
}

export interface AnnotationsState {
  annotations: EpubAnnotation[];
  byChapter: Map<number, EpubAnnotation[]>;
  loading: boolean;
  refresh: () => Promise<void>;
  create: (input: AnnotationInput) => Promise<EpubAnnotation | null>;
  update: (id: string, patch: { color?: AnnotationColor; note?: string | null }) => Promise<void>;
  remove: (id: string) => Promise<void>;
}

export function useAnnotations(bookId: string, enabled: boolean): AnnotationsState {
  const [annotations, setAnnotations] = React.useState<EpubAnnotation[]>([]);
  const [loading, setLoading] = React.useState(false);

  const refresh = React.useCallback(async () => {
    if (!enabled) return;
    setLoading(true);
    try {
      const result = await api.get<{ annotations: EpubAnnotation[] }>(
        `/api/epub/${bookId}/annotations`,
      );
      setAnnotations(result.annotations);
    } catch (error) {
      toast.error(errorMessage(error, 'Could not load your highlights.'));
    } finally {
      setLoading(false);
    }
  }, [bookId, enabled]);

  React.useEffect(() => {
    void refresh();
  }, [refresh]);

  const byChapter = React.useMemo(() => {
    const groups = new Map<number, EpubAnnotation[]>();
    for (const annotation of annotations) {
      const list = groups.get(annotation.chapterIndex);
      if (list) list.push(annotation);
      else groups.set(annotation.chapterIndex, [annotation]);
    }
    return groups;
  }, [annotations]);

  const create = React.useCallback(
    async (input: AnnotationInput) => {
      try {
        const result = await api.post<{ annotation: EpubAnnotation }>(
          `/api/epub/${bookId}/annotations`,
          input,
        );
        setAnnotations((current) => [...current, result.annotation]);
        toast.success(input.note ? 'Note saved.' : 'Highlight saved.');
        return result.annotation;
      } catch (error) {
        toast.error(errorMessage(error, 'Could not save that highlight.'));
        return null;
      }
    },
    [bookId],
  );

  const update = React.useCallback(
    async (id: string, patch: { color?: AnnotationColor; note?: string | null }) => {
      let previous: EpubAnnotation | undefined;
      setAnnotations((current) =>
        current.map((annotation) => {
          if (annotation.id !== id) return annotation;
          previous = annotation;
          return { ...annotation, ...patch };
        }),
      );
      try {
        const result = await api.patch<{ annotation: EpubAnnotation }>(
          `/api/epub/${bookId}/annotations/${id}`,
          patch,
        );
        setAnnotations((current) =>
          current.map((annotation) => (annotation.id === id ? result.annotation : annotation)),
        );
        if (patch.note !== undefined) toast.success('Note saved.');
      } catch (error) {
        if (previous) {
          const restored = previous;
          setAnnotations((current) =>
            current.map((annotation) => (annotation.id === id ? restored : annotation)),
          );
        }
        toast.error(errorMessage(error, 'Could not update that highlight.'));
      }
    },
    [bookId],
  );

  const remove = React.useCallback(
    async (id: string) => {
      let removed: EpubAnnotation | undefined;
      setAnnotations((current) => {
        removed = current.find((annotation) => annotation.id === id);
        return current.filter((annotation) => annotation.id !== id);
      });
      try {
        await api.delete(`/api/epub/${bookId}/annotations/${id}`);
        toast.success('Highlight removed.');
      } catch (error) {
        if (removed) {
          const restored = removed;
          setAnnotations((current) => [...current, restored].sort(byPosition));
        }
        toast.error(errorMessage(error, 'Could not remove that highlight.'));
      }
    },
    [bookId],
  );

  return { annotations, byChapter, loading, refresh, create, update, remove };
}

function byPosition(a: EpubAnnotation, b: EpubAnnotation): number {
  return (
    a.chapterIndex - b.chapterIndex ||
    a.startBlock - b.startBlock ||
    a.startOffset - b.startOffset
  );
}
