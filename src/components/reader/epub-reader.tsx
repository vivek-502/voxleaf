'use client';

import * as React from 'react';
import Link from 'next/link';
import { toast } from 'sonner';
import {
  ArrowLeft,
  Bookmark,
  ChevronLeft,
  ChevronRight,
  Ellipsis,
  FileAudio,
  Languages,
  List,
  Loader2,
  Pause,
  Play,
  Search,
  Sparkles,
  Type,
  Upload,
} from 'lucide-react';

import { AnnotationCard } from './annotation-card';
import { ContentsPanel } from './contents-panel';
import { activeRow, buildContentsRows, type ContentsRow } from './contents-rows';
import { DictionaryDialog, type DictionaryRequest } from './dictionary-dialog';
import { NoteDialog } from './note-dialog';
import {
  buildReaderBlocks,
  buildTranslatedBlocks,
  projectOffsets,
  rangeForBlockOffsets,
  readSelection,
  rectForRange,
  snapToWords,
  toSourceAnchor,
  type ReaderBlock,
} from './reader-index';
import { SelectionToolbar, type SelectionDraft } from './selection-toolbar';
import { StudyPanel, type StudyTab } from './study-panel';
import { useAnnotations } from './use-annotations';
import { useBookmarks } from './use-bookmarks';
import { useReaderHighlights, type ReaderMarker } from './use-reader-highlights';
import { useReaderSearch } from './use-reader-search';
import { useSavedWords } from './use-saved-words';
import {
  ReaderSettings,
  DEFAULT_PREFERENCES,
  READER_FONT_STACKS,
  type ReaderPreferences,
} from './reader-settings';
import { TranslateDialog } from './translate-dialog';
import { TranslatedText } from './translated-text';
import { useTranslations } from './use-translations';
import {
  blockAtPage,
  measurePageFill,
  MAX_PAGE_BLOCK_SIZE,
  MAX_PAGE_INLINE_SIZE,
  pageLayout,
  pageOfElement,
  pageOfRange,
  type PageLayout,
} from './pagination';
import { blockAtOffset, buildTextIndex, type TextBlock } from './text-index';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { UploadAudioDialog } from '@/components/chapters/upload-audio-dialog';
import { NarrateDialog } from '@/components/tts/narrate-dialog';
import { usePlayer } from '@/components/providers/player-provider';
import { api, errorMessage } from '@/hooks/use-api';
import { useEpubStatus } from '@/hooks/use-epub';
import { useNarrateChapter } from '@/hooks/use-narrate-chapter';
import { useSpeak } from '@/hooks/use-speak';
import { useTtsGeneration } from '@/hooks/use-tts';
import { clamp, debounce, formatDuration } from '@/utils/format';
import { cn } from '@/utils/cn';
import type {
  AnnotationColor,
  EpubAnnotation,
  EpubAudioEntry,
  EpubBookmarkInfo,
  EpubChapterBlock,
  EpubChapterEntry,
  EpubChapterPayload,
  EpubProgressInfo,
  EpubSearchResult,
  TranslationBlock,
} from '@/types';

/** Debounce for the position write. */
const SAVE_DEBOUNCE_MS = 700;

/** Below this the contents sits over the text; above it, beside it. */
const DOCK_MIN_WIDTH = 900;

/** Stable empty array, so a missing section does not re-run highlight effects. */
const EMPTY_BLOCKS: EpubChapterBlock[] = [];

/** Fore-edge marker colours, matching the highlight palette. */
const MARKER_COLORS: Record<AnnotationColor, string> = {
  yellow: '#f2c14e',
  green: '#7fb069',
  blue: '#6a9fd8',
  pink: '#e58fa8',
  violet: '#a48bd4',
};

interface NoteComposer {
  mode: 'create' | 'edit';
  id?: string;
  quote: string;
  note: string;
  color: AnnotationColor;
  /** The selection a new note is being made from. */
  selection?: SelectionDraft;
}

/**
 * The measured spread.
 *
 * `width` and `height` are the text area — what the columns actually fill —
 * and the frame is the paper around it. Kept in one object so the sheet and
 * the page it holds can never be measured against different sizes.
 */
interface PageBox {
  width: number;
  height: number;
  frameX: number;
  frameY: number;
}

interface PendingSearchHit {
  chapterIndex: number;
  blockIndex: number;
  startOffset: number;
  endOffset: number;
  source: 'original' | 'translation';
}

/**
 * The chapter's text, isolated from the rest of the reader's rendering.
 *
 * Any render of a `dangerouslySetInnerHTML` subtree rewrites its children, and
 * that detaches every `Range` the highlighter holds — the highlights then paint
 * nothing. The reader re-renders constantly (scroll progress, chrome, panels),
 * so the markup is memoised on its own value and only replaced when the section
 * or the language actually changes.
 */
const ChapterText = React.memo(function ChapterText({
  html,
  contentRef,
}: {
  html: string;
  contentRef: React.RefObject<HTMLDivElement | null>;
}) {
  return (
    <div ref={contentRef} data-reader-content>
      <div dangerouslySetInnerHTML={{ __html: html }} />
    </div>
  );
});

/** The same isolation for a translated section's blocks. */
const TranslatedChapterText = React.memo(function TranslatedChapterText({
  blocks,
  sourceIndices,
  contentRef,
}: {
  blocks: TranslationBlock[];
  sourceIndices: number[];
  contentRef: React.RefObject<HTMLDivElement | null>;
}) {
  return (
    <div ref={contentRef} data-reader-content>
      <TranslatedText blocks={blocks} sourceIndices={sourceIndices} />
    </div>
  );
});

export interface EpubReaderProps {
  bookId: string;
  /** Catalogue title; the EPUB's own metadata takes over once loaded. */
  bookTitle: string;
  /** Catalogue author, used until the EPUB's `dc:creator` is known. */
  authorName?: string;
}

/**
 * The EPUB reader.
 *
 * Full-screen and self-contained: the app chrome is gone and typography is the
 * main control surface. Sections are fetched one at a time, so opening a
 * 300-document book costs one request rather than one per section. Reading is
 * paged rather than scrolled: the text is laid out in columns exactly one page
 * wide and turned a page at a time, which gives a long chapter the shape of a
 * printed book instead of one endless column.
 *
 * Sanitised markup arrives from the server, so the chapter HTML is injected
 * directly. See `src/server/services/epub/sanitize.ts` for the allowlist.
 */
export function EpubReader({ bookId, bookTitle, authorName = '' }: EpubReaderProps) {
  const { status, error: statusError } = useEpubStatus(bookId);

  const [payload, setPayload] = React.useState<EpubChapterPayload | null>(null);
  const [chapterIndex, setChapterIndex] = React.useState(0);
  const [loading, setLoading] = React.useState(false);
  const [loadError, setLoadError] = React.useState<string | null>(null);
  const [contentsOpen, setContentsOpen] = React.useState(false);
  const [settingsOpen, setSettingsOpen] = React.useState(false);
  const [scrollPercent, setScrollPercent] = React.useState(0);
  /** Which spread of the section is on screen, and how many there are. */
  const [pageIndex, setPageIndex] = React.useState(0);
  const [pageCount, setPageCount] = React.useState(1);
  /** Printed pages (columns) in the section, for "Pages 5–6 of 12". */
  const [printedPages, setPrintedPages] = React.useState(1);
  /** The measured spread, in CSS pixels. */
  const [pageBox, setPageBox] = React.useState<PageBox | null>(null);
  /** Section whose page count has been measured; jumps wait for it. */
  const [measuredChapter, setMeasuredChapter] = React.useState<number | null>(null);
  /** Slides the article only after the reader has turned a page themselves. */
  const [pageMotion, setPageMotion] = React.useState(false);
  const [progressReady, setProgressReady] = React.useState(false);
  const [audioMap, setAudioMap] = React.useState<Map<number, EpubAudioEntry>>(new Map());
  const [uploadFor, setUploadFor] = React.useState<number | null>(null);
  const [preparing, setPreparing] = React.useState<number | null>(null);
  const [preferences, setPreferences] = React.useState<ReaderPreferences>(DEFAULT_PREFERENCES);
  /** Key of the contents row the reader is currently in. */
  const [activeKey, setActiveKey] = React.useState<string | null>(null);

  /* ----- Study: search, highlights, notes, bookmarks and words ----- */
  const [studyOpen, setStudyOpen] = React.useState(false);
  const [studyTab, setStudyTab] = React.useState<StudyTab>('search');
  const [selectionDraft, setSelectionDraft] = React.useState<SelectionDraft | null>(null);
  const [noteComposer, setNoteComposer] = React.useState<NoteComposer | null>(null);
  const [activeAnnotationId, setActiveAnnotationId] = React.useState<string | null>(null);
  const [annotationAnchor, setAnnotationAnchor] = React.useState<{
    left: number;
    top: number;
    width: number;
    height: number;
  } | null>(null);
  const [dictionaryRequest, setDictionaryRequest] = React.useState<DictionaryRequest | null>(null);
  /** The search match that was jumped to, so it stands out from the rest. */
  const [activeSearchHit, setActiveSearchHit] = React.useState<PendingSearchHit | null>(null);

  const { speak } = useSpeak();

  const stageRef = React.useRef<HTMLDivElement>(null);
  /** The one-page window the columns are clipped to; its size defines a page. */
  const leafRef = React.useRef<HTMLDivElement>(null);
  const articleRef = React.useRef<HTMLElement>(null);
  const contentRef = React.useRef<HTMLDivElement>(null);
  const headerRef = React.useRef<HTMLDivElement>(null);
  const footerRef = React.useRef<HTMLElement>(null);
  /**
   * Chrome heights, measured rather than guessed.
   *
   * The bars float over the text so hiding them leaves no gap, which means the
   * stage is padded by exactly their height at the top and bottom — the page
   * then keeps its size whether the bars are showing or not.
   */
  const [chromeHeight, setChromeHeight] = React.useState(0);
  const [footerHeight, setFooterHeight] = React.useState(0);
  /** Block index of the loaded section. A cache of the live DOM — see `ensureBlocks`. */
  const blocksRef = React.useRef<TextBlock[]>([]);
  const restoredRef = React.useRef(false);
  /**
   * True once the saved position has been read (or has failed to load).
   *
   * The ref gates saving; the state re-runs the page measurement, because the
   * saved page may only arrive after the section has been laid out.
   */
  const progressReadyRef = React.useRef(false);
  /** Where to restore the scroll to, read once when progress loads. */
  const restorePercentRef = React.useRef(0);
  const restoreChapterRef = React.useRef(0);
  /**
   * Contents row a click asked for, waiting for its section to arrive.
   *
   * Navigation addresses a place in the text, not a file, so choosing a row that
   * lives in another section is two steps: open the section, then land on the
   * page holding the offset. The row is kept here because the offset only means
   * something once that section has been laid out.
   */
  const pendingRowRef = React.useRef<ContentsRow | null>(null);
  /** Last reported active row, so paging only re-renders on a real change. */
  const activeKeyRef = React.useRef<string | null>(null);
  /**
   * Where a jump put the reader, while they are still sitting there.
   *
   * Not every contents offset points exactly at its heading. When a book's table
   * of contents names spots inside one file with no anchor to aim at, the offset
   * is an estimate proportional to the text — good to a paragraph, sometimes a
   * couple of pages out. Deriving the highlight from the page would then report
   * the row *before* the one that was chosen, which reads as the reader having
   * clicked the wrong thing.
   *
   * So an explicit choice holds the highlight until the reader turns away from
   * where it put them. After that the page is the honest answer again.
   */
  const settledPageRef = React.useRef<number | null>(null);
  /** A search result waiting for its section (and language) to be on screen. */
  const pendingSearchRef = React.useRef<EpubSearchResult | null>(null);
  /** A highlight picked from the panel or the margin, waiting for its section. */
  const pendingAnnotationRef = React.useRef<EpubAnnotation | null>(null);
  /** A saved place picked from the panel, waiting for its section. */
  const pendingBookmarkRef = React.useRef<EpubBookmarkInfo | null>(null);

  /** The measured page geometry, read by handlers that must not re-subscribe. */
  const layoutRef = React.useRef<PageLayout | null>(null);
  const pageCountRef = React.useRef(1);
  const pageIndexRef = React.useRef(0);
  /** Clears the turn transition, so a later reflow lands without a slide. */
  const motionTimerRef = React.useRef<number | undefined>(undefined);

  const hasEpub = status?.hasEpub === true;
  const docked = useDockedContents();

  const layout = pageBox ? pageLayout(pageBox.width, pageBox.height) : null;
  layoutRef.current = layout;
  const pagesReady = measuredChapter === chapterIndex;

  const chapters: EpubChapterEntry[] = React.useMemo(() => payload?.chapters ?? [], [payload]);
  /**
   * The book's own table of contents, as rows.
   *
   * Deliberately not the section list: a contents row is a place in the text, so
   * a book whose contents name two spots in one file — or one file that was split
   * into three — still navigates the way its publisher intended.
   */
  const rows: ContentsRow[] = React.useMemo(
    () => buildContentsRows(payload?.contents, chapters),
    [chapters, payload],
  );
  /**
   * Whether the text on screen is the section that was asked for.
   *
   * The fetch for the next section resolves after the switch, so for a moment the
   * component holds a new `chapterIndex` and the *previous* section's payload.
   * Rendering that payload is a real bug twice over: the reader sees a page they
   * did not ask for, and any jump addressed to the new section lands in the old
   * text, against offsets that mean nothing there.
   */
  const ready = payload !== null && payload.chapter.index === chapterIndex;
  const chapterHtml = ready ? payload.chapter.html : '';

  /**
   * Reading a section in another language.
   *
   * The hook owns which language the book is being read in and fetches that
   * section's translation; everything below has to know whether the text on screen
   * is the original, because every offset the contents carries is an offset into
   * the *original* text. Translating replaces the markup with different words of a
   * different length, so an index built over it would report positions the
   * contents knows nothing about — a contents jump would land at the end of the
   * section instead of on its heading, and the active-row highlight would walk
   * through the rows in proportion to how long each translation happened to be.
   *
   * A ref rather than the state itself, because these run inside scroll handlers
   * and jump effects that must not be re-subscribed whenever a translation loads.
   */
  const translations = useTranslations(bookId, chapterIndex, hasEpub);
  const translating = translations.showing;
  const translatingRef = React.useRef(false);
  translatingRef.current = translating;

  /**
   * Everything the reader marks or looks up, loaded alongside the book.
   *
   * The annotations are stored against the *source* text, so they survive a
   * translation being switched on and off; the highlighter projects them onto
   * whatever language is on screen.
   */
  const annotations = useAnnotations(bookId, hasEpub);
  const bookmarks = useBookmarks(bookId, hasEpub);
  const savedWords = useSavedWords(hasEpub);
  const search = useReaderSearch(bookId, translations.language, hasEpub);

  const sourceBlocks = React.useMemo<EpubChapterBlock[]>(
    () => (payload?.chapter.blocks ?? EMPTY_BLOCKS),
    [payload],
  );

  /**
   * Source block index of each translated block.
   *
   * The server labels every block it serves with the source block it stands
   * for, images included, so an annotation lands on the same paragraph in every
   * language. Older responses without those labels fall back to the section's
   * text blocks in order, which is what a translation used to contain.
   */
  const translationSourceIndices = React.useMemo(() => {
    const blocks = translations.entry?.blocks ?? [];
    if (blocks.every((block) => typeof block.sourceIndex === 'number')) {
      return blocks.map((block) => block.sourceIndex!);
    }
    const indices: number[] = [];
    for (let index = 0; index < sourceBlocks.length; index += 1) {
      if (sourceBlocks[index]?.tag !== 'img') indices.push(index);
    }
    return indices;
  }, [sourceBlocks, translations.entry]);

  // The highlighter's layout effect runs whenever this changes, so it has to be
  // the same object between renders rather than rebuilt from the hit each time.
  const paintedSearchHit = React.useMemo(
    () =>
      activeSearchHit && activeSearchHit.chapterIndex === chapterIndex
        ? {
            blockIndex: activeSearchHit.blockIndex,
            startOffset: activeSearchHit.startOffset,
            endOffset: activeSearchHit.endOffset,
            source: activeSearchHit.source,
          }
        : null,
    [activeSearchHit, chapterIndex],
  );

  const highlightState = useReaderHighlights({
    contentRef,
    scrollRef: stageRef,
    chapterIndex,
    sourceBlocks,
    translationBlocks: translations.entry?.blocks ?? null,
    showingTranslation: translating,
    annotations: annotations.annotations,
    activeId: activeAnnotationId,
    searchQuery: search.query,
    searchOptions: search.options,
    activeSearch: paintedSearchHit,
    layoutKey: `${preferences.fontFamily}:${pageBox?.width ?? 0}:${pageBox?.height ?? 0}`,
    enabled: hasEpub && ready,
  });

  // Event handlers read the painted ranges without re-subscribing on every paint.
  const rangesRef = React.useRef(highlightState.ranges);
  rangesRef.current = highlightState.ranges;

  const activeAnnotation =
    annotations.annotations.find((annotation) => annotation.id === activeAnnotationId) ?? null;

  /** Base language of the book, for dictionary and speech calls. */
  const bookLanguage = React.useMemo(() => {
    const raw = (payload?.meta.language || status?.language || 'en').toLowerCase();
    return raw.split(/[-_]/)[0] || 'en';
  }, [payload?.meta.language, status?.language]);

  const openStudy = React.useCallback(
    (tab: StudyTab) => {
      setStudyTab(tab);
      setStudyOpen(true);
      if (!docked) setContentsOpen(false);
    },
    [docked],
  );

  /**
   * The bookmark for where the reader is now, if they are at a saved place.
   *
   * A bookmark is a section plus how far down it, so "here" means within a
   * couple of percent — the same page, not merely the same chapter.
   */
  const currentBookmark = React.useMemo(
    () =>
      bookmarks.bookmarks.find(
        (bookmark) =>
          bookmark.chapterIndex === chapterIndex &&
          Math.abs(bookmark.scrollPercent - scrollPercent) <= 0.015,
      ) ?? null,
    [bookmarks.bookmarks, chapterIndex, scrollPercent],
  );


  /**
   * The block index for the text that is on screen, rebuilt if it has gone stale.
   *
   * The index is a cache of the live DOM: each entry holds an element so its
   * position can be measured. Element references die quietly. React rewrites the
   * article's inner HTML whenever a section's markup changes, which replaces
   * every child element while leaving the article itself alone — so an index
   * built a moment earlier ends up pointing at nodes that have left the document,
   * and measuring one of those reports a zero rect. A zero rect reads as "the
   * target is already at the top", which turns every contents jump into a silent
   * no-op: the row is marked active, the highlight moves, the text does not.
   *
   * So the cache is checked against the DOM before it is trusted. Rebuilding is a
   * walk of a few hundred nodes, and it happens only when the markup has actually
   * been replaced.
   */
  const ensureBlocks = React.useCallback((): TextBlock[] => {
    const article = articleRef.current;
    const blocks = blocksRef.current;
    if (!ready || !article) return blocks;
    if (blocks.length === 0 || !blocks[0]!.el.isConnected) {
      blocksRef.current = buildTextIndex(article);
    }
    return blocksRef.current;
  }, [ready]);

  /**
   * Re-read the index from the DOM, whatever the cached one looks like.
   *
   * Handed to the two lookups in `text-index` so they can recover on their own
   * when the block they picked has left the document — see `blockAtOffset`.
   */
  const reindex = React.useCallback((): TextBlock[] => {
    const article = articleRef.current;
    if (article) blocksRef.current = buildTextIndex(article);
    return blocksRef.current;
  }, []);

  /**
   * Report which contents row the reader is in.
   *
   * Called whenever the page changes — a turn, a jump, a restored position — so
   * the highlight is never left pointing at wherever the reader was before. The
   * answer is the block at the top of this page: the last block that starts at
   * or before it in column order, which handles a paragraph that began on the
   * previous page and runs onto this one.
   *
   * A row that has not changed is not reported at all: the panel re-renders all
   * of its rows with it.
   */
  const syncActiveRow = React.useCallback(() => {
    // Translated text has no place in the contents, so there is no row to be in.
    // The section being read stays marked in the panel instead.
    if (translatingRef.current) return;
    const article = articleRef.current;
    const geometry = layoutRef.current;
    if (!article || !geometry) return;
    const page = pageIndexRef.current;
    // While a jump is still where it left the reader, the row they chose is the
    // row in view — see `settledPageRef`.
    const settled = settledPageRef.current;
    if (settled !== null && settled === page) return;
    settledPageRef.current = null;

    const block = blockAtPage(article, geometry.width, ensureBlocks(), page);
    const row = activeRow(rows, chapterIndex, block?.start ?? 0);
    const key = row?.key ?? null;
    if (key === activeKeyRef.current) return;
    activeKeyRef.current = key;
    setActiveKey(key);
  }, [chapterIndex, ensureBlocks, rows]);

  // Pinned open beside the text. This is the panel's whole point: it is where
  // you see how far through the book you are, so closing it the moment you start
  // reading takes that away for the entire session.
  const pinnedRef = React.useRef(false);
  React.useEffect(() => {
    if (!docked || pinnedRef.current) return;
    pinnedRef.current = true;
    setContentsOpen(true);
  }, [docked]);

  /* ---------------------------------------------------------------------- */
  /* Progress and preferences                                               */
  /* ---------------------------------------------------------------------- */

  // Preferences are restored before the first chapter paints so typography
  // never flashes the defaults.
  React.useEffect(() => {
    let cancelled = false;

    void (async () => {
      try {
        const saved = await api.get<EpubProgressInfo>(`/api/epub/${bookId}/progress`);
        if (cancelled) return;

        setPreferences({
          theme: saved.theme ?? DEFAULT_PREFERENCES.theme,
          fontFamily: saved.fontFamily ?? DEFAULT_PREFERENCES.fontFamily,
        });
        restorePercentRef.current = clamp(saved.scrollPercent ?? 0, 0, 1);
        restoreChapterRef.current = saved.chapterIndex;
        if (saved.chapterIndex > 0) setChapterIndex(saved.chapterIndex);
      } catch {
        /* defaults are a fine fallback */
      } finally {
        // Unblocks saving. Anything dropped before this point was the reader's
        // own reset; the next page turn writes the true position.
        progressReadyRef.current = true;
        setProgressReady(true);
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [bookId]);

  const savePosition = React.useMemo(
    () =>
      debounce((index: number, percent: number) => {
        // Writing before the stored position has been read would replace it
        // with the pre-restore one.
        if (!progressReadyRef.current) return;
        void api
          .patch(`/api/epub/${bookId}/progress`, {
            chapterIndex: index,
            scrollPercent: clamp(percent, 0, 1),
          })
          .catch(() => {
            /* the next scroll event retries */
          });
      }, SAVE_DEBOUNCE_MS),
    [bookId],
  );

  // Flush on the way out. Unmount order is not guaranteed on navigation, and a
  // reader that is being closed is usually mid-page.
  React.useEffect(() => {
    const flush = () => savePosition.flush();
    window.addEventListener('pagehide', flush);
    document.addEventListener('visibilitychange', flush);
    return () => {
      window.removeEventListener('pagehide', flush);
      document.removeEventListener('visibilitychange', flush);
      savePosition.flush();
    };
  }, [savePosition]);

  // Appearance is saved separately from position, debounced so clicking
  // through themes does not fire a request per click.
  const preferencesRef = React.useRef(preferences);
  preferencesRef.current = preferences;

  React.useEffect(() => {
    if (!hasEpub) return;
    const timer = setTimeout(() => {
      const current = preferencesRef.current;
      void api
        .patch(`/api/epub/${bookId}/progress`, {
          theme: current.theme,
          fontFamily: current.fontFamily,
        })
        .catch(() => {
          /* harmless: these are re-sent on the next change */
        });
    }, 500);
    return () => clearTimeout(timer);
  }, [bookId, hasEpub, preferences]);

  /* ---------------------------------------------------------------------- */
  /* Pagination                                                             */
  /* ---------------------------------------------------------------------- */

  /**
   * Move to a page and record it as the reading position.
   *
   * `animate` is false for positions the reader did not choose — a restored
   * place, a reflow that clamped the page — so those arrive without a slide
   * across half the section. A turn also clears the "settled" row, because the
   * reader has now moved away from wherever a contents jump put them.
   */
  const applyPage = React.useCallback(
    (next: number, animate: boolean) => {
      const count = Math.max(1, pageCountRef.current);
      const target = clamp(Math.round(next), 0, count - 1);
      if (target === pageIndexRef.current) return;
      pageIndexRef.current = target;
      setPageIndex(target);
      if (animate) {
        setPageMotion(true);
        window.clearTimeout(motionTimerRef.current);
        motionTimerRef.current = window.setTimeout(() => setPageMotion(false), 280);
      }
      const percent = count > 1 ? target / (count - 1) : 0;
      setScrollPercent(percent);
      savePosition(chapterIndex, percent);
      syncActiveRow();
    },
    [chapterIndex, savePosition, syncActiveRow],
  );

  /**
   * Count the pages in the section on screen and keep the position valid.
   *
   * Runs after every reflow — a new section, an appearance change, a window
   * resize, an image arriving — and measures the laid-out DOM rather than
   * estimating from word count. A turn advances one spread, which is two
   * printed pages wherever the screen shows two columns. The saved position is
   * applied here too, because until the first measurement there are no pages to
   * restore into.
   */
  const measurePages = React.useCallback(() => {
    const article = articleRef.current;
    const geometry = layoutRef.current;
    if (!article || !geometry) return;

    const { pages: spreads, printed } = measurePageFill(article, geometry);
    pageCountRef.current = spreads;
    setPageCount((current) => (current === spreads ? current : spreads));
    setPrintedPages((current) => (current === printed ? current : printed));
    setMeasuredChapter(chapterIndex);

    if (
      !restoredRef.current &&
      progressReadyRef.current &&
      chapterIndex === restoreChapterRef.current
    ) {
      restoredRef.current = true;
      const percent = restorePercentRef.current;
      if (percent > 0.005 && spreads > 1) applyPage(Math.round(percent * (spreads - 1)), false);
    }

    // A shorter section can leave the reader past its last page.
    if (pageIndexRef.current > spreads - 1) applyPage(spreads - 1, false);
    syncActiveRow();
  }, [applyPage, chapterIndex, syncActiveRow]);

  /**
   * Measure the spread.
   *
   * Foliate's page is a fixed measure, not the window: the text block tops out
   * at 720 pixels a page and 1440 a spread, with paper around it. So the stage
   * is measured, the frame is taken out of it, and what is left — capped at
   * those limits — becomes the page box. The frame is part of the same
   * measurement so the sheet can be sized around the text exactly.
   *
   * A layout effect, so the state it sets re-renders before the browser paints:
   * the first frame is already paginated instead of showing the article as one
   * tall column that then snaps into pages. The observer covers window resizes
   * and the chrome's measured height settling after its first paint.
   */
  React.useLayoutEffect(() => {
    if (!ready) return;
    const stage = stageRef.current;
    if (!stage) return;

    const measure = () => {
      const rect = stage.getBoundingClientRect();
      const style = window.getComputedStyle(stage);
      const innerWidth = Math.floor(
        rect.width - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight),
      );
      const innerHeight = Math.floor(
        rect.height - parseFloat(style.paddingTop) - parseFloat(style.paddingBottom),
      );
      if (innerWidth < 240 || innerHeight < 240) return;

      // A book's margin, scaled a little by the screen but never thin.
      const frameX = Math.min(44, Math.max(16, Math.round(innerWidth * 0.035)));
      const frameY = Math.min(40, Math.max(16, Math.round(innerHeight * 0.035)));
      const width = Math.min(innerWidth - frameX * 2, MAX_PAGE_INLINE_SIZE * 2);
      const height = Math.min(innerHeight - frameY * 2, MAX_PAGE_BLOCK_SIZE);

      setPageBox((current) =>
        current &&
        current.width === width &&
        current.height === height &&
        current.frameX === frameX &&
        current.frameY === frameY
          ? current
          : { width, height, frameX, frameY },
      );
    };

    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(stage);
    return () => observer.disconnect();
  }, [chapterIndex, ready]);

  /**
   * Lay out and count the section once the page styles are on the article.
   *
   * Images reserve no height until they load and a web font can change the line
   * count, so both re-measure: otherwise the last page would be a little short
   * or the page count would be wrong until the next turn.
   */
  React.useLayoutEffect(() => {
    if (!ready || !pageBox) return;
    const article = articleRef.current;
    if (!article) return;

    let frame = 0;
    const schedule = () => {
      if (frame) return;
      frame = requestAnimationFrame(() => {
        frame = 0;
        measurePages();
      });
    };

    measurePages();

    const onLoad = (event: Event) => {
      if (event.target instanceof HTMLImageElement) schedule();
    };
    article.addEventListener('load', onLoad, true);
    void document.fonts?.ready.then(schedule);

    return () => {
      if (frame) cancelAnimationFrame(frame);
      article.removeEventListener('load', onLoad, true);
    };
  }, [
    chapterHtml,
    chapterIndex,
    measurePages,
    pageBox,
    preferences.fontFamily,
    progressReady,
    ready,
    translating,
    translations.entry,
  ]);

  /* ---------------------------------------------------------------------- */
  /* Audio map                                                              */
  /* ---------------------------------------------------------------------- */

  const refreshAudioMap = React.useCallback(async () => {
    try {
      const result = await api.get<{ entries: EpubAudioEntry[] }>(`/api/epub/${bookId}/chapters/audio`);
      setAudioMap(new Map(result.entries.map((entry) => [entry.index, entry])));
    } catch {
      /* the listen affordances simply stay as they are */
    }
  }, [bookId]);

  React.useEffect(() => {
    if (!hasEpub) return;
    void refreshAudioMap();
  }, [hasEpub, refreshAudioMap]);

  

  /* ---------------------------------------------------------------------- */
  /* Chapter loading                                                        */
  /* ---------------------------------------------------------------------- */

  React.useEffect(() => {
    if (!hasEpub) return;
    let cancelled = false;

    setLoading(true);
    setLoadError(null);

    void (async () => {
      try {
        const next = await api.get<EpubChapterPayload>(`/api/epub/${bookId}/chapter/${chapterIndex}`);
        if (!cancelled) setPayload(next);
      } catch (err) {
        if (!cancelled) setLoadError(errorMessage(err, 'Could not open that section.'));
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [bookId, chapterIndex, hasEpub]);

  // Track the bars' heights so the floating chrome never covers the text.
  React.useLayoutEffect(() => {
    const header = headerRef.current;
    const footer = footerRef.current;
    if (!header || !footer) return;
    const measure = () => {
      setChromeHeight(header.offsetHeight);
      setFooterHeight(footer.offsetHeight);
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(header);
    observer.observe(footer);
    return () => observer.disconnect();
  }, []);

  // A new section opens on its first page; the previous page number is
  // meaningless here.
  React.useEffect(() => {
    pageIndexRef.current = 0;
    pageCountRef.current = 1;
    settledPageRef.current = null;
    setPageIndex(0);
    setPageCount(1);
    setMeasuredChapter(null);
    setScrollPercent(0);
    setPageMotion(false);
  }, [chapterIndex]);

  /**
   * Index the loaded section's blocks.
   *
   * Every contents row's offset, and every "which row am I in" question, is an
   * offset into the visible text — so both directions go through this index.
   */
  React.useEffect(() => {
    const article = articleRef.current;
    // No index while a translation is on screen: its offsets belong to the
    // original text, which is not what is on the page. See `translatingRef`.
    blocksRef.current = ready && !translatingRef.current && article ? buildTextIndex(article) : [];
    syncActiveRow();
  }, [ready, chapterHtml, syncActiveRow, translating]);

  /**
   * Land on the row a click asked for.
   *
   * The page count has to be known first: a jump is expressed in pages, and
   * clamping it against the placeholder count of one would send every row to
   * the first page. Images arriving later can add pages, so an image load while
   * the reader is still exactly where the jump put them repeats it.
   */
  React.useEffect(() => {
    const row = pendingRowRef.current;
    if (!ready || !pagesReady || !row || row.index !== chapterIndex) return;
    // In translated text there is no offset to land on, so the row's section is
    // as close as a jump can honestly get. Clear it either way, or the next
    // section would be sent to this row's offset.
    if (translatingRef.current) {
      pendingRowRef.current = null;
      return;
    }
    pendingRowRef.current = null;

    const apply = () => {
      const article = articleRef.current;
      const geometry = layoutRef.current;
      if (!article || !geometry) return;
      const block = blockAtOffset(ensureBlocks(), row.offset, reindex);
      if (!block) return;
      const page = pageOfElement(article, geometry.width, block.el);
      // Hold the row the reader chose while they are sitting where it put them.
      settledPageRef.current = page;
      activeKeyRef.current = row.key;
      setActiveKey(row.key);
      applyPage(page, true);
    };

    const frame = requestAnimationFrame(apply);
    return () => cancelAnimationFrame(frame);
  }, [applyPage, chapterIndex, ensureBlocks, pagesReady, ready, reindex]);

  /* ---------------------------------------------------------------------- */
  /* Navigation                                                             */
  /* ---------------------------------------------------------------------- */

  const lastIndex = Math.max(0, chapters.length - 1);
  const hasPrev = chapterIndex > 0;
  const hasNext = chapterIndex < lastIndex;
  // Paging continues into the neighbouring section at either end.
  const canTurnBack = pageIndex > 0 || hasPrev;
  const canTurnForward = pageIndex < pageCount - 1 || hasNext;

  const goTo = React.useCallback(
    (index: number) => {
      const next = clamp(Math.round(index), 0, lastIndex);
      if (next === chapterIndex) return;
      savePosition.flush();
      setChapterIndex(next);
    },
    [chapterIndex, lastIndex, savePosition],
  );

  /**
   * Turn within the section, or move to the next one at either end.
   *
   * Reaching past the last page continues the book rather than stopping dead,
   * which is how a paged reader is expected to behave — and going back from the
   * first page reopens the previous section on its last page. A short pause
   * after a chapter change keeps a fast trackpad from skipping a whole one.
   */
  const turn = React.useCallback(
    (delta: number) => {
      // Until the section has been measured there is one page and no boundary
      // to cross; turning anyway would skip past it into the next section.
      if (!pagesReady) return;
      const max = Math.max(0, pageCountRef.current - 1);
      const next = pageIndexRef.current + delta;
      if (next >= 0 && next <= max) {
        settledPageRef.current = null;
        applyPage(next, true);
        return;
      }
      // A floating card or toolbar is placed in viewport coordinates, so it
      // would stay behind while its words move away.
      setSelectionDraft(null);
      setActiveAnnotationId(null);
      if (delta > 0 && hasNext) goTo(chapterIndex + 1);
      else if (delta < 0 && hasPrev) goTo(chapterIndex - 1);
    },
    [applyPage, chapterIndex, goTo, hasNext, hasPrev, pagesReady],
  );

  // Read by the wheel and touch handlers, which subscribe once.
  const turnRef = React.useRef(turn);
  turnRef.current = turn;

  // A wheel or trackpad gesture is a page turn, not a scroll: the section has
  // nothing to scroll. Deltas are accumulated because a trackpad sends dozens
  // of small events, and held off after a turn for the same reason.
  React.useEffect(() => {
    const node = stageRef.current;
    if (!node) return;

    let accumulated = 0;
    let last = 0;
    let locked = 0;
    const onWheel = (event: WheelEvent) => {
      if (event.ctrlKey || event.metaKey) return;
      event.preventDefault();
      const now = performance.now();
      if (now < locked) return;
      const delta = Math.abs(event.deltaX) > Math.abs(event.deltaY) ? event.deltaX : event.deltaY;
      if (!delta) return;
      if (now - last > 260) accumulated = 0;
      last = now;
      accumulated += delta;
      if (Math.abs(accumulated) < 32) return;
      const direction = accumulated > 0 ? 1 : -1;
      accumulated = 0;
      locked = now + 320;
      turnRef.current(direction);
    };

    node.addEventListener('wheel', onWheel, { passive: false });
    return () => node.removeEventListener('wheel', onWheel);
  }, []);

  /**
   * A horizontal swipe turns the page.
   *
   * Deliberately ignored while text is selected: on a touch screen the same
   * gesture is how a selection is extended, and turning the page out from under
   * a highlight the reader was making is the worse mistake.
   */
  React.useEffect(() => {
    const node = stageRef.current;
    if (!node) return;

    let startX = 0;
    let startY = 0;
    let started = 0;
    const onStart = (event: TouchEvent) => {
      if (event.touches.length !== 1) return;
      const touch = event.touches[0]!;
      startX = touch.clientX;
      startY = touch.clientY;
      started = performance.now();
    };
    const onEnd = (event: TouchEvent) => {
      const touch = event.changedTouches[0];
      if (!touch) return;
      const dx = touch.clientX - startX;
      const dy = touch.clientY - startY;
      if (performance.now() - started > 700) return;
      if (Math.abs(dx) < 48 || Math.abs(dx) < Math.abs(dy) * 1.4) return;
      const selection = window.getSelection();
      if (selection && !selection.isCollapsed) return;
      turnRef.current(dx < 0 ? 1 : -1);
    };

    node.addEventListener('touchstart', onStart, { passive: true });
    node.addEventListener('touchend', onEnd, { passive: true });
    return () => {
      node.removeEventListener('touchstart', onStart);
      node.removeEventListener('touchend', onEnd);
    };
  }, []);

  /**
   * The page-relative position of a saved place.
   *
   * Saved positions are a fraction of the section, so they survive a change of
   * font or window size that changes the page count.
   */
  const pageForPercent = React.useCallback((percent: number): number => {
    const count = Math.max(1, pageCountRef.current);
    return Math.round(clamp(percent, 0, 1) * (count - 1));
  }, []);

  /**
   * Open the section a contents row points at and land on it.
   *
   * The panel stays put when it is docked — that is the difference between a
   * side panel and a menu — and closes only when it is covering the text, where
   * leaving it open would hide the thing that was just chosen. A row in the
   * section already open still counts as having been chosen from the drawer, so
   * it closes the drawer too: the reader asked to be shown something, and the
   * drawer is what was standing in the way of seeing it.
   */
  const openRow = React.useCallback(
    (row: ContentsRow) => {
      const article = articleRef.current;
      const geometry = layoutRef.current;
      if (row.index === chapterIndex && article && geometry && pagesReady) {
        // In translated text an offset is a place in words that are not on the
        // page; the section highlight already says where the reader is.
        if (!translatingRef.current) {
          const block = blockAtOffset(ensureBlocks(), row.offset, reindex);
          if (block) {
            const page = pageOfElement(article, geometry.width, block.el);
            // Hold this row until the reader turns away — see `settledPageRef`.
            settledPageRef.current = page;
            activeKeyRef.current = row.key;
            setActiveKey(row.key);
            applyPage(page, true);
          }
        }
        if (!docked) setContentsOpen(false);
        return;
      }

      pendingRowRef.current = row;
      goTo(row.index);
      if (!docked) setContentsOpen(false);
    },
    [applyPage, chapterIndex, docked, ensureBlocks, goTo, pagesReady, reindex],
  );

  /* ---------------------------------------------------------------------- */
  /* Study: selection, highlights, notes, search, places and words          */
  /* ---------------------------------------------------------------------- */

  /** The blocks of the text currently on screen, read straight from the DOM. */
  const visibleReaderBlocks = React.useCallback((): ReaderBlock[] => {
    const root = contentRef.current;
    if (!root) return [];
    if (translations.showing && translations.entry) return buildTranslatedBlocks(root);
    return buildReaderBlocks(root, sourceBlocks);
  }, [sourceBlocks, translations.entry, translations.showing]);

  const clearBrowserSelection = React.useCallback(() => {
    window.getSelection()?.removeAllRanges();
  }, []);

  /**
   * Show the page a range is on.
   *
   * A page, not a scroll position: the point of pagination is that a passage
   * has exactly one place it can appear, and turning to that page is what "show
   * me this" means. The turn is animated, because it is the reader's own motion
   * — a jump that teleports leaves them unsure whether anything happened.
   */
  const revealRange = React.useCallback(
    (range: Range | null, fallback: HTMLElement) => {
      const article = articleRef.current;
      const geometry = layoutRef.current;
      if (!article || !geometry) return;
      const page = range
        ? pageOfRange(article, geometry.width, range)
        : pageOfElement(article, geometry.width, fallback);
      applyPage(page, true);
    },
    [applyPage],
  );

  const landOnPercent = React.useCallback(
    (percent: number) => {
      applyPage(pageForPercent(percent), true);
    },
    [applyPage, pageForPercent],
  );

  /** Land on a result, using the offsets the search reported for that view. */
  const landOnSearchHit = React.useCallback(
    (hit: EpubSearchResult) => {
      const root = contentRef.current;
      if (!root) return;
      const blocks =
        hit.source === 'translation'
          ? buildTranslatedBlocks(root)
          : buildReaderBlocks(root, sourceBlocks);
      // A translation result is indexed by its own paragraph order; the
      // rendered translation is labelled with source block indices.
      const blockIndex =
        hit.source === 'translation'
          ? (translationSourceIndices[hit.blockIndex] ?? hit.blockIndex)
          : hit.blockIndex;
      const block = blocks.find((item) => item.serverIndex === blockIndex);
      if (!block) return;
      const range = rangeForBlockOffsets(block, hit.startOffset, hit.endOffset);
      revealRange(range, block.el);
      setActiveSearchHit({
        chapterIndex: hit.chapterIndex,
        blockIndex,
        startOffset: hit.startOffset,
        endOffset: hit.endOffset,
        source: hit.source,
      });
      setActiveAnnotationId(null);
    },
    [revealRange, sourceBlocks, translationSourceIndices],
  );

  /** Land on a stored highlight, projecting its source anchor if needed. */
  const landOnAnnotation = React.useCallback(
    (annotation: EpubAnnotation) => {
      const root = contentRef.current;
      if (!root) return;
      const translated = translations.showing && translations.entry !== null;
      const blocks = translated
        ? buildTranslatedBlocks(root)
        : buildReaderBlocks(root, sourceBlocks);
      const block = blocks.find((item) => item.serverIndex === annotation.startBlock);
      if (!block) return;

      let start = annotation.startOffset;
      let end = annotation.startBlock === annotation.endBlock ? annotation.endOffset : block.text.length;
      if (translated) {
        const sourceText = sourceBlocks[annotation.startBlock]?.text ?? '';
        const endSource =
          annotation.startBlock === annotation.endBlock ? annotation.endOffset : sourceText.length;
        const projected = projectOffsets(
          sourceText.length,
          block.text.length,
          annotation.startOffset,
          endSource,
        );
        ({ start, end } = snapToWords(block.text, projected.start, projected.end));
      }

      const range =
        rangeForBlockOffsets(block, start, end) ?? rangeForBlockOffsets(block, 0, block.text.length);
      revealRange(range, block.el);
      setActiveAnnotationId(annotation.id);
      setSelectionDraft(null);
      if (range) {
        // The card is anchored in viewport coordinates, which move with the
        // page as it slides. Placing it after the turn keeps it on the words.
        window.setTimeout(() => setAnnotationAnchor(rectForRange(range)), 260);
      } else {
        setAnnotationAnchor(null);
      }
    },
    [revealRange, sourceBlocks, translations.entry, translations.showing],
  );

  const openAnnotation = React.useCallback(
    (annotation: EpubAnnotation) => {
      if (annotation.chapterIndex !== chapterIndex) {
        pendingAnnotationRef.current = annotation;
        setActiveSearchHit(null);
        goTo(annotation.chapterIndex);
        return;
      }
      if (!pagesReady) {
        // The page count is still being measured; the pending effect lands it.
        pendingAnnotationRef.current = annotation;
        return;
      }
      landOnAnnotation(annotation);
    },
    [chapterIndex, goTo, landOnAnnotation, pagesReady],
  );

  const openSearchResult = React.useCallback(
    (result: EpubSearchResult) => {
      setActiveAnnotationId(null);
      if (result.source === 'translation' && result.language) {
        if (!translations.showing || translations.language !== result.language) {
          translations.showTranslation(result.language);
        }
      } else if (translations.showing) {
        translations.showOriginal();
      }
      setActiveSearchHit({
        chapterIndex: result.chapterIndex,
        blockIndex: result.blockIndex,
        startOffset: result.startOffset,
        endOffset: result.endOffset,
        source: result.source,
      });

      const viewMatchesNow =
        result.source === 'translation'
          ? translations.showing && translations.entry?.language === result.language
          : !translations.showing;

      if (result.chapterIndex === chapterIndex && viewMatchesNow && pagesReady) {
        landOnSearchHit(result);
        return;
      }
      pendingSearchRef.current = result;
      if (result.chapterIndex !== chapterIndex) goTo(result.chapterIndex);
    },
    [
      chapterIndex,
      goTo,
      landOnSearchHit,
      pagesReady,
      translations,
    ],
  );

  const openBookmark = React.useCallback(
    (bookmark: EpubBookmarkInfo) => {
      if (bookmark.chapterIndex !== chapterIndex || !pagesReady) {
        pendingBookmarkRef.current = bookmark;
        if (bookmark.chapterIndex !== chapterIndex) goTo(bookmark.chapterIndex);
        return;
      }
      landOnPercent(bookmark.scrollPercent);
    },
    [chapterIndex, goTo, landOnPercent, pagesReady],
  );

  // A jump chosen from the panel waits for its section, its language, its page
  // count, or all three.
  React.useEffect(() => {
    const pending = pendingAnnotationRef.current;
    if (!pending || !ready || !pagesReady || pending.chapterIndex !== chapterIndex) return;
    pendingAnnotationRef.current = null;
    const frame = requestAnimationFrame(() => landOnAnnotation(pending));
    return () => cancelAnimationFrame(frame);
  }, [chapterIndex, landOnAnnotation, pagesReady, ready]);

  React.useEffect(() => {
    const pending = pendingSearchRef.current;
    if (!pending || !ready || !pagesReady || pending.chapterIndex !== chapterIndex) return;
    const matchesView =
      pending.source === 'translation'
        ? translations.showing && translations.entry?.language === pending.language
        : !translations.showing;
    if (!matchesView) return;
    pendingSearchRef.current = null;
    const frame = requestAnimationFrame(() => landOnSearchHit(pending));
    return () => cancelAnimationFrame(frame);
  }, [
    chapterIndex,
    landOnSearchHit,
    pagesReady,
    ready,
    translations.entry,
    translations.showing,
  ]);

  React.useEffect(() => {
    const pending = pendingBookmarkRef.current;
    if (!pending || !ready || !pagesReady || pending.chapterIndex !== chapterIndex) return;
    pendingBookmarkRef.current = null;
    const frame = requestAnimationFrame(() => landOnPercent(pending.scrollPercent));
    return () => cancelAnimationFrame(frame);
  }, [chapterIndex, landOnPercent, pagesReady, ready]);

  // A card is placed in viewport coordinates; a change of language reflows the
  // page under it, so it closes rather than pointing at the wrong words.
  React.useEffect(() => {
    setActiveAnnotationId(null);
    setAnnotationAnchor(null);
  }, [translations.showing]);

  /**
   * Read a selection into an anchor as soon as the pointer comes up.
   *
   * Reading it then rather than on `selectionchange` means the browser has
   * finished the drag; it also means a click elsewhere collapses the selection
   * without ever opening the toolbar. The anchor is captured in source
   * coordinates so a highlight made while reading a translation survives the
   * switch back — see `reader-index`.
   */
  React.useEffect(() => {
    const node = stageRef.current;
    if (!node) return;

    const read = (target: EventTarget | null) => {
      if (target instanceof HTMLElement && target.closest('[role="toolbar"], [role="dialog"]')) return;
      const root = contentRef.current;
      const selection = window.getSelection();
      if (!root || !selection || selection.isCollapsed || selection.rangeCount === 0) return;
      if (!selection.anchorNode || !selection.focusNode) return;
      if (!root.contains(selection.anchorNode) || !root.contains(selection.focusNode)) return;

      const blocks = visibleReaderBlocks();
      const anchor = readSelection(blocks, selection);
      if (!anchor) return;

      const translated = translations.showing && translations.entry !== null;
      const source = translated ? toSourceAnchor(anchor, sourceBlocks, blocks) : anchor;

      setSelectionDraft({
        text: anchor.text,
        quote: source.quote || anchor.text,
        start: source.start,
        end: source.end,
        rect: anchor.rect,
        language: translated ? translations.language : null,
        translated,
      });
      setActiveAnnotationId(null);
    };

    const onPointerUp = (event: Event) => {
      window.setTimeout(() => read(event.target), 0);
    };

    node.addEventListener('pointerup', onPointerUp);
    node.addEventListener('touchend', onPointerUp);
    return () => {
      node.removeEventListener('pointerup', onPointerUp);
      node.removeEventListener('touchend', onPointerUp);
    };
  }, [
    sourceBlocks,
    translations.entry,
    translations.language,
    translations.showing,
    visibleReaderBlocks,
  ]);

  // Clicking words a highlight covers opens its card; clicking elsewhere is
  // left alone, so selecting text never pops a card up underneath the toolbar.
  React.useEffect(() => {
    const node = stageRef.current;
    if (!node) return;

    const onClick = (event: MouseEvent) => {
      const target = event.target;
      if (
        target instanceof HTMLElement &&
        target.closest('[role="toolbar"], [role="dialog"], button, a, input, textarea')
      ) {
        return;
      }
      const selection = window.getSelection();
      if (selection && !selection.isCollapsed) return;

      const caret = caretFromPoint(event.clientX, event.clientY);
      if (!caret) return;

      for (const [id, ranges] of rangesRef.current) {
        for (const range of ranges) {
          try {
            if (range.isPointInRange(caret.node, caret.offset)) {
              setSelectionDraft(null);
              setActiveAnnotationId(id);
              setAnnotationAnchor(rectForRange(range));
              return;
            }
          } catch {
            /* the range belongs to markup that has left the document */
          }
        }
      }
    };

    node.addEventListener('click', onClick);
    return () => node.removeEventListener('click', onClick);
  }, []);

  const highlightSelection = React.useCallback(
    async (color: AnnotationColor) => {
      const draft = selectionDraft;
      if (!draft) return;
      setSelectionDraft(null);
      clearBrowserSelection();
      await annotations.create({
        chapterIndex,
        startBlock: draft.start.block,
        startOffset: draft.start.offset,
        endBlock: draft.end.block,
        endOffset: draft.end.offset,
        quote: draft.quote,
        selectedText: draft.text,
        language: draft.language,
        color,
      });
    },
    [annotations, chapterIndex, clearBrowserSelection, selectionDraft],
  );

  const noteSelection = React.useCallback(() => {
    const draft = selectionDraft;
    if (!draft) return;
    setSelectionDraft(null);
    clearBrowserSelection();
    setNoteComposer({
      mode: 'create',
      quote: draft.quote,
      note: '',
      color: 'yellow',
      selection: draft,
    });
  }, [clearBrowserSelection, selectionDraft]);

  const copySelection = React.useCallback(async () => {
    const draft = selectionDraft;
    if (!draft) return;
    try {
      await navigator.clipboard.writeText(draft.text);
      toast.success('Copied to the clipboard.');
    } catch {
      toast.error('Could not copy that text.');
    }
    setSelectionDraft(null);
    clearBrowserSelection();
  }, [clearBrowserSelection, selectionDraft]);

  const searchInBook = React.useCallback(
    (text: string) => {
      setSelectionDraft(null);
      clearBrowserSelection();
      openStudy('search');
      search.setQuery(text.trim().slice(0, 120));
    },
    [clearBrowserSelection, openStudy, search],
  );

  const defineSelection = React.useCallback(() => {
    const draft = selectionDraft;
    if (!draft) return;
    const word = firstWord(draft.text);
    if (!word) return;
    setSelectionDraft(null);
    clearBrowserSelection();
    setDictionaryRequest({ word, language: (draft.language ?? bookLanguage).toLowerCase() });
  }, [bookLanguage, clearBrowserSelection, selectionDraft]);

  const speakSelection = React.useCallback(() => {
    const draft = selectionDraft;
    if (!draft) return;
    speak(draft.text, draft.language ?? bookLanguage);
    setSelectionDraft(null);
    clearBrowserSelection();
  }, [bookLanguage, clearBrowserSelection, selectionDraft, speak]);

  const defineAnnotation = React.useCallback(() => {
    if (!activeAnnotation) return;
    const word = firstWord(activeAnnotation.selectedText || activeAnnotation.quote);
    if (!word) return;
    setDictionaryRequest({
      word,
      language: (activeAnnotation.language ?? bookLanguage).toLowerCase(),
    });
  }, [activeAnnotation, bookLanguage]);

  const editAnnotationNote = React.useCallback((annotation: EpubAnnotation) => {
    setActiveAnnotationId(null);
    setAnnotationAnchor(null);
    setNoteComposer({
      mode: 'edit',
      id: annotation.id,
      quote: annotation.selectedText || annotation.quote,
      note: annotation.note ?? '',
      color: annotation.color,
    });
  }, []);

  const [noteBusy, setNoteBusy] = React.useState(false);

  const saveNote = React.useCallback(
    async (text: string, color: AnnotationColor) => {
      const composer = noteComposer;
      if (!composer || noteBusy) return;
      setNoteBusy(true);
      try {
        if (composer.mode === 'create' && composer.selection) {
          const draft = composer.selection;
          const created = await annotations.create({
            chapterIndex,
            startBlock: draft.start.block,
            startOffset: draft.start.offset,
            endBlock: draft.end.block,
            endOffset: draft.end.offset,
            quote: draft.quote,
            selectedText: draft.text,
            language: draft.language,
            color,
            note: text || null,
          });
          if (created) setNoteComposer(null);
        } else if (composer.id) {
          await annotations.update(composer.id, { note: text || null, color });
          setNoteComposer(null);
        }
      } finally {
        setNoteBusy(false);
      }
    },
    [annotations, chapterIndex, noteBusy, noteComposer],
  );

  const deleteComposedNote = React.useCallback(async () => {
    const composer = noteComposer;
    if (!composer?.id) return;
    await annotations.remove(composer.id);
    setNoteComposer(null);
  }, [annotations, noteComposer]);

  const toggleBookmark = React.useCallback(() => {
    if (currentBookmark) {
      void bookmarks.remove(currentBookmark.id);
      return;
    }
    void bookmarks.create(
      chapterIndex,
      clamp(scrollPercent, 0, 1),
      currentTitle(chapters, chapterIndex) || null,
    );
  }, [bookmarks, chapterIndex, chapters, currentBookmark, scrollPercent]);

  const lookupWord = React.useCallback((word: string, language: string) => {
    setDictionaryRequest({ word, language });
  }, []);

  // Keyboard shortcuts. Bound at the window so they work without a focused
  // element, but skipped when the user is typing or a dialog owns focus.
  React.useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      const tag = target?.tagName ?? '';
      const typing =
        tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || target?.isContentEditable === true;

      // Search is the one browser shortcut the reader takes over: its own find
      // is per-section, and the point of this one is the whole book.
      if ((event.metaKey || event.ctrlKey) && !event.altKey && event.key.toLowerCase() === 'f') {
        event.preventDefault();
        openStudy('search');
        return;
      }

      if (event.metaKey || event.ctrlKey || event.altKey) return;

      if (event.key === 'Escape') {
        // Radix dialogs handle their own Escape and must keep focus.
        if (document.querySelector('[role="dialog"][data-state="open"]')) return;
        if (activeAnnotationId) {
          setActiveAnnotationId(null);
          setAnnotationAnchor(null);
          return;
        }
        if (selectionDraft) {
          setSelectionDraft(null);
          clearBrowserSelection();
          return;
        }
        if (studyOpen) {
          setStudyOpen(false);
          return;
        }
      }

      if (typing) return;

      switch (event.key) {
        case 'ArrowLeft':
        case 'PageUp':
          event.preventDefault();
          turn(-1);
          break;
        case 'ArrowRight':
        case 'PageDown':
          event.preventDefault();
          turn(1);
          break;
        case ' ':
          // Shift+Space goes back, the way a paged reader's space bar should.
          event.preventDefault();
          turn(event.shiftKey ? -1 : 1);
          break;
        case 't':
        case 'T':
          setContentsOpen((open) => !open);
          if (!docked) setStudyOpen(false);
          break;
        case 's':
        case 'S':
          setSettingsOpen((open) => !open);
          break;
        case '/':
          event.preventDefault();
          openStudy('search');
          break;
        case 'h':
        case 'H':
          openStudy('highlights');
          break;
        case 'b':
        case 'B':
          toggleBookmark();
          break;
        default:
          break;
      }
    };

    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [
    activeAnnotationId,
    clearBrowserSelection,
    docked,
    openStudy,
    selectionDraft,
    studyOpen,
    toggleBookmark,
    turn,
  ]);

  /* ---------------------------------------------------------------------- */
  /* Audio in the reader                                                    */
  /* ---------------------------------------------------------------------- */

  const { playChapter, track, isPlaying } = usePlayer();

  const currentAudio = audioMap.get(chapterIndex);

  // Which section this reader started narrating, by spine index. A generation
  // started elsewhere is not this button's business: claiming another section
  // is "Narrating…" both lies and blocks the one on screen from starting.
  const [narratingSection, setNarratingSection] = React.useState<number | null>(null);
  const { generate } = useTtsGeneration(refreshAudioMap);
  const preparingSection = preparing === chapterIndex;
  const narrating = narratingSection === chapterIndex;

  /**
   * Narrate the section on screen.
   *
   * Chapters are created lazily, so a section may be readable but not yet exist
   * as a chapter — and narration needs a chapter to attach audio to. The
   * `ensure` call materialises it first, which is also what gives the generated
   * file a sensible name instead of a blank one.
   */
  const beginNarration = React.useCallback(
    async (language: string | null) => {
      const entry = chapters.find((item) => item.index === chapterIndex);
      if (!entry) {
        toast.error('That section is no longer part of the book.');
        return;
      }

      setPreparing(chapterIndex);
      try {
        // Cheap when the chapter already exists, and the response is that
        // chapter, so there is no need to look its id up first.
        const result = await api.post<{ id: string }>(
          `/api/epub/${bookId}/chapter/${chapterIndex}/ensure`,
          { title: entry.title },
        );
        setPreparing(null);
        setNarratingSection(chapterIndex);
        await generate(result.id, entry.title || `section ${chapterIndex + 1}`, language);
      } catch (error) {
        toast.error(errorMessage(error, 'Could not prepare that section for narration.'));
      } finally {
        setNarratingSection(null);
        setPreparing(null);
      }
    },
    [bookId, chapterIndex, chapters, generate],
  );

  const narration = useNarrateChapter(bookId);

  /**
   * One button for the section. A section with saved translations asks which
   * language should be spoken; one without is narrated in the original without
   * a question — unless the configured voice cannot read that original, in
   * which case the dialog asks for a compatible voice first.
   */
  const narrateSection = () => {
    const entry = chapters.find((item) => item.index === chapterIndex);
    if (!entry) {
      toast.error('That section is no longer part of the book.');
      return;
    }
    void narration.request(
      {
        epubIndex: chapterIndex,
        title: entry.title || `Section ${chapterIndex + 1}`,
        chapterNumber: chapterIndex + 1,
      },
      beginNarration,
      translations.language,
    );
  };

  const playingCurrentSection =
    currentAudio?.hasAudio === true &&
    currentAudio.chapterId !== null &&
    track?.chapterId === currentAudio.chapterId &&
    isPlaying;

  /**
   * Attach audio to a section.
   *
   * Library chapters are created lazily, so a section the user never turned
   * into a chapter still accepts an upload — the `ensure` call materialises it
   * first, which also gives the dialog a real title instead of a blank one.
   */
  const attachAudio = async (index: number) => {
    const entry = chapters.find((item) => item.index === index);
    if (!entry) {
      // Should not happen — every row comes from this same list — but a button
      // that does nothing when pressed is worse than a word about why.
      toast.error('That section is no longer part of the book.');
      return;
    }

    setPreparing(index);
    try {
      await api.post(`/api/epub/${bookId}/chapter/${index}/ensure`, { title: entry.title });
      await refreshAudioMap();
      setUploadFor(index);
    } catch (err) {
      toast.error(errorMessage(err, 'Could not prepare that section for audio.'));
    } finally {
      setPreparing(null);
    }
  };

  /**
   * The section's listen button: play what is there, or offer to add it.
   *
   * The question is whether the section has *audio*, not whether it has a
   * library chapter. A section can have a chapter and no audio — chapters are
   * created for their own sake, and importing audio materialises one on demand —
   * and asking for playback on the strength of an empty chapter left a button
   * labelled "import" doing nothing but failing.
   */
  const activateSection = (index: number) => {
    const entry = audioMap.get(index);
    if (!entry?.hasAudio || !entry.chapterId) {
      void attachAudio(index);
      return;
    }
    void playChapter(entry.chapterId, bookId).catch((err: unknown) => {
      toast.error(errorMessage(err, 'Could not play that section.'));
    });
  };

  /**
   * In translated text the contents highlight marks the section, not a place in
   * it: a jump to an offset is meaningless once the words on the page are not the
   * words the offsets were counted in.
   */
  React.useEffect(() => {
    if (!translating) return;
    const key = rows.find((row) => row.index === chapterIndex)?.key ?? null;
    activeKeyRef.current = key;
    setActiveKey(key);
  }, [translating, rows, chapterIndex]);

  /* ---------------------------------------------------------------------- */
  /* Render                                                                 */
  /* ---------------------------------------------------------------------- */

  const displayTitle = status?.title || payload?.meta.title || bookTitle;
  const displayAuthor = status?.creator || payload?.meta.creator || authorName;
  const progress = overallProgress(chapters, chapterIndex, scrollPercent);
  /**
   * What the reader calls this moment of the book.
   *
   * The contents row, because that is what a contents is for — "Chapter 4" is
   * more use in the header than the section's own first heading, which is
   * whatever front matter got folded in front of it.
   */
  const sectionTitle =
    rows.find((row) => row.key === activeKey)?.title ?? currentTitle(chapters, chapterIndex);

  /**
   * The pages in view, numbered the way a printed book numbers them.
   *
   * A turn shows a spread, so the screen usually holds two printed pages; on a
   * phone it holds one. The last spread of an uneven section holds one.
   */
  const columns = layout?.columns ?? 1;
  const firstPrinted = pageIndex * columns + 1;
  const lastPrinted = Math.min(printedPages, firstPrinted + columns - 1);
  const pageLabel =
    lastPrinted > firstPrinted
      ? `Pages ${firstPrinted}–${lastPrinted}`
      : `Page ${firstPrinted}`;
  const pageSummary = pagesReady && printedPages > 1 ? `${pageLabel} of ${printedPages}` : '';

  /**
   * Whether to repeat the title above the section.
   *
   * Most books already open their chapter with a heading, and printing the title
   * twice is the single ugliest thing a reader can do. A section whose markup
   * has no heading of its own — one that starts with folded-in front matter —
   * still gets one, or it would open with no title at all.
   */
  const showHeading = !/<h[123][\s>]/i.test(chapterHtml);

  /**
   * Highlights in the section, mapped onto the fore-edge rail.
   *
   * A page has no scroll offset to measure against, so the rail becomes a map
   * of the whole section: each mark sits at the fraction of the way through the
   * pages its highlight was found. Measuring the painted range rather than the
   * stored anchor keeps it honest when a translation or a font change moves the
   * words but not the annotation.
   */
  const visibleMarkers = React.useMemo(() => {
    const article = articleRef.current;
    const geometry = pageBox ? pageLayout(pageBox.width, pageBox.height) : null;
    if (!article || !geometry || highlightState.ranges.size === 0) return [];
    const byId = new Map(annotations.annotations.map((item) => [item.id, item]));
    const last = Math.max(1, pageCount - 1);
    const marks: Array<{ marker: ReaderMarker; position: number }> = [];
    for (const [id, ranges] of highlightState.ranges) {
      const annotation = byId.get(id);
      const range = ranges[0];
      if (!annotation || !range) continue;
      const page = pageOfRange(article, geometry.width, range);
      marks.push({
        marker: { annotation, percent: clamp(page / last, 0, 1) },
        position: clamp(page / last, 0, 1),
      });
    }
    return marks.sort((a, b) => a.position - b.position);
  }, [annotations.annotations, highlightState.ranges, pageBox, pageCount]);

  const body = (() => {
    // `status === null` means "not asked yet" — during SSR and on the first
    // paint it is always null. Rendering the "no file" notice then would flash a
    // prompt at someone whose book is already on disk, so hold the skeleton
    // until the answer is actually in.
    if (status === null && !statusError) {
      return <ReaderSkeleton />;
    }

    if (!hasEpub) {
      return (
        <ReaderNotice
          bookId={bookId}
          icon={Upload}
          title="No book file yet"
          message={
            status?.error ??
            statusError ??
            'This book has no EPUB stored, so there is no text to read. Add the file from the book page — the layout and images come with it.'
          }
          action={
            <Button asChild icon={<Upload className="size-4" aria-hidden />}>
              <Link href={`/book/${bookId}`}>Add the book file</Link>
            </Button>
          }
        />
      );
    }

    if (loading && !ready) return <ReaderSkeleton />;

    if (loadError) {
      return (
        <ReaderNotice
          bookId={bookId}
          icon={FileAudio}
          title="Could not open that section"
          message={loadError}
          action={
            hasPrev ? (
              <Button variant="outline" onClick={() => goTo(chapterIndex - 1)}>
                Go back
              </Button>
            ) : undefined
          }
        />
      );
    }

    return (
      <article
        // Remounting per section is what makes the first page reliable and
        // drops the previous section's images from memory.
        key={chapterIndex}
        ref={articleRef}
        lang={translating ? (translations.language ?? bookLanguage) : bookLanguage}
        className="reader-prose"
        data-motion={pageMotion ? '' : undefined}
        style={{
          width: layout?.width,
          height: layout?.height,
          paddingInline: layout ? layout.gap / 2 : undefined,
          columnCount: layout?.columns,
          columnGap: layout?.gap,
          transform: layout ? `translate3d(${-pageIndex * layout.width}px, 0, 0)` : undefined,
          ['--reader-page-height' as string]: layout ? `${layout.height}px` : undefined,
        }}
      >
        {/* Typography comes from `reader-prose`, not from utility classes:
            the prose rules target descendants, which outrank a single class,
            so classes here would be silently dead. */}
        {showHeading && <h1>{ready ? payload.chapter.title : sectionTitle}</h1>}

        {/*
          A translation replaces the original text rather than sitting under it:
          a reader who has chosen a language wants one column of prose to read,
          not two, and the original is always one click away. The notice is part
          of the column rather than the chrome, because unlike the chrome it is
          never hidden — a machine translation should never be mistaken for the
          book.
        */}
        {translating && translations.entry ? (
          <>
            <p
              className="reader-note mb-6 flex flex-wrap items-center gap-x-3 gap-y-1 border-l-2 py-1 pl-3 pr-1 text-sm"
              style={{ borderColor: 'var(--reader-rule)', color: 'var(--reader-muted)' }}
            >
              <Languages className="size-4 shrink-0" aria-hidden />
              <span>
                Machine translation into <strong style={{ color: 'var(--reader-fg)' }}>{translations.entry.languageName}</strong>
                {translations.entry.stale ? ' — the source text has changed since this was made.' : '.'}
              </span>
              <button
                type="button"
                onClick={translations.showOriginal}
                className="font-medium underline underline-offset-4 hover:opacity-75"
                style={{ color: 'var(--reader-link)' }}
              >
                Read the original
              </button>
            </p>
            <TranslatedChapterText
              blocks={translations.entry.blocks}
              sourceIndices={translationSourceIndices}
              contentRef={contentRef}
            />
          </>
        ) : (
          <>
            {translations.missingHere && translations.languageName ? (
              <p
                className="reader-note mb-6 flex flex-wrap items-center gap-x-3 gap-y-1 border-l-2 py-1 pl-3 pr-1 text-sm"
                style={{ borderColor: 'var(--reader-rule)', color: 'var(--reader-muted)' }}
              >
                <Languages className="size-4 shrink-0" aria-hidden />
                <span>This section has no {translations.languageName} translation yet.</span>
                <button
                  type="button"
                  onClick={() => translations.setOpen(true)}
                  className="font-medium underline underline-offset-4 hover:opacity-75"
                  style={{ color: 'var(--reader-link)' }}
                >
                  Translate it
                </button>
              </p>
            ) : null}
            {/*
              The annotatable text only. Headings the reader adds, notices and
              the controls at the foot are excluded, so a highlight's block
              index means the same thing to the server as it does here.
            */}
            <ChapterText html={chapterHtml} contentRef={contentRef} />
          </>
        )}
      </article>
    );
  })();

  return (
    <div
      data-reader-theme={preferences.theme}
      style={{
        ['--reader-font' as string]: READER_FONT_STACKS[preferences.fontFamily].body,
        ['--reader-font-heading' as string]: READER_FONT_STACKS[preferences.fontFamily].heading,
      }}
      className="reader-page fixed inset-0 z-50 flex overflow-hidden"
    >
      <ContentsPanel
        open={contentsOpen}
        docked={docked}
        onClose={() => setContentsOpen(false)}
        rows={rows}
        chapterCount={chapters.length}
        activeKey={activeKey}
        overallProgress={progress}
        onSelect={openRow}
        bookTitle={displayTitle}
        author={displayAuthor || undefined}
        audioByIndex={audioMap}
        playingIndex={playingCurrentSection ? chapterIndex : null}
        preparingIndex={preparing}
        onPlayChapter={activateSection}
      />

      <div className="relative flex min-w-0 flex-1 flex-col">
        {/* The bars float over the stage, which reserves their measured height,
            so the page keeps the same size whether they are drawn or not. */}
        <div className="absolute inset-x-0 top-0 z-30">
          <header
            ref={headerRef}
            className="flex items-center gap-1.5 border-b px-2.5 py-2.5"
            style={chromeStyle}
          >
            <Button
              asChild
              variant="ghost"
              size="icon"
              aria-label="Back to book"
              className="shrink-0"
            >
              <Link href={`/book/${bookId}`}>
                <ArrowLeft className="size-4" aria-hidden />
              </Link>
            </Button>

            <div className="min-w-0 flex-1 text-center">
              <h1 className="truncate font-display text-[15px] font-medium leading-tight" style={{ color: 'var(--reader-fg)' }}>
                {hasEpub ? sectionTitle || displayTitle : displayTitle}
              </h1>
              {hasEpub ? (
                displayAuthor && (
                  <p className="truncate font-mono text-[9.5px] uppercase tracking-[0.12em]" style={{ color: 'var(--reader-muted)' }}>
                    {displayAuthor}
                  </p>
                )
              ) : null}
            </div>

            <Button
              variant="ghost"
              size="icon"
              onClick={() => {
                setContentsOpen((open) => !open);
                if (!docked) setStudyOpen(false);
              }}
              aria-label={contentsOpen ? 'Hide contents' : 'Contents'}
              aria-keyshortcuts="t"
              aria-pressed={contentsOpen}
              disabled={!hasEpub}
              className="shrink-0"
            >
              <List className="size-4" aria-hidden />
            </Button>
            {/* Search, highlights, saved places and words live behind one button:
                they are all "what have I done with this book", and the header
                has no room for four. */}
            <Button
              variant="ghost"
              size="icon"
              onClick={() => (studyOpen ? setStudyOpen(false) : openStudy('search'))}
              aria-label="Search, highlights and words"
              aria-keyshortcuts="Control+F"
              aria-pressed={studyOpen}
              disabled={!hasEpub}
              className="shrink-0"
            >
              <Search className="size-4" aria-hidden />
            </Button>
            {/* What can be done with this section. At the top rather than at its
                end: a paged chapter may be twenty spreads long, and a reader
                who wants to listen or translate should not have to finish it
                first. Same three actions as the section has ever had. */}
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button
                  variant="ghost"
                  size="icon"
                  aria-label="Section actions"
                  disabled={!hasEpub}
                  className="shrink-0"
                >
                  <Ellipsis className="size-4" aria-hidden />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="w-60">
                <DropdownMenuLabel>This section</DropdownMenuLabel>
                {currentAudio?.hasAudio ? (
                  <DropdownMenuItem onSelect={() => activateSection(chapterIndex)}>
                    {playingCurrentSection ? (
                      <Pause className="fill-current" aria-hidden />
                    ) : (
                      <Play className="fill-current" aria-hidden />
                    )}
                    {playingCurrentSection
                      ? 'Pause audio'
                      : `Listen · ${formatDuration(currentAudio.duration)}`}
                  </DropdownMenuItem>
                ) : null}
                <DropdownMenuItem
                  disabled={preparing === chapterIndex}
                  onSelect={() => void attachAudio(chapterIndex)}
                >
                  {preparing === chapterIndex ? (
                    <Loader2 className="animate-spin" aria-hidden />
                  ) : (
                    <Upload aria-hidden />
                  )}
                  {preparing === chapterIndex ? 'Preparing…' : 'Import audio…'}
                </DropdownMenuItem>
                <DropdownMenuItem
                  onSelect={() => translations.setOpen(true)}
                  aria-haspopup="dialog"
                >
                  <Languages aria-hidden />
                  {translating && translations.languageName
                    ? `Reading in ${translations.languageName}`
                    : 'Translate…'}
                </DropdownMenuItem>
                <DropdownMenuItem
                  disabled={narrating || preparingSection}
                  onSelect={() => void narrateSection()}
                >
                  {narrating || preparingSection ? (
                    <Loader2 className="animate-spin" aria-hidden />
                  ) : (
                    <Sparkles aria-hidden />
                  )}
                  {narrating || preparingSection ? 'Narrating…' : 'Narrate…'}
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
            <Button
              variant="ghost"
              size="icon"
              onClick={() => setSettingsOpen(true)}
              aria-label="Reading settings"
              aria-keyshortcuts="s"
              className="shrink-0"
            >
              <Type className="size-4" aria-hidden />
            </Button>
          </header>

          <div className="relative h-px" style={{ background: 'var(--reader-rule)' }} aria-hidden>
            <div
              className="h-full transition-[width] duration-150"
              style={{ width: `${Math.round(progress * 100)}%`, background: 'var(--reader-link)' }}
            />
            {/* Saved places, shown on the book's own measure. A tick is where a
                bookmark sits in the whole book, not in this section. */}
            {bookmarks.bookmarks.map((bookmark) => (
              <span
                key={bookmark.id}
                className="absolute -top-px h-[3px] w-px"
                style={{
                  left: `${overallProgress(chapters, bookmark.chapterIndex, bookmark.scrollPercent) * 100}%`,
                  background: 'var(--reader-fg)',
                  opacity: 0.55,
                }}
              />
            ))}
          </div>
        </div>

        {/*
          The page itself. The sheet is the paper, the leaf is the window onto
          the article's columns, and the article slides sideways by one spread
          per turn — see the paged-reading rules in `globals.css`. The sheet is
          sized around the measured text block so the margins stay a book's and
          the measure never stretches past Foliate's limits.
        */}
        <div
          ref={stageRef}
          className="reader-stage"
          style={{ paddingTop: chromeHeight, paddingBottom: footerHeight }}
        >
          <div
            className="reader-sheet"
            style={
              pageBox
                ? {
                    width: pageBox.width + pageBox.frameX * 2,
                    height: pageBox.height + pageBox.frameY * 2,
                    padding: `${pageBox.frameY}px ${pageBox.frameX}px`,
                  }
                : undefined
            }
          >
            <div
              ref={leafRef}
              className={cn('reader-leaf', !ready && 'reader-leaf--scroll')}
            >
              {body}
              {layout?.columns === 2 && ready && !loadError ? (
                <div className="reader-gutter" aria-hidden />
              ) : null}
            </div>
          </div>
        </div>

        {/* What a screen reader hears when a page turns. */}
        <p role="status" aria-live="polite" className="sr-only">
          {ready && pagesReady && pageSummary ? `${pageSummary}. ${sectionTitle}` : ''}
        </p>

        <footer
          ref={footerRef}
          className="absolute inset-x-0 bottom-0 z-30 flex items-center gap-1 border-t px-2 py-1.5"
          style={chromeStyle}
        >
          <Button
            variant="ghost"
            size="icon-sm"
            disabled={!canTurnBack}
            onClick={() => turn(-1)}
            aria-label={pageIndex > 0 ? 'Previous page' : 'Previous section'}
          >
            <ChevronLeft className="size-4" aria-hidden />
          </Button>
          <p
            className="flex min-w-0 flex-1 items-baseline justify-center gap-2 truncate text-center font-mono text-[10px] uppercase tracking-[0.1em]"
            style={{ color: 'var(--reader-muted)' }}
          >
            <span className="truncate">{hasEpub ? sectionTitle : ''}</span>
            {ready && pagesReady && printedPages > 1 ? (
              <span className="shrink-0 tabular-nums opacity-70">
                {firstPrinted === lastPrinted ? firstPrinted : `${firstPrinted}–${lastPrinted}`}
                /{printedPages}
              </span>
            ) : null}
          </p>
          <Button
            variant="ghost"
            size="icon-sm"
            onClick={toggleBookmark}
            disabled={!hasEpub}
            aria-label={currentBookmark ? 'Remove bookmark here' : 'Bookmark this page'}
            aria-keyshortcuts="b"
            aria-pressed={currentBookmark !== null}
            className="shrink-0"
          >
            <Bookmark
              className="size-4"
              fill={currentBookmark ? 'currentColor' : 'none'}
              aria-hidden
            />
          </Button>
          <Button
            variant="ghost"
            size="icon-sm"
            disabled={!canTurnForward}
            onClick={() => turn(1)}
            aria-label={pageIndex < pageCount - 1 ? 'Next page' : 'Next section'}
          >
            <ChevronRight className="size-4" aria-hidden />
          </Button>
        </footer>

        {/* Marks in the margin: a coloured tick where a highlight sits in the
            text on screen, so a long chapter's marks can be found by eye. */}
        <div
          aria-label="Highlights on this page"
          className="pointer-events-none absolute bottom-0 right-0 z-20 hidden w-3 sm:block"
          style={{ top: chromeHeight + footerHeight }}
        >
          {visibleMarkers.map(({ marker, position }) => (
            <button
              key={marker.annotation.id}
              type="button"
              onClick={() => openAnnotation(marker.annotation)}
              aria-label={`Highlight: ${marker.annotation.selectedText.slice(0, 60)}`}
              title={marker.annotation.selectedText.slice(0, 90)}
              className="pointer-events-auto absolute right-0.5 h-[3px] w-2 rounded-full transition-transform hover:scale-x-150 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--reader-link)]"
              style={{
                top: `${Math.min(99, Math.max(0, position * 100))}%`,
                background: MARKER_COLORS[marker.annotation.color],
                animation: 'marker-in 200ms var(--ease-out-quint)',
              }}
            />
          ))}
        </div>

        <StudyPanel
          open={studyOpen}
          docked={docked}
          tab={studyTab}
          onTabChange={setStudyTab}
          onClose={() => setStudyOpen(false)}
          search={search}
          onOpenResult={openSearchResult}
          readingLanguageName={
            translations.language
              ? translations.languageName ?? translations.language
              : null
          }
          annotations={annotations.annotations}
          onOpenAnnotation={openAnnotation}
          onEditAnnotation={editAnnotationNote}
          onDeleteAnnotation={(id) => void annotations.remove(id)}
          bookmarks={bookmarks.bookmarks}
          onOpenBookmark={openBookmark}
          onDeleteBookmark={(id) => void bookmarks.remove(id)}
          onToggleBookmark={toggleBookmark}
          bookmarkedHere={currentBookmark !== null}
          words={savedWords.words}
          onLookupWord={lookupWord}
          onDeleteWord={(id) => void savedWords.remove(id)}
        />
      </div>

      {selectionDraft ? (
        <SelectionToolbar
          selection={selectionDraft}
          onHighlight={(color) => void highlightSelection(color)}
          onNote={noteSelection}
          onDefine={defineSelection}
          onCopy={() => void copySelection()}
          onSearch={() => searchInBook(selectionDraft.text)}
          onSpeak={speakSelection}
        />
      ) : null}

      {activeAnnotation && annotationAnchor ? (
        <AnnotationCard
          annotation={activeAnnotation}
          anchor={annotationAnchor}
          onClose={() => {
            setActiveAnnotationId(null);
            setAnnotationAnchor(null);
          }}
          onColor={(color) => void annotations.update(activeAnnotation.id, { color })}
          onEditNote={() => editAnnotationNote(activeAnnotation)}
          onDefine={defineAnnotation}
          onSearch={() =>
            searchInBook(activeAnnotation.selectedText || activeAnnotation.quote)
          }
          onDelete={() => {
            void annotations.remove(activeAnnotation.id);
            setActiveAnnotationId(null);
            setAnnotationAnchor(null);
          }}
        />
      ) : null}

      <NoteDialog
        open={noteComposer !== null}
        onOpenChange={(open) => {
          if (!open) setNoteComposer(null);
        }}
        mode={noteComposer?.mode ?? 'create'}
        quote={noteComposer?.quote ?? ''}
        note={noteComposer?.note ?? ''}
        color={noteComposer?.color ?? 'yellow'}
        busy={noteBusy}
        onSave={(text, color) => void saveNote(text, color)}
        onDelete={noteComposer?.mode === 'edit' ? () => void deleteComposedNote() : undefined}
      />

      <DictionaryDialog
        open={dictionaryRequest !== null}
        onOpenChange={(open) => {
          if (!open) setDictionaryRequest(null);
        }}
        request={dictionaryRequest}
        savedWords={savedWords.words}
        onSave={savedWords.save}
        onRemove={savedWords.remove}
        onSearchInBook={searchInBook}
      />

      <ReaderSettings
        open={settingsOpen}
        onOpenChange={setSettingsOpen}
        preferences={preferences}
        onChange={(patch) => setPreferences((current) => ({ ...current, ...patch }))}
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

      <TranslateDialog
        open={translations.open}
        onOpenChange={translations.setOpen}
        sectionTitle={sectionTitle}
        saved={translations.saved}
        language={translations.language}
        languageName={translations.languageName}
        showing={translations.showing}
        missingHere={translations.missingHere}
        bookLanguages={translations.bookLanguages}
        source={translations.source}
        detectingSource={translations.detectingSource}
        progress={translations.progress}
        busy={translations.busy}
        showOriginal={translations.showOriginal}
        showTranslation={translations.showTranslation}
        translate={translations.translate}
        remove={translations.remove}
      />

      {uploadFor !== null && audioMap.get(uploadFor)?.chapterId && (
        <UploadAudioDialog
          open
          onOpenChange={(open) => {
            if (!open) setUploadFor(null);
          }}
          chapterId={audioMap.get(uploadFor)!.chapterId!}
          chapterNumber={uploadFor + 1}
          chapterTitle={currentTitle(chapters, uploadFor)}
          bookTitle={displayTitle}
          onUploaded={() => {
            void refreshAudioMap();
            toast.success('Audio attached. Play it from here or from the chapter list.');
          }}
        />
      )}
    </div>
  );
}

/* -------------------------------------------------------------------------- */
/* Pieces                                                                      */
/* -------------------------------------------------------------------------- */

const chromeStyle: React.CSSProperties = {
  background: 'var(--reader-chrome)',
  backdropFilter: 'blur(16px) saturate(140%)',
  WebkitBackdropFilter: 'blur(16px) saturate(140%)',
  borderColor: 'var(--reader-rule)',
  color: 'var(--reader-muted)',
};

/** Empty / error / prompting state, shared by the pre-book and mid-book cases. */
function ReaderNotice({
  bookId,
  icon: Icon,
  title,
  message,
  action,
  busy,
}: {
  bookId: string;
  icon: typeof Upload;
  title: string;
  message: string;
  action?: React.ReactNode;
  busy?: boolean;
}) {
  return (
    <div className="flex min-h-full flex-col items-center justify-center gap-4 px-6 py-16 text-center">
      <span
        className="grid size-12 place-items-center"
        style={{ border: '1px solid var(--reader-rule)', color: 'var(--reader-muted)' }}
      >
        <Icon className={cn('size-5', busy && 'animate-spin')} aria-hidden />
      </span>
      <div>
        <h2 className="font-display text-xl font-medium" style={{ color: 'var(--reader-fg)' }}>
          {title}
        </h2>
        <p className="mx-auto mt-1.5 max-w-sm text-sm" style={{ color: 'var(--reader-muted)' }}>
          {message}
        </p>
      </div>
      {action}
      <Link
        href={`/book/${bookId}`}
        className="text-xs underline underline-offset-4 hover:opacity-75"
        style={{ color: 'var(--reader-muted)' }}
      >
        Back to book
      </Link>
    </div>
  );
}

function ReaderSkeleton() {
  return (
    <div className="px-5 py-14 sm:px-8" style={{ maxWidth: 620, marginInline: 'auto' }}>
      <div className="skeleton mb-5 h-9 w-2/3" />
      <div className="space-y-3">
        {Array.from({ length: 14 }).map((_, index) => (
          <div
            key={index}
            className="skeleton h-4"
            style={{ width: `${94 - (index % 5) * 12}%` }}
          />
        ))}
      </div>
    </div>
  );
}

/* -------------------------------------------------------------------------- */
/* Helpers                                                                     */
/* -------------------------------------------------------------------------- */

/**
 * Whether the contents can sit beside the text rather than over it.
 *
 * `useSyncExternalStore` rather than a `matchMedia` in an effect, because the
 * reader has to render the same way on the server as on the first client paint:
 * `false` on both sides, so the panel is a drawer until the real width is known
 * and nothing reflows under a reader who has already started.
 */
function useDockedContents(): boolean {
  const query = React.useMemo(
    () =>
      typeof window === 'undefined' ? null : window.matchMedia(`(min-width: ${DOCK_MIN_WIDTH}px)`),
    [],
  );

  const subscribe = React.useCallback(
    (onChange: () => void) => {
      if (!query) return () => {};
      query.addEventListener('change', onChange);
      return () => query.removeEventListener('change', onChange);
    },
    [query],
  );

  return React.useSyncExternalStore(
    subscribe,
    () => query?.matches ?? false,
    () => false,
  );
}

function currentTitle(chapters: EpubChapterEntry[], index: number): string {
  return chapters.find((chapter) => chapter.index === index)?.title ?? '';
}

/**
 * The DOM position under a pointer.
 *
 * `caretRangeFromPoint` is the WebKit/Blink spelling, `caretPositionFromPoint`
 * the Firefox one. Highlight ranges are compared with `Range.isPointInRange`,
 * which needs a node and an offset rather than coordinates.
 */
function caretFromPoint(x: number, y: number): { node: Node; offset: number } | null {
  const doc = document as Document & {
    caretRangeFromPoint?: (x: number, y: number) => Range | null;
    caretPositionFromPoint?: (x: number, y: number) => { offsetNode: Node; offset: number } | null;
  };
  const range = doc.caretRangeFromPoint?.(x, y);
  if (range) return { node: range.startContainer, offset: range.startOffset };
  const position = doc.caretPositionFromPoint?.(x, y);
  return position ? { node: position.offsetNode, offset: position.offset } : null;
}

/** The first word of a selection, without the punctuation around it. */
function firstWord(text: string): string {
  const first = text.trim().split(/\s+/)[0] ?? '';
  return first.replace(/^[^\p{L}\p{N}]+|[^\p{L}\p{N}]+$/gu, '');
}


/**
 * Share of the whole book behind the current position.
 *
 * Weighted by word count rather than section count: a book with a two-page
 * preface and thirty long chapters would otherwise sit at 3% after reading
 * the entire preface. Empty sections still count for one word so they do not
 * divide by zero or stall the bar.
 */
export function overallProgress(
  chapters: EpubChapterEntry[],
  currentIndex: number,
  scrollPercent: number,
): number {
  if (chapters.length === 0) return 0;

  const weight = (wordCount: number) => Math.max(1, wordCount);
  let total = 0;
  let before = 0;
  let own = 0;

  for (const chapter of chapters) {
    const size = weight(chapter.wordCount);
    total += size;
    if (chapter.index < currentIndex) before += size;
    else if (chapter.index === currentIndex) own = size;
  }

  return clamp((before + own * clamp(scrollPercent, 0, 1)) / total, 0, 1);
}
