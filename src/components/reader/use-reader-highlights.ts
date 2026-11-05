'use client';

import * as React from 'react';

import {
  buildReaderBlocks,
  buildTranslatedBlocks,
  findMatches,
  projectOffsets,
  rangeForBlockOffsets,
  snapToWords,
  type ReaderBlock,
} from './reader-index';
import type { SearchOptions } from './use-reader-search';
import type {
  AnnotationColor,
  EpubAnnotation,
  EpubChapterBlock,
  TranslationBlock,
} from '@/types';

/**
 * Painting highlights without touching the book's markup.
 *
 * The CSS Custom Highlight API is the right tool here: it styles text ranges
 * without wrapping them in elements, so the sanitised chapter HTML stays exactly
 * as the server produced it, React's reconciliation cannot break a highlight, and
 * the browser's own find-in-page and selection still work.
 *
 * The same pass that paints the highlights also keeps their `Range`s, which is
 * what makes a highlight clickable, and measures where each one sits so the
 * fore-edge rail can show markers.
 */

const COLORS: AnnotationColor[] = ['yellow', 'green', 'blue', 'pink', 'violet'];

/**
 * The `::highlight()` rules, injected at runtime.
 *
 * The build's CSS parser does not recognise the pseudo-element yet, and a
 * highlight has to be styled by the registry name it is painted under, so the
 * rules live together with the code that creates those names.
 */
const HIGHLIGHT_CSS = `
::highlight(vb-hl-yellow) { background-color: rgb(242 193 78 / 0.38); }
::highlight(vb-hl-green) { background-color: rgb(127 176 105 / 0.34); }
::highlight(vb-hl-blue) { background-color: rgb(106 159 216 / 0.34); }
::highlight(vb-hl-pink) { background-color: rgb(229 143 168 / 0.34); }
::highlight(vb-hl-violet) { background-color: rgb(164 139 212 / 0.36); }
::highlight(vb-hl-active) {
  background-color: rgb(242 193 78 / 0.5);
  text-decoration: underline;
  text-decoration-thickness: 2px;
  text-underline-offset: 0.18em;
}
::highlight(vb-search) { background-color: rgb(224 140 72 / 0.28); }
::highlight(vb-search-active) {
  background-color: rgb(224 112 72 / 0.55);
  color: inherit;
}
`;

const HIGHLIGHT_KEYS = [
  ...COLORS.map((color) => `vb-hl-${color}`),
  'vb-hl-active',
  'vb-search',
  'vb-search-active',
];

interface HighlightRegistry {
  set(name: string, highlight: unknown): void;
  delete(name: string): boolean;
}

type HighlightConstructor = new (...ranges: Range[]) => unknown;

function registry(): HighlightRegistry | null {
  if (typeof CSS === 'undefined') return null;
  return (CSS as unknown as { highlights?: HighlightRegistry }).highlights ?? null;
}

function highlightClass(): HighlightConstructor | null {
  if (typeof window === 'undefined') return null;
  return (window as unknown as { Highlight?: HighlightConstructor }).Highlight ?? null;
}

export interface ReaderMarker {
  annotation: EpubAnnotation;
  /** Where down the section the marker sits, 0..1. */
  percent: number;
}

export interface ActiveSearchHit {
  blockIndex: number;
  startOffset: number;
  endOffset: number;
  source: 'original' | 'translation';
}

export interface ReaderHighlightsState {
  ranges: Map<string, Range[]>;
  markers: ReaderMarker[];
  /** Whether the browser can paint highlights at all. */
  supported: boolean;
}

interface Options {
  contentRef: React.RefObject<HTMLElement | null>;
  scrollRef: React.RefObject<HTMLElement | null>;
  chapterIndex: number;
  sourceBlocks: EpubChapterBlock[];
  translationBlocks: TranslationBlock[] | null;
  showingTranslation: boolean;
  annotations: EpubAnnotation[];
  activeId: string | null;
  searchQuery: string;
  searchOptions: SearchOptions;
  activeSearch: ActiveSearchHit | null;
  /**
   * Changes when typography or the column changes. The text itself is memoised
   * so its `Range`s survive ordinary re-renders, which means a reflow does not
   * repaint on its own and the marker positions have to be measured again.
   */
  layoutKey: string;
  enabled: boolean;
}

export function useReaderHighlights(options: Options): ReaderHighlightsState {
  const {
    contentRef,
    scrollRef,
    chapterIndex,
    sourceBlocks,
    translationBlocks,
    showingTranslation,
    annotations,
    activeId,
    searchQuery,
    searchOptions,
    activeSearch,
    layoutKey,
    enabled,
  } = options;

  const [state, setState] = React.useState<ReaderHighlightsState>({
    ranges: new Map(),
    markers: [],
    supported: false,
  });
  const [layoutTick, setLayoutTick] = React.useState(0);

  // One stylesheet for every reader on the page; removing it with the last
  // mount leaves the app as it was found.
  React.useEffect(() => {
    const style = document.createElement('style');
    style.setAttribute('data-reader-highlights', '');
    style.textContent = HIGHLIGHT_CSS;
    document.head.appendChild(style);
    return () => style.remove();
  }, []);

  // Re-measure the rail when the window changes size; type size and theme
  // changes are React re-renders and are covered by the main effect.
  React.useEffect(() => {
    const bump = () => setLayoutTick((tick) => tick + 1);
    window.addEventListener('resize', bump);
    return () => window.removeEventListener('resize', bump);
  }, []);

  const annotationKey = annotations
    .filter((annotation) => annotation.chapterIndex === chapterIndex)
    .map((annotation) => `${annotation.id}:${annotation.color}:${annotation.startBlock}:${annotation.startOffset}:${annotation.endBlock}:${annotation.endOffset}`)
    .join('|');

  React.useLayoutEffect(() => {
    const root = contentRef.current;
    const scroll = scrollRef.current;
    const canPaint = highlightClass() !== null && registry() !== null;

    if (!root || !scroll || !enabled) {
      const Registry = registry();
      if (Registry) {
        for (const key of HIGHLIGHT_KEYS) Registry.delete(key);
      }
      setState({ ranges: new Map(), markers: [], supported: canPaint });
      return;
    }

    const blocks: ReaderBlock[] = showingTranslation
      ? buildTranslatedBlocks(root)
      : buildReaderBlocks(root, sourceBlocks);
    const visibleByIndex = new Map<number, ReaderBlock>();
    for (const block of blocks) {
      if (block.serverIndex >= 0) visibleByIndex.set(block.serverIndex, block);
    }

    const ranges = new Map<string, Range[]>();
    const byColor = new Map<AnnotationColor, Range[]>();

    const current = annotations.filter((annotation) => annotation.chapterIndex === chapterIndex);
    for (const annotation of current) {
      const list: Range[] = [];
      for (let index = annotation.startBlock; index <= annotation.endBlock; index += 1) {
        const block = visibleByIndex.get(index);
        if (!block) continue;

        const sourceText = sourceBlocks[index]?.text ?? '';
        if (!sourceText) continue;

        let start: number;
        let end: number;
        if (showingTranslation) {
          const sourceStart = index === annotation.startBlock ? annotation.startOffset : 0;
          const sourceEnd = index === annotation.endBlock ? annotation.endOffset : sourceText.length;
          ({ start, end } = projectOffsets(sourceText.length, block.text.length, sourceStart, sourceEnd));
          ({ start, end } = snapToWords(block.text, start, end));
        } else {
          start = index === annotation.startBlock ? annotation.startOffset : 0;
          end = index === annotation.endBlock ? annotation.endOffset : block.text.length;
        }

        const range = rangeForBlockOffsets(block, start, end);
        if (range) list.push(range);
      }
      if (list.length > 0) {
        ranges.set(annotation.id, list);
        const group = byColor.get(annotation.color);
        if (group) group.push(...list);
        else byColor.set(annotation.color, [...list]);
      }
    }

    const searchRanges: Range[] = [];
    const needle = searchQuery.trim();
    if (needle.length >= 2) {
      for (const block of blocks) {
        for (const match of findMatches(block.text, needle, searchOptions)) {
          const range = rangeForBlockOffsets(block, match.start, match.end);
          if (range) searchRanges.push(range);
        }
      }
    }

    let activeSearchRange: Range | null = null;
    if (activeSearch) {
      const matchesView =
        activeSearch.source === 'translation' ? showingTranslation : !showingTranslation;
      if (matchesView) {
        const block = visibleByIndex.get(activeSearch.blockIndex);
        if (block) {
          activeSearchRange = rangeForBlockOffsets(block, activeSearch.startOffset, activeSearch.endOffset);
        }
      }
    }

    if (canPaint) {
      const Registry = registry()!;
      const Highlight = highlightClass()!;
      for (const key of HIGHLIGHT_KEYS) Registry.delete(key);

      for (const color of COLORS) {
        const list = byColor.get(color);
        if (list?.length) Registry.set(`vb-hl-${color}`, new Highlight(...list));
      }
      const active = activeId ? ranges.get(activeId) : undefined;
      if (active?.length) Registry.set('vb-hl-active', new Highlight(...active));
      if (searchRanges.length) Registry.set('vb-search', new Highlight(...searchRanges));
      if (activeSearchRange) Registry.set('vb-search-active', new Highlight(activeSearchRange));
    }

    const scrollRect = scroll.getBoundingClientRect();
    const height = Math.max(1, scroll.scrollHeight);
    const markers: ReaderMarker[] = [];
    for (const annotation of current) {
      const list = ranges.get(annotation.id);
      const first = list?.[0];
      if (!first) continue;
      const block = visibleByIndex.get(annotation.startBlock);
      if (!block) continue;
      const top =
        block.el.getBoundingClientRect().top - scrollRect.top + scroll.scrollTop + 6;
      markers.push({
        annotation,
        percent: Math.min(0.995, Math.max(0.005, top / height)),
      });
    }
    markers.sort((a, b) => a.percent - b.percent);

    setState({ ranges, markers, supported: canPaint });
  }, [
    activeId,
    activeSearch,
    annotationKey,
    annotations,
    chapterIndex,
    contentRef,
    enabled,
    layoutKey,
    layoutTick,
    scrollRef,
    searchOptions,
    searchQuery,
    showingTranslation,
    sourceBlocks,
    translationBlocks,
  ]);

  // A section that is replaced while a search result is being landed on
  // re-creates the ranges; the cleanup below would otherwise leave the last
  // chapter's ranges registered against detached nodes.
  React.useEffect(() => {
    return () => {
      const Registry = registry();
      if (!Registry) return;
      for (const key of HIGHLIGHT_KEYS) Registry.delete(key);
    };
  }, []);

  return state;
}
