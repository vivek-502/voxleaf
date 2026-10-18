/** App-wide constants: genres, playback speeds, accepted audio, nav. */

export const PLAYBACK_SPEEDS = [0.5, 0.75, 1, 1.25, 1.5, 1.75, 2, 2.5, 3] as const;

export const DEFAULT_SKIP_FORWARD = 30;
export const DEFAULT_SKIP_BACKWARD = 15;

/**
 * Store filters. `subject` is the query term handed to the metadata provider;
 * the `slug` is used for URLs and the `label` for the UI.
 */
export interface GenreFilter {
  slug: string;
  label: string;
  subject: string;
  icon?: string;
}

export const GENRE_FILTERS: GenreFilter[] = [
  { slug: 'all', label: 'All', subject: '' },
  { slug: 'fiction', label: 'Fiction', subject: 'fiction' },
  { slug: 'non-fiction', label: 'Non-fiction', subject: 'nonfiction' },
  { slug: 'biography', label: 'Biography', subject: 'biography' },
  { slug: 'business', label: 'Business', subject: 'business' },
  { slug: 'self-help', label: 'Self-help', subject: 'self_help' },
  { slug: 'history', label: 'History', subject: 'history' },
  { slug: 'science', label: 'Science', subject: 'science' },
  { slug: 'technology', label: 'Technology', subject: 'technology' },
  { slug: 'romance', label: 'Romance', subject: 'romance' },
  { slug: 'mystery', label: 'Mystery', subject: 'mystery' },
  { slug: 'fantasy', label: 'Fantasy', subject: 'fantasy' },
  { slug: 'thriller', label: 'Thriller', subject: 'thriller' },
  { slug: 'poetry', label: 'Poetry', subject: 'poetry' },
  { slug: 'children', label: 'Children', subject: 'juvenile_literature' },
  { slug: 'philosophy', label: 'Philosophy', subject: 'philosophy' },
  { slug: 'psychology', label: 'Psychology', subject: 'psychology' },
  { slug: 'travel', label: 'Travel', subject: 'travel' },
];

export const GENRE_BY_SLUG = new Map(GENRE_FILTERS.map((g) => [g.slug, g]));

/** Store sort options. */
export const STORE_SORTS = [
  { value: 'relevance', label: 'Relevance' },
  { value: 'newest', label: 'Newest' },
  { value: 'oldest', label: 'Oldest' },
  { value: 'title-asc', label: 'Title A–Z' },
] as const;

export type StoreSort = (typeof STORE_SORTS)[number]['value'];

/** Library sort options. */
export const LIBRARY_SORTS = [
  { value: 'recently-added', label: 'Recently added' },
  { value: 'recently-listened', label: 'Recently listened' },
  { value: 'title', label: 'Title' },
  { value: 'author', label: 'Author' },
  { value: 'progress', label: 'Progress' },
] as const;

export type LibrarySort = (typeof LIBRARY_SORTS)[number]['value'];

/** Library filters. */
export const LIBRARY_FILTERS = [
  { value: 'all', label: 'All' },
  { value: 'in-progress', label: 'In progress' },
  { value: 'completed', label: 'Completed' },
  { value: 'not-started', label: 'Not started' },
  { value: 'has-audio', label: 'Has audio' },
  { value: 'needs-audio', label: 'Needs audio' },
] as const;

export type LibraryFilter = (typeof LIBRARY_FILTERS)[number]['value'];

/** Audio containers the uploader accepts. */
export interface AudioFormat {
  extensions: string[];
  mimeTypes: string[];
  label: string;
}

export const AUDIO_FORMATS: AudioFormat[] = [
  { extensions: ['.mp3'], mimeTypes: ['audio/mpeg', 'audio/mp3'], label: 'MP3' },
  {
    extensions: ['.m4a', '.mp4', '.aac'],
    mimeTypes: ['audio/mp4', 'audio/x-m4a', 'audio/aac', 'audio/m4a', 'audio/x-aac'],
    label: 'M4A / AAC',
  },
  { extensions: ['.wav', '.wave'], mimeTypes: ['audio/wav', 'audio/x-wav', 'audio/wave'], label: 'WAV' },
  { extensions: ['.ogg', '.oga', '.opus'], mimeTypes: ['audio/ogg', 'application/ogg'], label: 'OGG / Opus' },
  { extensions: ['.flac'], mimeTypes: ['audio/flac', 'audio/x-flac'], label: 'FLAC' },
];

export const ACCEPTED_AUDIO_EXTENSIONS = AUDIO_FORMATS.flatMap((f) => f.extensions);

export const ACCEPTED_AUDIO_MIME = AUDIO_FORMATS.flatMap((f) => f.mimeTypes);

export const AUDIO_ACCEPT_ATTRIBUTE = [
  ...ACCEPTED_AUDIO_EXTENSIONS,
  ...ACCEPTED_AUDIO_MIME,
].join(',');

/** Formats the browser can natively decode. Used for client-side capability hints. */
export const NATIVELY_PLAYABLE_EXTENSIONS = ['.mp3', '.m4a', '.mp4', '.aac', '.wav', '.ogg', '.oga'];

/**
 * Accepted EPUB file names, for the manual upload's file picker.
 *
 * A hint only. An `.epub` is a zip, so the extension says nothing about whether
 * the bytes are a readable book -- the server parses it either way, and a file
 * with the wrong name but the right content is worth accepting rather than
 * making someone rename it first.
 */
export const EPUB_ACCEPT_ATTRIBUTE = '.epub,application/epub+zip,application/zip';

/**
 * Client-side size ceiling for a manual EPUB upload, in MB.
 *
 * Matches the server's `EPUB_MAX_SIZE_MB` default. Kept as a number rather than
 * read from an API response because it only exists to catch an obvious mistake
 * before a 200 MB file starts uploading; the server enforces the real limit.
 */
export const EPUB_MAX_SIZE_MB = 200;

export const SLEEP_TIMER_OPTIONS = [
  { value: 'off', label: 'Off' },
  { value: '5', label: '5 minutes' },
  { value: '10', label: '10 minutes' },
  { value: '15', label: '15 minutes' },
  { value: '30', label: '30 minutes' },
  { value: 'end-of-chapter', label: 'End of chapter' },
] as const;

export const SLEEP_TIMER_LABEL: Record<string, string> = {
  off: 'Off',
  '5': '5 min',
  '10': '10 min',
  '15': '15 min',
  '30': '30 min',
  'end-of-chapter': 'End of chapter',
};

export const PAGE_SIZE = 20;

/** Longest chapter / note text we accept, enforced on both client and server. */
export const MAX_CHAPTER_TITLE = 160;
export const MAX_BOOKMARK_NOTE = 2000;
export const MAX_CHAPTERS_PER_BOOK = 1000;

export interface TranslationLanguage {
  /** Code the translation provider is given, e.g. `pt-BR`. */
  code: string;
  /** English name, for the picker and for the label on a saved translation. */
  name: string;
  /** What the language is called in its own script, so it is recognisable. */
  native: string;
}

/**
 * Languages offered for translating a section.
 *
 * The provider understands well over a hundred codes; these are the ones worth
 * one click, and the picker accepts any other code the reader types, so the list
 * being short costs nothing. Ordered as a reader would choose: the languages of
 * the books in most libraries first, then the rest alphabetically.
 */
export const TRANSLATION_LANGUAGES: TranslationLanguage[] = [
  { code: 'es', name: 'Spanish', native: 'Español' },
  { code: 'pt-BR', name: 'Brazilian Portuguese', native: 'Português (Brasil)' },
  { code: 'pt', name: 'Portuguese', native: 'Português' },
  { code: 'fr', name: 'French', native: 'Français' },
  { code: 'de', name: 'German', native: 'Deutsch' },
  { code: 'it', name: 'Italian', native: 'Italiano' },
  { code: 'nl', name: 'Dutch', native: 'Nederlands' },
  { code: 'pl', name: 'Polish', native: 'Polski' },
  { code: 'ru', name: 'Russian', native: 'Русский' },
  { code: 'uk', name: 'Ukrainian', native: 'Українська' },
  { code: 'tr', name: 'Turkish', native: 'Türkçe' },
  { code: 'ar', name: 'Arabic', native: 'العربية' },
  { code: 'he', name: 'Hebrew', native: 'עברית' },
  { code: 'fa', name: 'Persian', native: 'فارسی' },
  { code: 'hi', name: 'Hindi', native: 'हिन्दी' },
  { code: 'bn', name: 'Bengali', native: 'বাংলা' },
  { code: 'ta', name: 'Tamil', native: 'தமிழ்' },
  { code: 'id', name: 'Indonesian', native: 'Bahasa Indonesia' },
  { code: 'ms', name: 'Malay', native: 'Bahasa Melayu' },
  { code: 'th', name: 'Thai', native: 'ไทย' },
  { code: 'vi', name: 'Vietnamese', native: 'Tiếng Việt' },
  { code: 'ja', name: 'Japanese', native: '日本語' },
  { code: 'ko', name: 'Korean', native: '한국어' },
  { code: 'zh-CN', name: 'Chinese (Simplified)', native: '简体中文' },
  { code: 'zh-TW', name: 'Chinese (Traditional)', native: '繁體中文' },
  { code: 'ca', name: 'Catalan', native: 'Català' },
  { code: 'el', name: 'Greek', native: 'Ελληνικά' },
  { code: 'sv', name: 'Swedish', native: 'Svenska' },
  { code: 'da', name: 'Danish', native: 'Dansk' },
  { code: 'fi', name: 'Finnish', native: 'Suomi' },
  { code: 'nb', name: 'Norwegian', native: 'Norsk' },
  { code: 'cs', name: 'Czech', native: 'Čeština' },
  { code: 'hu', name: 'Hungarian', native: 'Magyar' },
  { code: 'ro', name: 'Romanian', native: 'Română' },
  { code: 'bg', name: 'Bulgarian', native: 'Български' },
  { code: 'uk-Latn', name: 'Ukrainian (Latin)', native: 'Ukrainsʹka' },
  { code: 'sr', name: 'Serbian', native: 'Српски' },
  { code: 'sw', name: 'Swahili', native: 'Kiswahili' },
  { code: 'af', name: 'Afrikaans', native: 'Afrikaans' },
  { code: 'sl', name: 'Slovene', native: 'Slovenščina' },
];

const TRANSLATION_LANGUAGE_BY_CODE = new Map(TRANSLATION_LANGUAGES.map((l) => [l.code.toLowerCase(), l]));
/** The reader's name for a language code, falling back to the code itself. */
export function translationLanguageName(code: string): string {
  const language = TRANSLATION_LANGUAGE_BY_CODE.get(code.trim().toLowerCase());
  if (language) return language.name;
  return code.trim() || 'Unknown';
}

/**
 * Put a language code into the form BCP 47 gives it, which is also the form the
 * provider expects.
 *
 * Casing is not cosmetic here. `pt` and `pt-BR` are different languages — general
 * Portuguese and Brazilian Portuguese — and `zh-CN` and `zh-TW` name different
 * scripts, so lowercasing a code does not merely tidy it, it risks asking for a
 * language nobody wanted. Canonicalising to `pt-BR`, `zh-CN`, `zh-Hant` means a
 * language typed two ways is stored once and sent the same way both times.
 */
export function canonicalLanguageCode(code: string): string {
  return code
    .trim()
    .toLowerCase()
    .split('-')
    .map((part, index) => {
      if (index === 0) return part;
      // A script subtag is four letters, written in title case.
      if (part.length === 4) return part[0]!.toUpperCase() + part.slice(1);
      // A region subtag is two letters or three digits, written in capitals.
      if (/^[a-z]{2}$/.test(part) || /^[0-9]{3}$/.test(part)) return part.toUpperCase();
      return part;
    })
    .join('-');
}