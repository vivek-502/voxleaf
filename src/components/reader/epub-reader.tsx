import * as React from 'react';
import { Link } from 'react-router-dom';
import { toast } from 'sonner';
import {
  ArrowLeft,
  ChevronLeft,
  ChevronRight,
  FileAudio,
  Languages,
  List,
  Loader2,
  Pause,
  Play,
  Sparkles,
  Type,
  Upload,
} from 'lucide-react';

import { ContentsPanel } from './contents-panel';
import { activeRow, buildContentsRows, type ContentsRow } from './contents-rows';
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
  buildTextIndex,
  offsetAtViewportTop,
  scrollToOffset,
  type TextBlock,
} from './text-index';
import { Button } from '@/components/ui/button';
import { UploadAudioDialog } from '@/components/chapters/upload-audio-dialog';
import { usePlayer } from '@/components/providers/player-provider';
import { api, errorMessage } from '@/hooks/use-api';
import { useEpubStatus } from '@/hooks/use-epub';
import { useTtsGeneration } from '@/hooks/use-tts';
import { clamp, debounce, formatDuration } from '@/utils/format';
import { cn } from '@/utils/cn';
import type { EpubAudioEntry, EpubChapterEntry, EpubChapterPayload, EpubProgressInfo } from '@/types';

/** Debounce for the scroll-position write. */
const SAVE_DEBOUNCE_MS = 700;

/** Chrome stays visible while near either end of the document. */
const CHROME_EDGE_PX = 90;

/** Below this the contents sits over the text; above it, beside it. */
const DOCK_MIN_WIDTH = 900;

/** Breathing room above a heading a contents row jumps to. */
const JUMP_PADDING_PX = 12;
/**
 * How far below the top edge the viewport counts as "here".
 *
 * A row has to be a little way into view before it counts as the one being read,
 * or the highlight flickers between a heading and the paragraph under it while the
 * heading is still half on screen.
 */
const VIEWPORT_PROBE_PX = 12;

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
 * 300-document book costs one request rather than one per section, and reading
 * is a continuous scroll — which is what people expect on a phone and what
 * keeps browser find-in-page working.
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
  const [chromeVisible, setChromeVisible] = React.useState(true);
  const [scrollPercent, setScrollPercent] = React.useState(0);
  const [audioMap, setAudioMap] = React.useState<Map<number, EpubAudioEntry>>(new Map());
  const [uploadFor, setUploadFor] = React.useState<number | null>(null);
  const [preparing, setPreparing] = React.useState<number | null>(null);
  const [preferences, setPreferences] = React.useState<ReaderPreferences>(DEFAULT_PREFERENCES);
  /** Key of the contents row the reader is currently in. */
  const [activeKey, setActiveKey] = React.useState<string | null>(null);

  const scrollRef = React.useRef<HTMLDivElement>(null);
  const articleRef = React.useRef<HTMLElement>(null);
  /** Block index of the loaded section. A cache of the live DOM — see `ensureBlocks`. */
  const blocksRef = React.useRef<TextBlock[]>([]);
  const restoredRef = React.useRef(false);
  /**
   * The reader jumps the scroll container itself on two occasions: resetting to
   * the top of a new section, and putting back the offset from the last visit.
   * Neither is where the reader actually is, so neither may be saved.
   */
  const autoScrollRef = React.useRef(false);
  /**
   * True once the saved position has been read (or has failed to load).
   *
   * Before that, saving is actively harmful: the first section mounts at index
   * 0, its reset scroll queues a save, and 700ms later that stale `{0, 0}`
   * overwrites the position the reader was about to restore — losing the place
   * in the book on every visit.
   */
  const progressReadyRef = React.useRef(false);
  /** Where to restore the scroll to, read once when progress loads. */
  const restorePercentRef = React.useRef(0);
  /**
   * Contents row a click asked for, waiting for its section to arrive.
   *
   * Navigation addresses a place in the text, not a file, so choosing a row that
   * lives in another section is two steps: open the section, then land on the
   * offset inside it. The row is kept here because the offset only means
   * something once the markup for that section is in the DOM.
   */
  const pendingRowRef = React.useRef<ContentsRow | null>(null);
  /** Last reported active row, so scrolling only re-renders on a real change. */
  const activeKeyRef = React.useRef<string | null>(null);
  /**
   * Where a jump put the reader, while they are still sitting there.
   *
   * Not every contents offset points exactly at its heading. When a book's table
   * of contents names spots inside one file with no anchor to aim at, the offset
   * is an estimate proportional to the text — good to a paragraph, sometimes a
   * couple of pages out. Deriving the highlight from the scroll position would
   * then report the row *before* the one that was chosen, which reads as the
   * reader having clicked the wrong thing.
   *
   * So an explicit choice holds the highlight until the reader scrolls away from
   * where it put them. After that the viewport is the honest answer again.
   */
  const settledAtRef = React.useRef<number | null>(null);

  const hasEpub = status?.hasEpub === true;
  const docked = useDockedContents();

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
   * Called from the scroll handler and directly whenever the text under the
   * viewport changes without a scroll — a section opening, a restored position
   * being applied, a jump landing — so the highlight is never left pointing at
   * wherever the reader was before they moved on. A row that has not changed is
   * not reported at all: this runs on every scroll frame, and the panel
   * re-renders all of its rows with it.
   */
  const syncActiveRow = React.useCallback(() => {
    const node = scrollRef.current;
    if (!node) return;
    // Translated text has no place in the contents, so there is no row to be in.
    // The section being read stays marked in the panel instead.
    if (translatingRef.current) return;
    // While a jump is still where it left the reader, the row they chose is the
    // row in view — see `settledAtRef`.
    const settled = settledAtRef.current;
    if (settled !== null && Math.abs(node.scrollTop - settled) <= 2) return;
    settledAtRef.current = null;

    const row = activeRow(
      rows,
      chapterIndex,
      offsetAtViewportTop(node, ensureBlocks(), VIEWPORT_PROBE_PX, reindex),
    );
    const key = row?.key ?? null;
    if (key === activeKeyRef.current) return;
    activeKeyRef.current = key;
    setActiveKey(key);
  }, [chapterIndex, ensureBlocks, reindex, rows]);

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
          fontSize: saved.fontSize ?? DEFAULT_PREFERENCES.fontSize,
          lineHeight: saved.lineHeight ?? DEFAULT_PREFERENCES.lineHeight,
          columnWidth: saved.columnWidth ?? DEFAULT_PREFERENCES.columnWidth,
          theme: saved.theme ?? DEFAULT_PREFERENCES.theme,
          fontFamily: saved.fontFamily ?? DEFAULT_PREFERENCES.fontFamily,
        });
        restorePercentRef.current = clamp(saved.scrollPercent ?? 0, 0, 1);
        if (saved.chapterIndex > 0) setChapterIndex(saved.chapterIndex);
      } catch {
        /* defaults are a fine fallback */
      } finally {
        // Unblocks saving. Anything dropped before this point was the reader's
        // own reset scroll; the next real scroll writes the true position.
        progressReadyRef.current = true;
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

  // Typography is saved separately from position, debounced so dragging a
  // slider does not fire a request per pixel.
  const preferencesRef = React.useRef(preferences);
  preferencesRef.current = preferences;

  React.useEffect(() => {
    if (!hasEpub) return;
    const timer = setTimeout(() => {
      const current = preferencesRef.current;
      void api
        .patch(`/api/epub/${bookId}/progress`, {
          fontSize: current.fontSize,
          lineHeight: current.lineHeight,
          columnWidth: current.columnWidth,
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

  // A new section starts at the top; the previous offset is meaningless here.
  React.useEffect(() => {
    // Assigning scrollTop fires a scroll event, so mark the jump as automatic
    // and re-arm on the next frame, once that event has been delivered.
    autoScrollRef.current = true;
    settledAtRef.current = null;
    if (scrollRef.current) scrollRef.current.scrollTop = 0;
    setScrollPercent(0);
    setChromeVisible(true);

    const frame = requestAnimationFrame(() => {
      autoScrollRef.current = false;
    });
    return () => cancelAnimationFrame(frame);
  }, [chapterIndex]);

  // Restore the saved offset once, after the section's markup is in the DOM.
  React.useEffect(() => {
    if (restoredRef.current || !ready || translatingRef.current) return;
    restoredRef.current = true;

    const saved = restorePercentRef.current;
    if (saved <= 0.01) return;

    const node = scrollRef.current;
    if (!node) return;

    const apply = () => {
      const max = node.scrollHeight - node.clientHeight;
      if (max <= 0) return;
      // Same reasoning as the reset: the restored offset is not a new position.
      autoScrollRef.current = true;
      node.scrollTop = max * saved;
      requestAnimationFrame(() => {
        autoScrollRef.current = false;
        syncActiveRow();
      });
    };

    requestAnimationFrame(apply);
  }, [ready, syncActiveRow]);

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
   * Images that have not loaded yet push the text below them down, so a jump
   * measured immediately after injection can come up short. The jump is repeated
   * on every image load that arrives *and* finds the reader still exactly where
   * the jump left them; once they have moved on their own, it stops interfering.
   */
  React.useEffect(() => {
    const row = pendingRowRef.current;
    if (!ready || !row || row.index !== chapterIndex) return;
    // In translated text there is no offset to land on, so the row's section is
    // as close as a jump can honestly get. Clear it either way, or the next
    // section would be sent to this row's offset.
    if (translatingRef.current) {
      pendingRowRef.current = null;
      return;
    }
    pendingRowRef.current = null;

    const node = scrollRef.current;
    if (!node) return;

    let landed = 0;
    const apply = () => {
      autoScrollRef.current = true;
      landed = scrollToOffset(node, ensureBlocks(), row.offset, JUMP_PADDING_PX, reindex);
      const max = node.scrollHeight - node.clientHeight;
      const percent = max <= 0 ? 1 : clamp(landed / max, 0, 1);
      // The jump is where the reader now is, so it is the position to keep.
      setScrollPercent(percent);
      savePosition(chapterIndex, percent);
      // Hold the row the reader chose while they are sitting where it put them.
      settledAtRef.current = landed;
      activeKeyRef.current = row.key;
      setActiveKey(row.key);
      requestAnimationFrame(() => {
        autoScrollRef.current = false;
      });
    };

    const frame = requestAnimationFrame(apply);
    const onLoad = () => {
      if (Math.abs(node.scrollTop - landed) > 2) return;
      apply();
    };
    node.addEventListener('load', onLoad, true);

    return () => {
      cancelAnimationFrame(frame);
      node.removeEventListener('load', onLoad, true);
    };
  }, [chapterIndex, ensureBlocks, ready, reindex, savePosition, syncActiveRow]);

  /* ---------------------------------------------------------------------- */
  /* Scroll                                                                 */
  /* ---------------------------------------------------------------------- */

  React.useEffect(() => {
    const node = scrollRef.current;
    if (!node) return;

    let frame = 0;
    const handle = () => {
      frame = 0;
      if (autoScrollRef.current) return;
      const max = node.scrollHeight - node.clientHeight;
      const percent = max <= 0 ? 1 : clamp(node.scrollTop / max, 0, 1);
      setScrollPercent(percent);
      // Hide the chrome while reading the middle of a section.
      setChromeVisible(node.scrollTop < CHROME_EDGE_PX || percent > 0.995);
      savePosition(chapterIndex, percent);
      syncActiveRow();
    };

    const onScroll = () => {
      if (frame) return;
      frame = requestAnimationFrame(handle);
    };

    node.addEventListener('scroll', onScroll, { passive: true });
    return () => {
      node.removeEventListener('scroll', onScroll);
      if (frame) cancelAnimationFrame(frame);
    };
  }, [chapterIndex, savePosition, syncActiveRow]);

  /* ---------------------------------------------------------------------- */
  /* Navigation                                                             */
  /* ---------------------------------------------------------------------- */

  const lastIndex = Math.max(0, chapters.length - 1);
  const hasPrev = chapterIndex > 0;
  const hasNext = chapterIndex < lastIndex;

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
      const node = scrollRef.current;
      if (row.index === chapterIndex && node) {
        autoScrollRef.current = true;
        const landed = scrollToOffset(node, ensureBlocks(), row.offset, JUMP_PADDING_PX, reindex);
        const max = node.scrollHeight - node.clientHeight;
        setScrollPercent(max <= 0 ? 1 : clamp(landed / max, 0, 1));
        setChromeVisible(true);
        // Hold this row until the reader scrolls away — see `settledAtRef`.
        settledAtRef.current = landed;
        activeKeyRef.current = row.key;
        setActiveKey(row.key);
        if (!docked) setContentsOpen(false);
        requestAnimationFrame(() => {
          autoScrollRef.current = false;
        });
        return;
      }

      pendingRowRef.current = row;
      goTo(row.index);
      if (!docked) setContentsOpen(false);
    },
    [chapterIndex, docked, ensureBlocks, goTo, reindex],
  );
  // Keyboard shortcuts. Bound at the window so they work without a focused
  // element, but skipped when the user is typing or a dialog owns focus.
  React.useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.metaKey || event.ctrlKey || event.altKey) return;

      const target = event.target as HTMLElement | null;
      const tag = target?.tagName ?? '';
      if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || target?.isContentEditable) {
        return;
      }

      switch (event.key) {
        case 'ArrowLeft':
        case 'PageUp':
          if (hasPrev) {
            event.preventDefault();
            goTo(chapterIndex - 1);
          }
          break;
        case 'ArrowRight':
        case 'PageDown':
          if (hasNext) {
            event.preventDefault();
            goTo(chapterIndex + 1);
          }
          break;
        case ' ':
          event.preventDefault();
          setChromeVisible((visible) => !visible);
          break;
        case 't':
        case 'T':
          setContentsOpen((open) => !open);
          break;
        case 's':
        case 'S':
          setSettingsOpen((open) => !open);
          break;
        default:
          break;
      }
    };

    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [chapterIndex, goTo, hasNext, hasPrev]);

  /* ---------------------------------------------------------------------- */
  /* Audio in the reader                                                    */
  /* ---------------------------------------------------------------------- */

  const { playChapter, track, isPlaying } = usePlayer();

  const currentAudio = audioMap.get(chapterIndex);
  /**
   * Narrate the section on screen.
   *
   * Chapters are created lazily, so a section may be readable but not yet exist
   * as a chapter — and narration needs a chapter to attach audio to. The
   * `ensure` call materialises it first, which is also what gives the generated
   * file a sensible name instead of a blank one.
   */
  const narrateSection = async () => {
    const entry = chapters.find((item) => item.index === chapterIndex);
    if (!entry) {
      toast.error('That section is no longer part of the book.');
      return;
    }

    setPreparing(chapterIndex);
    try {
      // Cheap when the chapter already exists, and the response is that chapter,
      // so there is no need to look its id up first.
      const result = await api.post<{ id: string }>(
        `/api/epub/${bookId}/chapter/${chapterIndex}/ensure`,
        { title: entry.title },
      );
      setPreparing(null);
      setNarratingSection(true);
      // The language being read is the language that should be spoken: a reader
      // looking at Hindi should not get an English narration of it.
      await generate(result.id, entry.title || `section ${chapterIndex + 1}`, translations.language);
    } catch (error) {
      toast.error(errorMessage(error, 'Could not prepare that section for narration.'));
    } finally {
      setNarratingSection(false);
      setPreparing(null);
    }
  };

  const playingCurrentSection =
    currentAudio?.hasAudio === true &&
    currentAudio.chapterId !== null &&
    track?.chapterId === currentAudio.chapterId &&
    isPlaying;

  // Narrating the section on screen. The button sits beside Translate because
  // both answer the same question — what else can I do with this section — and
  // because a reader who has just finished a section is exactly who wants it.
  // Narrating the section on screen. The button sits beside Translate because
  // both answer the same question — what else can I do with this section — and
  // because a reader who has just finished a section is exactly who wants it.
  //
  // `busyChapters` cannot answer this on its own: a section that has never been
  // narrated has no chapter id until `ensure` runs, and no audio-map entry at
  // all, so the section being narrated is tracked here instead.
  const [narratingSection, setNarratingSection] = React.useState(false);
  const { busyChapters, generate } = useTtsGeneration(refreshAudioMap);
  const preparingSection = preparing === chapterIndex;
  const narrating = narratingSection || busyChapters.size > 0;

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
   * Whether to repeat the title above the section.
   *
   * Most books already open their chapter with a heading, and printing the title
   * twice is the single ugliest thing a reader can do. A section whose markup
   * has no heading of its own — one that starts with folded-in front matter —
   * still gets one, or it would open with no title at all.
   */
  const showHeading = !/<h[123][\s>]/i.test(chapterHtml);

  const body = (() => {
    // `status === null` means "not asked yet" — during SSR and on the first
    // paint it is always null. Rendering the "no file" notice then would flash a
    // prompt at someone whose book is already on disk, so hold the skeleton
    // until the answer is actually in.
    if (status === null && !statusError) {
      return <ReaderSkeleton columnWidth={preferences.columnWidth} />;
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
              <Link to={`/book/${bookId}`}>Add the book file</Link>
            </Button>
          }
        />
      );
    }

    if (loading && !ready) return <ReaderSkeleton columnWidth={preferences.columnWidth} />;

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
        // Remounting per section is what makes `scrollTop = 0` reliable and
        // drops the previous section's images from memory.
        key={chapterIndex}
        ref={articleRef}
        className="reader-prose px-5 py-10 sm:px-8 sm:py-14"
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
              className="mb-6 flex flex-wrap items-center gap-x-3 gap-y-1 rounded-lg border px-3.5 py-2 text-sm"
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
            <TranslatedText blocks={translations.entry.blocks} />
          </>
        ) : (
          <>
            {translations.missingHere && translations.languageName ? (
              <p
                className="mb-6 flex flex-wrap items-center gap-x-3 gap-y-1 rounded-lg border px-3.5 py-2 text-sm"
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
            <div dangerouslySetInnerHTML={{ __html: chapterHtml }} />
          </>
        )}

        <SectionAudio
          state={
            preparing === chapterIndex
              ? 'preparing'
              : playingCurrentSection
                ? 'playing'
                : currentAudio?.hasAudio
                  ? 'hasAudio'
                  : 'empty'
          }
          duration={currentAudio?.duration ?? 0}
          progress={currentAudio?.progress ?? 0}
          onClick={() => activateSection(chapterIndex)}
          translation={
            <div className="flex gap-2 flex-wrap">
              <button
                type="button"
                onClick={() => translations.setOpen(true)}
                aria-haspopup="dialog"
                className="inline-flex items-center gap-2.5 rounded-full border px-4 py-2 text-sm font-medium transition-opacity hover:opacity-75"
                style={{ borderColor: 'var(--reader-rule)', color: 'var(--reader-link)' }}
              >
                <Languages className="size-4" aria-hidden />
                {translating && translations.languageName
                  ? `Reading in ${translations.languageName}`
                  : translations.saved.length > 0
                    ? `Translate · ${translations.saved.length} saved`
                    : 'Translate'}
              </button>
              <button
                type="button"
                onClick={() => void narrateSection()}
                disabled={narrating || preparingSection}
                className="inline-flex items-center gap-2.5 rounded-full border px-4 py-2 text-sm font-medium transition-opacity hover:opacity-75 disabled:opacity-50"
                style={{ borderColor: 'var(--reader-rule)', color: 'var(--reader-link)' }}
              >
                {narrating || preparingSection ? (
                  <Loader2 className="size-4 animate-spin" aria-hidden />
                ) : (
                  <Sparkles className="size-4" aria-hidden />
                )}
                {narrating || preparingSection ? 'Narrating…' : translations.languageName ? `Narrate in ${translations.languageName}` : 'Narrate'}
              </button>
              <button
                type="button"
                onClick={() => setUploadFor(chapterIndex)}
                className="inline-flex items-center gap-2.5 rounded-full border px-4 py-2 text-sm font-medium transition-opacity hover:opacity-75"
                style={{ borderColor: 'var(--reader-rule)', color: 'var(--reader-link)' }}
              >
                <Upload className="size-4" aria-hidden />
                Import audio
              </button>
            </div>
          }
        />

        <nav className="mt-10 flex items-center justify-between gap-3">
          <Button
            variant="ghost"
            disabled={!hasPrev}
            onClick={() => goTo(chapterIndex - 1)}
            icon={<ChevronLeft className="size-4" aria-hidden />}
          >
            <span className="truncate">Previous</span>
          </Button>
          <span className="shrink-0 text-xs tabular-nums" style={{ color: 'var(--reader-muted)' }}>
            {chapterIndex + 1} / {Math.max(1, chapters.length)}
          </span>
          <Button variant="ghost" disabled={!hasNext} onClick={() => goTo(chapterIndex + 1)}>
            <span className="truncate">Next</span>
            <ChevronRight className="size-4" aria-hidden />
          </Button>
        </nav>
      </article>
    );
  })();

  return (
    <div
      data-reader-theme={preferences.theme}
      style={{
        ['--reader-font-size' as string]: `${preferences.fontSize}px`,
        ['--reader-line-height' as string]: String(preferences.lineHeight),
        ['--reader-column' as string]: `${preferences.columnWidth}px`,
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

      <div className="flex min-w-0 flex-1 flex-col">
        <header
          className={cn(
            'z-30 flex shrink-0 items-center gap-1.5 border-b px-2.5 py-2.5 transition-transform duration-200',
            chromeVisible ? 'translate-y-0' : '-translate-y-full',
          )}
          style={chromeStyle}
        >
          <Button
            asChild
            variant="ghost"
            size="icon"
            aria-label="Back to book"
            className="shrink-0"
          >
            <Link to={`/book/${bookId}`}>
              <ArrowLeft className="size-4" aria-hidden />
            </Link>
          </Button>

          <div className="min-w-0 flex-1 text-center">
            <p className="truncate text-sm font-semibold" style={{ color: 'var(--reader-fg)' }}>
              {hasEpub ? sectionTitle || displayTitle : displayTitle}
            </p>
            {hasEpub ? (
              displayAuthor && (
                <p className="truncate text-xs" style={{ color: 'var(--reader-muted)' }}>
                  {displayAuthor}
                </p>
              )
            ) : null}
          </div>

          <Button
            variant="ghost"
            size="icon"
            onClick={() => setContentsOpen((open) => !open)}
            aria-label={contentsOpen ? 'Hide contents' : 'Contents'}
            aria-keyshortcuts="t"
            aria-pressed={contentsOpen}
            disabled={!hasEpub}
            className="shrink-0"
          >
            <List className="size-4" aria-hidden />
          </Button>
          {/* In the header as well as at the foot of the section: the footer is
              off-screen for most of a chapter, and switching back to the original
              has to be possible from anywhere. */}
          <Button
            variant="ghost"
            size="icon"
            onClick={() => translations.setOpen(true)}
            aria-label={
              translating && translations.languageName
                ? `Reading in ${translations.languageName}`
                : 'Translate this section'
            }
            aria-haspopup="dialog"
            disabled={!hasEpub}
            className="shrink-0"
          >
            <Languages className={cn('size-4', translating && 'text-accent')} aria-hidden />
          </Button>
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

        <div className="h-px shrink-0" style={{ background: 'var(--reader-rule)' }} aria-hidden>
          <div
            className="h-full transition-[width] duration-150"
            style={{ width: `${Math.round(progress * 100)}%`, background: 'var(--reader-link)' }}
          />
        </div>

        <div ref={scrollRef} className="flex-1 overflow-y-auto overscroll-contain">
          {body}
        </div>

        <footer
          className={cn(
            'flex shrink-0 items-center gap-1 border-t px-2 py-1.5 transition-transform duration-200',
            chromeVisible ? 'translate-y-0' : 'translate-y-full',
          )}
          style={chromeStyle}
        >
          <Button
            variant="ghost"
            size="icon-sm"
            disabled={!hasPrev}
            onClick={() => goTo(chapterIndex - 1)}
            aria-label="Previous section"
          >
            <ChevronLeft className="size-4" aria-hidden />
          </Button>
          <p
            className="flex-1 truncate text-center text-xs"
            style={{ color: 'var(--reader-muted)' }}
          >
            {hasEpub ? sectionTitle : ''}
          </p>
          <Button
            variant="ghost"
            size="icon-sm"
            disabled={!hasNext}
            onClick={() => goTo(chapterIndex + 1)}
            aria-label="Next section"
          >
            <ChevronRight className="size-4" aria-hidden />
          </Button>
        </footer>
      </div>

      <ReaderSettings
        open={settingsOpen}
        onOpenChange={setSettingsOpen}
        preferences={preferences}
        onChange={(patch) => setPreferences((current) => ({ ...current, ...patch }))}
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

/**
 * What the reader can do with the section it is on: listen to it, or translate
 * it.
 *
 * Doubles as the import entry point for audio: with none attached it offers to
 * add one, so reading and listening stay in the same place. The translate
 * control sits beside it because they are the same kind of decision — what else
 * can I do with this section — and because at the end of a section is exactly
 * where a reader who has just finished it in another language wants to be.
 */
function SectionAudio({
  state,
  duration,
  progress,
  onClick,
  translation,
}: {
  state: 'empty' | 'preparing' | 'hasAudio' | 'playing';
  duration: number;
  progress: number;
  onClick: () => void;
  /** The translate control, rendered beside the listen one. */
  translation?: React.ReactNode;
}) {
  const labels: Record<typeof state, string> = {
    empty: 'Add audio for this section',
    preparing: 'Preparing…',
    hasAudio: `Listen · ${formatDuration(duration)}`,
    playing: 'Pause',
  };

  return (
    <section
      className="mt-12 flex flex-wrap items-center gap-3 border-t pt-6"
      style={{ borderColor: 'var(--reader-rule)' }}
    >
      <button
        type="button"
        onClick={onClick}
        disabled={state === 'preparing'}
        className="inline-flex items-center gap-2.5 rounded-full border px-4 py-2 text-sm font-medium transition-opacity hover:opacity-75 disabled:opacity-50"
        style={{ borderColor: 'var(--reader-rule)', color: 'var(--reader-link)' }}
      >
        {state === 'preparing' ? (
          <Loader2 className="size-4 animate-spin" aria-hidden />
        ) : state === 'playing' ? (
          <Pause className="size-4 fill-current" aria-hidden />
        ) : state === 'hasAudio' ? (
          <Play className="size-4 fill-current" aria-hidden />
        ) : (
          <Upload className="size-4" aria-hidden />
        )}
        {labels[state]}
      </button>

      {translation}

      {state === 'hasAudio' || state === 'playing' ? (
        <span className="text-xs tabular-nums" style={{ color: 'var(--reader-muted)' }}>
          {progress > 0.005
            ? `${Math.round(clamp(progress, 0, 1) * 100)}% played`
            : 'Not started'}
        </span>
      ) : (
        <span className="text-xs" style={{ color: 'var(--reader-muted)' }}>
          Import an audio file to listen along while you read.
        </span>
      )}
    </section>
  );
}

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
        className="grid size-12 place-items-center rounded-full"
        style={{ border: '1px solid var(--reader-rule)', color: 'var(--reader-muted)' }}
      >
        <Icon className={cn('size-5', busy && 'animate-spin')} aria-hidden />
      </span>
      <div>
        <h2 className="text-lg font-semibold" style={{ color: 'var(--reader-fg)' }}>
          {title}
        </h2>
        <p className="mx-auto mt-1.5 max-w-sm text-sm" style={{ color: 'var(--reader-muted)' }}>
          {message}
        </p>
      </div>
      {action}
      <Link
        to={`/book/${bookId}`}
        className="text-xs underline underline-offset-4 hover:opacity-75"
        style={{ color: 'var(--reader-muted)' }}
      >
        Back to book
      </Link>
    </div>
  );
}

function ReaderSkeleton({ columnWidth }: { columnWidth: number }) {
  return (
    <div className="px-5 py-14 sm:px-8" style={{ maxWidth: columnWidth, marginInline: 'auto' }}>
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
