/**
 * Domain types shared by client and server.
 *
 * These mirror the Prisma models but stay free of generated-client imports so
 * they can be used inside `'use client'` components without dragging the
 * Prisma engine into the browser bundle.
 */

/** A catalog book as returned by the book metadata provider. */
export interface BookSummary {
  id: string;
  externalId: string;
  title: string;
  subtitle: string | null;
  author: string;
  description: string | null;
  coverUrl: string | null;
  publisher: string | null;
  publicationDate: string | null;
  isbn: string | null;
  categories: string[];
  pageCount: number | null;
  language: string | null;
  rating: number | null;
  ratingCount: number | null;
  /** Whether the book is already in the user's library. */
  inLibrary: boolean;
  favorite: boolean;
  /** Internal catalogue id when known, so pages can link straight through. */
  libraryId: string | null;
}

export interface BookSearchResult {
  books: BookSummary[];
  total: number;
  page: number;
  pageSize: number;
  hasMore: boolean;
  /** True when the provider was unreachable and local results were served. */
  degraded: boolean;
  provider: string;
}

export interface ChapterAudioInfo {
  id: string;
  chapterId: string;
  /** Signed, expiring stream URL. Never a raw storage path. */
  streamUrl: string;
  duration: number;
  fileSize: number;
  mimeType: string;
  originalName: string | null;
  uploadedAt: string;
}

export interface ChapterInfo {
  id: string;
  bookId: string;
  chapterNumber: number;
  title: string;
  sortIndex: number;
  audio: ChapterAudioInfo | null;
  progress: ChapterProgressInfo | null;
  bookmarkCount: number;
  createdAt: string;
  updatedAt: string;
}

export interface ChapterProgressInfo {
  position: number;
  duration: number;
  percentage: number;
  completed: boolean;
  playCount: number;
  updatedAt: string;
}

/** A book in the user's library, with its chapters and progress rollup. */
export interface LibraryBookInfo {
  libraryId: string;
  bookId: string;
  addedAt: string;
  favorite: boolean;
  overallProgress: number;
  completed: boolean;
  lastPlayedAt: string | null;
  lastPlayedChapterId: string | null;
  lastChapterTitle: string | null;
  lastChapterNumber: number | null;
  lastPosition: number;
  totalAudioDuration: number;
  chapterCount: number;
  chaptersWithAudio: number;
  book: BookSummary;
}

export interface BookmarkInfo {
  id: string;
  chapterId: string;
  bookId: string;
  bookTitle: string;
  chapterTitle: string;
  chapterNumber: number;
  timestamp: number;
  note: string | null;
  createdAt: string;
}

export interface AppSettingsInfo {
  playbackSpeed: number;
  theme: 'dark' | 'light' | 'system';
  volume: number;
  muted: boolean;
  sleepTimerPreference: SleepTimerId;
  skipForwardSeconds: number;
  skipBackwardSeconds: number;
  completionThreshold: number;
  autoAdvance: boolean;
  bookmarkNote?: string;
  ttsVoice: string;
}

/** The single track the audio engine is currently bound to. */
export interface PlaybackTrack {
  bookId: string;
  libraryId: string;
  chapterId: string;
  audioId: string;
  bookTitle: string;
  chapterTitle: string;
  chapterNumber: number;
  coverUrl: string | null;
  author: string;
  streamUrl: string;
  /** Resumed position, in seconds. */
  startPosition: number;
  duration: number;
  favorite: boolean;
}

export const SLEEP_TIMER_IDS = [
  'off',
  '5',
  '10',
  '15',
  '30',
  'end-of-chapter',
] as const;

export type SleepTimerId = (typeof SLEEP_TIMER_IDS)[number];
export type ThemeId = AppSettingsInfo['theme'];

/* -------------------------------------------------------------------------- */
/* EPUB                                                                       */
/* -------------------------------------------------------------------------- */

/** State of a book's stored EPUB. */
export type EpubStatusKind =
  /** No usable file yet: nothing uploaded, or a rejected upload. */
  | 'none'
  /** Stored, parsed and ready to read. */
  | 'ready'
  /** A file was supplied but could not be used. */
  | 'failed'
  /** EPUB support is switched off on this instance. */
  | 'disabled';

export interface EpubStatusInfo {
  hasEpub: boolean;
  status: EpubStatusKind | null;
  error: string | null;
  fileName: string | null;
  fileSize: number | null;
  chapterCount: number;
  imageCount: number;
  title: string | null;
  creator: string | null;
  language: string | null;
  enabled?: boolean;
}

/** A voice offered by Edge TTS. */
export interface TtsVoice {
  /** Short name passed to the voice service, e.g. `en-US-GuyNeural`. */
  name: string;
  locale: string;
  gender: string;
}

/**
 * Progress of one chapter's narration.
 *
 * Generation is a background job, not a request: it takes minutes, so the client
 * starts it and polls this until `state` is no longer `running`.
 */
export interface TtsJobStatus {
  chapterId: string;
  state: 'running' | 'done' | 'failed';
  /** Parts finished so far. A long chapter is synthesised in several. */
  completed: number;
  total: number;
  error: string | null;
  audioId: string | null;
  streamUrl: string | null;
  /** What is being narrated. Null until the source is resolved. */
  narration: TtsNarrationInfo | null;
}

/**
 * Which language a generation settled on, and the voice reading it.
 *
 * Reported rather than assumed, because a chapter the reader is reading in Hindi
 * is narrated in Hindi — and because a translation that turned out to be missing
 * or stale is narrated in the original instead, which the reader needs to know.
 */
export interface TtsNarrationInfo {
  /** Provider language code, or null when narrating the original. */
  language: string | null;
  languageName: string | null;
  voice: string;
  /** A translation was asked for but the original was narrated instead. */
  fellBackToOriginal: boolean;
  /** Why the original was used, or why a different voice was chosen. */
  reason: string | null;
}

/** A reading-order document from the EPUB spine. */
export interface EpubChapterEntry {
  id: string;
  index: number;
  title: string;
  /** Nesting depth from the table of contents, 0 for a flat spine. */
  depth: number;
  inToc: boolean;
  wordCount: number;
  charOffset: number;
}

/**
 * One row of the book's own table of contents.
 *
 * This is the list the reader navigates by, and it is deliberately not the list
 * of documents: a book's contents can name several places inside one document,
 * and can leave documents out entirely.
 */
export interface EpubContentsEntry {
  /** Reading-order chapter the row opens, matching `EpubChapterEntry.index`. */
  index: number;
  /** Anchor id within the chapter, empty when the row has no fragment. */
  anchor: string;
  /** Visible-text character offset inside the chapter. */
  offset: number;
  title: string;
  /** Nesting depth from the table of contents, 0 for a flat one. */
  depth: number;
}

export interface EpubChapterPayload {
  chapter: {
    index: number;
    title: string;
    /** Sanitised server-side at import time. */
    html: string;
    wordCount: number;
    depth: number;
  };
  chapters: EpubChapterEntry[];
  contents: EpubContentsEntry[];
  meta: {
    title: string | null;
    creator: string | null;
    language: string | null;
    chapterCount: number;
  };
}

/** Audio state for one reading-order document. */
export interface EpubAudioEntry {
  /** Spine index, matching `EpubChapterEntry.index`. */
  index: number;
  /** Library chapter carrying the audio, or null when nothing is attached. */
  chapterId: string | null;
  hasAudio: boolean;
  audioId: string | null;
  /** Seconds of audio, 0 when nothing is attached. */
  duration: number;
  /** 0..1 through the chapter, 0 when never played. */
  progress: number;
  completed: boolean;
}

/** One block of a translated section: the source block's tag, and its new text. */
export interface TranslationBlock {
  tag: string;
  text: string;
  /** Original HTML attributes for non-text blocks (e.g. img src/alt, table attrs). */
  html?: string;
  /** For table cells, colspan/rowspan info. */
  colspan?: number;
  rowspan?: number;
  /** Table structure metadata. */
  isHeader?: boolean;
}

/**
 * A saved translation of one section.
 *
 * `blocks` is text, never markup — a translation is rendered as React elements,
 * so no translated string is ever trusted as HTML.
 */
export interface EpubTranslationEntry {
  /** Reading-order index of the translated section. */
  epubIndex: number;
  language: string;
  languageName: string;
  title: string;
  blocks: TranslationBlock[];
  charCount: number;
  /** True when the section's text changed since the translation was made. */
  stale: boolean;
  updatedAt: string;
}

/** A saved translation without its text — enough to offer it in a picker. */
export interface EpubTranslationSummary {
  epubIndex: number;
  language: string;
  languageName: string;
  title: string;
  charCount: number;
  stale: boolean;
  updatedAt: string;
}

/** How a translation job is going, for a long section. */
export interface TranslationProgress {
  /** Characters translated so far. */
  done: number;
  /** Characters in the whole section. */
  total: number;
}

export type EpubTheme = 'paper' | 'sepia' | 'night' | 'ink';
export type EpubFont = 'serif' | 'sans' | 'mono';

export interface EpubProgressInfo {
  bookId: string;
  chapterIndex: number;
  scrollPercent: number;
  fontSize: number;
  lineHeight: number;
  theme: EpubTheme;
  fontFamily: EpubFont;
  columnWidth: number;
}
