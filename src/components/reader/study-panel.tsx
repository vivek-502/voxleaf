'use client';

import * as React from 'react';
import {
  Bookmark,
  BookmarkMinus,
  BookmarkPlus,
  BookOpen,
  Highlighter,
  Loader2,
  Pencil,
  Search,
  Trash2,
  Volume2,
  X,
} from 'lucide-react';

import type { ReaderSearchState } from './use-reader-search';
import { useSpeak } from '@/hooks/use-speak';
import { cn } from '@/utils/cn';
import { translationLanguageName } from '@/utils/constants';
import type {
  AnnotationColor,
  EpubAnnotation,
  EpubBookmarkInfo,
  EpubSearchResult,
  SavedWord,
} from '@/types';

export type StudyTab = 'search' | 'highlights' | 'bookmarks' | 'words';

const TABS: { value: StudyTab; label: string; icon: typeof Search }[] = [
  { value: 'search', label: 'Search', icon: Search },
  { value: 'highlights', label: 'Marks', icon: Highlighter },
  { value: 'bookmarks', label: 'Places', icon: Bookmark },
  { value: 'words', label: 'Words', icon: BookOpen },
];

const COLOR_HEX: Record<AnnotationColor, string> = {
  yellow: '#f2c14e',
  green: '#7fb069',
  blue: '#6a9fd8',
  pink: '#e58fa8',
  violet: '#a48bd4',
};

export interface StudyPanelProps {
  open: boolean;
  /** Room beside the text; when there is not, the panel covers it. */
  docked: boolean;
  tab: StudyTab;
  onTabChange: (tab: StudyTab) => void;
  onClose: () => void;

  search: ReaderSearchState;
  onOpenResult: (result: EpubSearchResult) => void;
  /** Language the results will be read in, for the summary line. */
  readingLanguageName: string | null;

  annotations: EpubAnnotation[];
  onOpenAnnotation: (annotation: EpubAnnotation) => void;
  onEditAnnotation: (annotation: EpubAnnotation) => void;
  onDeleteAnnotation: (id: string) => void;

  bookmarks: EpubBookmarkInfo[];
  onOpenBookmark: (bookmark: EpubBookmarkInfo) => void;
  onDeleteBookmark: (id: string) => void;
  onToggleBookmark: () => void;
  bookmarkedHere: boolean;

  words: SavedWord[];
  onLookupWord: (word: string, language: string) => void;
  onDeleteWord: (id: string) => void;
}

/**
 * Everything the reader has marked, searched or looked up, in one drawer.
 *
 * Foliate's sidebar, adapted to the web: a whole-book search grouped by chapter,
 * every highlight and note, every saved place, and the word notebook. The tabs
 * are the same four questions, so one panel is enough — an entry point per
 * feature would leave four buttons in a header that has room for none.
 *
 * The panel reads from the same state the page does. Opening a result is a jump,
 * not a second copy of the book, and deleting a highlight here removes it from
 * the page behind the drawer in the same frame.
 */
export function StudyPanel({
  open,
  docked,
  tab,
  onTabChange,
  onClose,
  search,
  onOpenResult,
  readingLanguageName,
  annotations,
  onOpenAnnotation,
  onEditAnnotation,
  onDeleteAnnotation,
  bookmarks,
  onOpenBookmark,
  onDeleteBookmark,
  onToggleBookmark,
  bookmarkedHere,
  words,
  onLookupWord,
  onDeleteWord,
}: StudyPanelProps) {
  const { speak } = useSpeak();
  const searchRef = React.useRef<HTMLInputElement>(null);

  // Focusing the field is the point of opening the panel on the search tab; a
  // reader who asked to search should not have to click again to type.
  React.useEffect(() => {
    if (!open || tab !== 'search') return;
    const frame = requestAnimationFrame(() => searchRef.current?.focus());
    return () => cancelAnimationFrame(frame);
  }, [open, tab]);

  React.useEffect(() => {
    if (!open) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      // Radix dialogs own Escape while they are open, and stop there.
      if (document.querySelector('[role="dialog"][data-state="open"]')) return;
      onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose, open]);

  if (!open) return null;

  const noteCount = annotations.filter((annotation) => annotation.note).length;
  const counts: Record<StudyTab, number> = {
    search: search.total,
    highlights: annotations.length,
    bookmarks: bookmarks.length,
    words: words.length,
  };

  const panel = (
    <aside
      aria-label="Search, highlights, places and words"
      className={cn(
        'absolute inset-y-0 right-0 z-[60] flex w-[min(26rem,92vw)] flex-col border-l shadow-[-18px_0_44px_-24px_rgb(0_0_0/0.55)]',
        docked ? 'animate-[contents-fade_180ms_var(--ease-out-quint)]' : 'animate-[study-slide_220ms_var(--ease-out-quint)]',
      )}
      style={{
        background: 'var(--reader-chrome)',
        backdropFilter: 'blur(20px) saturate(145%)',
        WebkitBackdropFilter: 'blur(20px) saturate(145%)',
        borderColor: 'var(--reader-rule)',
        color: 'var(--reader-fg)',
      }}
    >
      <header className="shrink-0 px-4 pt-4 pb-0">
        <div className="flex items-start justify-between gap-2">
          <div>
            <h2
              className="font-mono text-[9.5px] font-medium uppercase tracking-[0.16em]"
              style={{ color: 'var(--reader-muted)' }}
            >
              Study
            </h2>
            <p className="mt-1 font-display text-[17px] font-medium leading-tight">
              What you have marked
            </p>
          </div>
          <button
            type="button"
            onClick={onClose}
            aria-label="Close study panel"
            className="grid size-8 shrink-0 place-items-center transition-opacity hover:opacity-65"
            style={{ color: 'var(--reader-muted)' }}
          >
            <X className="size-4" aria-hidden />
          </button>
        </div>

        <div
          role="tablist"
          aria-label="Study sections"
          className="mt-3 grid grid-cols-4 gap-px"
          style={{ background: 'var(--reader-rule)' }}
        >
          {TABS.map((item) => {
            const Icon = item.icon;
            const active = item.value === tab;
            return (
              <button
                key={item.value}
                type="button"
                role="tab"
                id={`study-tab-${item.value}`}
                aria-controls={`study-tabpanel-${item.value}`}
                aria-selected={active}
                tabIndex={active ? 0 : -1}
                onClick={() => onTabChange(item.value)}
                onKeyDown={(event) => {
                  const step =
                    event.key === 'ArrowRight' ? 1 : event.key === 'ArrowLeft' ? -1 : 0;
                  if (step === 0 && event.key !== 'Home' && event.key !== 'End') return;
                  event.preventDefault();
                  const index = TABS.findIndex((entry) => entry.value === item.value);
                  const next =
                    event.key === 'Home'
                      ? 0
                      : event.key === 'End'
                        ? TABS.length - 1
                        : (index + step + TABS.length) % TABS.length;
                  onTabChange(TABS[next].value);
                  document.getElementById(`study-tab-${TABS[next].value}`)?.focus();
                }}
                className="flex flex-col items-center gap-1 px-1 py-2 transition-colors"
                style={{
                  background: active
                    ? 'color-mix(in srgb, var(--reader-link) 12%, var(--reader-chrome))'
                    : 'var(--reader-chrome)',
                  color: active ? 'var(--reader-link)' : 'var(--reader-muted)',
                }}
              >
                <Icon className="size-3.5" aria-hidden />
                <span className="font-mono text-[9px] uppercase tracking-[0.1em]">
                  {item.label}
                  {counts[item.value] > 0 ? ` ${counts[item.value]}` : ''}
                </span>
              </button>
            );
          })}
        </div>
      </header>

      <div className="h-px shrink-0" style={{ background: 'var(--reader-rule)' }} aria-hidden />

      {tab === 'search' ? (
        <div
          role="tabpanel"
          id="study-tabpanel-search"
          aria-labelledby="study-tab-search"
          tabIndex={-1}
          className="flex min-h-0 flex-1 flex-col"
        >
          <div className="shrink-0 px-4 pt-3 pb-2">
            <div
              className="flex items-center gap-2 border px-2.5"
              style={{
                borderColor: 'var(--reader-rule)',
                background: 'color-mix(in srgb, var(--reader-fg) 4%, transparent)',
              }}
            >
              <Search className="size-3.5 shrink-0" style={{ color: 'var(--reader-muted)' }} aria-hidden />
              <input
                ref={searchRef}
                type="search"
                autoFocus
                value={search.query}
                onChange={(event) => search.setQuery(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === 'Enter') {
                    event.preventDefault();
                    search.run();
                  }
                }}
                placeholder="Search the whole book"
                aria-label="Search the whole book"
                className="h-9 min-w-0 flex-1 bg-transparent text-sm outline-none placeholder:opacity-55"
                style={{ color: 'var(--reader-fg)' }}
              />
              {search.loading ? (
                <Loader2 className="size-3.5 shrink-0 animate-spin" aria-hidden style={{ color: 'var(--reader-muted)' }} />
              ) : search.query ? (
                <button
                  type="button"
                  onClick={search.reset}
                  aria-label="Clear search"
                  className="shrink-0 p-0.5 hover:opacity-70"
                  style={{ color: 'var(--reader-muted)' }}
                >
                  <X className="size-3.5" aria-hidden />
                </button>
              ) : null}
            </div>

            <div className="mt-2 flex flex-wrap items-center gap-1.5">
              <OptionChip
                active={search.options.caseSensitive}
                onClick={() => search.toggleOption('caseSensitive')}
                label="Case"
                hint="Match the exact capitalisation"
              >
                Aa
              </OptionChip>
              <OptionChip
                active={search.options.matchDiacritics}
                onClick={() => search.toggleOption('matchDiacritics')}
                label="Accents"
                hint="Treat accented letters as different"
              >
                Á
              </OptionChip>
              <OptionChip
                active={search.options.wholeWord}
                onClick={() => search.toggleOption('wholeWord')}
                label="Words"
                hint="Match whole words only"
              >
                ␣
              </OptionChip>
              <span className="ml-auto font-mono text-[9px] uppercase tracking-[0.1em]" style={{ color: 'var(--reader-muted)' }}>
                {readingLanguageName ? `in ${readingLanguageName}` : 'original text'}
              </span>
            </div>

            {search.query.trim().length >= 2 && !search.loading && !search.error ? (
              <p className="mt-2 text-[11px]" style={{ color: 'var(--reader-muted)' }} role="status">
                {search.total === 0
                  ? `No matches in ${search.chaptersScanned} sections.`
                  : `${search.total} ${search.total === 1 ? 'match' : 'matches'} · ${search.chaptersScanned} ${
                      search.chaptersScanned === 1 ? 'section' : 'sections'
                    } scanned`}
                {search.truncated ? ' · showing the first few' : ''}
              </p>
            ) : null}

            {search.error ? (
              <p className="mt-2 text-[11px]" style={{ color: '#c0492f' }} role="alert">
                {search.error}
              </p>
            ) : null}
          </div>

          <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain pb-6">
            {search.query.trim().length < 2 ? (
              <EmptyState
                icon={Search}
                title="Search every section"
                body="Words are found in the text itself — including translations — not just in the chapter list."
              />
            ) : search.results.length === 0 && !search.loading && !search.error ? (
              <EmptyState
                icon={Search}
                title={`Nothing matches “${search.query.trim()}”`}
                body="Try fewer words, or turn off whole-word matching."
              />
            ) : (
              <SearchGroups results={search.results} onOpenResult={onOpenResult} />
            )}
          </div>
        </div>
      ) : null}

      {tab === 'highlights' ? (
        <div
          role="tabpanel"
          id="study-tabpanel-highlights"
          aria-labelledby="study-tab-highlights"
          tabIndex={-1}
          className="min-h-0 flex-1 overflow-y-auto overscroll-contain pb-6"
        >
          {annotations.length === 0 ? (
            <EmptyState
              icon={Highlighter}
              title="No highlights yet"
              body="Select a passage and pick a colour. Marks stay with the paragraph when the section is read in another language."
            />
          ) : (
            <>
              <p className="px-4 pt-3 pb-1 font-mono text-[9.5px] uppercase tracking-[0.12em]" style={{ color: 'var(--reader-muted)' }}>
                {annotations.length} {annotations.length === 1 ? 'mark' : 'marks'}
                {noteCount > 0 ? ` · ${noteCount} with notes` : ''}
              </p>
              {groupAnnotations(annotations).map((group) => (
                <section key={group.chapterIndex}>
                  <h3
                    className="sticky top-0 z-10 border-y px-4 py-1.5 font-mono text-[9px] uppercase tracking-[0.12em]"
                    style={{
                      borderColor: 'var(--reader-rule)',
                      background: 'var(--reader-chrome)',
                      color: 'var(--reader-muted)',
                    }}
                  >
                    {group.title}
                  </h3>
                  <ul>
                    {group.items.map((annotation) => (
                      <li key={annotation.id} className="group relative border-b" style={{ borderColor: 'var(--reader-rule)' }}>
                        <button
                          type="button"
                          onClick={() => onOpenAnnotation(annotation)}
                          className="block w-full px-4 py-3 pr-14 text-left"
                        >
                          <span className="flex items-center gap-1.5">
                            <span
                              aria-hidden
                              className="size-2 shrink-0 rounded-full"
                              style={{ background: COLOR_HEX[annotation.color] }}
                            />
                            <span className="truncate font-mono text-[9px] uppercase tracking-[0.1em]" style={{ color: 'var(--reader-muted)' }}>
                              {annotation.chapterTitle}
                              {annotation.language ? ` · ${translationLanguageName(annotation.language) || annotation.language}` : ''}
                            </span>
                          </span>
                          <span className="mt-1.5 block text-[13px] leading-relaxed" style={{ color: 'var(--reader-fg)' }}>
                            “{truncate(annotation.selectedText || annotation.quote, 190)}”
                          </span>
                          {annotation.note ? (
                            <span className="mt-1.5 flex items-start gap-1.5 text-[12px] leading-relaxed" style={{ color: 'var(--reader-muted)' }}>
                              <Pencil className="mt-0.5 size-3 shrink-0" aria-hidden />
                              {truncate(annotation.note, 160)}
                            </span>
                          ) : null}
                        </button>
                        <div className="absolute right-2 top-2 flex gap-0.5 opacity-0 transition-opacity focus-within:opacity-100 group-hover:opacity-100">
                          <IconButton label="Edit note" onClick={() => onEditAnnotation(annotation)}>
                            <Pencil className="size-3.5" aria-hidden />
                          </IconButton>
                          <IconButton label="Delete highlight" onClick={() => onDeleteAnnotation(annotation.id)} danger>
                            <Trash2 className="size-3.5" aria-hidden />
                          </IconButton>
                        </div>
                      </li>
                    ))}
                  </ul>
                </section>
              ))}
            </>
          )}
        </div>
      ) : null}

      {tab === 'bookmarks' ? (
        <div
          role="tabpanel"
          id="study-tabpanel-bookmarks"
          aria-labelledby="study-tab-bookmarks"
          tabIndex={-1}
          className="min-h-0 flex-1 overflow-y-auto overscroll-contain pb-6"
        >
          <div className="px-4 pt-3">
            <button
              type="button"
              onClick={onToggleBookmark}
              className="flex w-full items-center justify-center gap-2 border px-3 py-2 font-mono text-[10px] uppercase tracking-[0.08em] transition-opacity hover:opacity-75"
              style={{ borderColor: 'var(--reader-rule)', color: 'var(--reader-link)' }}
            >
              {bookmarkedHere ? (
                <BookmarkMinus className="size-4" aria-hidden />
              ) : (
                <BookmarkPlus className="size-4" aria-hidden />
              )}
              {bookmarkedHere ? 'Remove the bookmark here' : 'Bookmark this page'}
            </button>
          </div>

          {bookmarks.length === 0 ? (
            <EmptyState
              icon={Bookmark}
              title="No saved places"
              body="A bookmark remembers the section and how far down it you were, so you can come back later."
            />
          ) : (
            <ul className="mt-2">
              {bookmarks.map((bookmark) => (
                <li key={bookmark.id} className="group relative border-b" style={{ borderColor: 'var(--reader-rule)' }}>
                  <button
                    type="button"
                    onClick={() => onOpenBookmark(bookmark)}
                    className="block w-full px-4 py-3 pr-12 text-left"
                  >
                    <span className="flex items-baseline justify-between gap-2">
                      <span className="truncate text-[13.5px] leading-snug" style={{ color: 'var(--reader-fg)' }}>
                        {bookmark.label || bookmark.chapterTitle}
                      </span>
                      <span className="shrink-0 font-mono text-[9.5px] tabular-nums" style={{ color: 'var(--reader-muted)' }}>
                        {Math.round(bookmark.scrollPercent * 100)}%
                      </span>
                    </span>
                    <span className="mt-1 block truncate font-mono text-[9px] uppercase tracking-[0.1em]" style={{ color: 'var(--reader-muted)' }}>
                      {bookmark.label ? `${bookmark.chapterTitle} · ` : ''}
                      {formatDate(bookmark.createdAt)}
                    </span>
                    <span className="mt-2 block h-px w-full" style={{ background: 'var(--reader-rule)' }} aria-hidden>
                      <span
                        className="block h-px"
                        style={{
                          width: `${Math.round(bookmark.scrollPercent * 100)}%`,
                          background: 'var(--reader-link)',
                        }}
                      />
                    </span>
                  </button>
                  <div className="absolute right-2 top-2 opacity-0 transition-opacity focus-within:opacity-100 group-hover:opacity-100">
                    <IconButton label="Delete bookmark" onClick={() => onDeleteBookmark(bookmark.id)} danger>
                      <Trash2 className="size-3.5" aria-hidden />
                    </IconButton>
                  </div>
                </li>
              ))}
            </ul>
          )}
        </div>
      ) : null}

      {tab === 'words' ? (
        <div
          role="tabpanel"
          id="study-tabpanel-words"
          aria-labelledby="study-tab-words"
          tabIndex={-1}
          className="min-h-0 flex-1 overflow-y-auto overscroll-contain pb-6"
        >
          {words.length === 0 ? (
            <EmptyState
              icon={BookOpen}
              title="No saved words"
              body="Select a word and choose Define. Save the ones worth keeping and they collect here."
            />
          ) : (
            <>
              <p className="px-4 pt-3 pb-1 font-mono text-[9.5px] uppercase tracking-[0.12em]" style={{ color: 'var(--reader-muted)' }}>
                {words.length} {words.length === 1 ? 'word' : 'words'}
              </p>
              <ul>
                {words.map((word) => (
                  <li key={word.id} className="group relative border-b" style={{ borderColor: 'var(--reader-rule)' }}>
                    <button
                      type="button"
                      onClick={() => onLookupWord(word.word, word.language)}
                      className="block w-full px-4 py-3 pr-20 text-left"
                    >
                      <span className="flex flex-wrap items-baseline gap-x-2 gap-y-0.5">
                        <span className="font-display text-[17px] leading-none" style={{ color: 'var(--reader-fg)' }}>
                          {word.word}
                        </span>
                        {word.phonetic ? (
                          <span className="font-mono text-[10px]" style={{ color: 'var(--reader-muted)' }}>
                            {word.phonetic}
                          </span>
                        ) : null}
                        <span className="ml-auto font-mono text-[9px] uppercase tracking-[0.1em]" style={{ color: 'var(--reader-muted)' }}>
                          {translationLanguageName(word.language) || word.language}
                        </span>
                      </span>
                      {word.senses[0]?.definitions[0]?.definition ? (
                        <span className="mt-1 block text-[12px] leading-relaxed" style={{ color: 'var(--reader-muted)' }}>
                          {truncate(word.senses[0].definitions[0].definition, 150)}
                        </span>
                      ) : null}
                    </button>
                    <div className="absolute right-2 top-2 flex gap-0.5 opacity-0 transition-opacity focus-within:opacity-100 group-hover:opacity-100">
                      <IconButton label={`Say ${word.word}`} onClick={() => speak(word.word, word.language)}>

                        <Volume2 className="size-3.5" aria-hidden />
                      </IconButton>
                      <IconButton label="Remove word" onClick={() => onDeleteWord(word.id)} danger>
                        <Trash2 className="size-3.5" aria-hidden />
                      </IconButton>
                    </div>
                  </li>
                ))}
              </ul>
            </>
          )}
        </div>
      ) : null}
    </aside>
  );

  if (docked) return panel;

  return (
    <>
      <button
        type="button"
        aria-label="Close study panel"
        tabIndex={-1}
        onClick={onClose}
        className="absolute inset-0 z-[59] animate-[scrim-enter_160ms_ease-out] bg-black/45"
      />
      {panel}
    </>
  );
}

/* -------------------------------------------------------------------------- */
/* Pieces                                                                      */
/* -------------------------------------------------------------------------- */

function SearchGroups({
  results,
  onOpenResult,
}: {
  results: EpubSearchResult[];
  onOpenResult: (result: EpubSearchResult) => void;
}) {
  const groups = React.useMemo(() => {
    const map = new Map<number, { title: string; items: EpubSearchResult[] }>();
    for (const result of results) {
      const group = map.get(result.chapterIndex);
      if (group) group.items.push(result);
      else map.set(result.chapterIndex, { title: result.chapterTitle, items: [result] });
    }
    return [...map.entries()].sort((a, b) => a[0] - b[0]);
  }, [results]);

  return (
    <>
      {groups.map(([index, group]) => (
        <section key={index}>
          <h3
            className="sticky top-0 z-10 flex items-baseline justify-between gap-2 border-y px-4 py-1.5 font-mono text-[9px] uppercase tracking-[0.12em]"
            style={{
              borderColor: 'var(--reader-rule)',
              background: 'var(--reader-chrome)',
              color: 'var(--reader-muted)',
            }}
          >
            <span className="truncate">{group.title}</span>
            <span className="shrink-0 tabular-nums">{group.items.length}</span>
          </h3>
          <ul>
            {group.items.map((result) => (
              <li key={result.id} className="border-b" style={{ borderColor: 'var(--reader-rule)' }}>
                <button
                  type="button"
                  onClick={() => onOpenResult(result)}
                  className="block w-full px-4 py-2.5 text-left transition-colors hover:bg-[color-mix(in_srgb,var(--reader-fg)_5%,transparent)]"
                >
                  <span className="block text-[13px] leading-relaxed" style={{ color: 'var(--reader-fg)' }}>
                    {result.excerpt.before}
                    <mark
                      className="px-0.5"
                      style={{ background: 'color-mix(in srgb, var(--reader-link) 28%, transparent)', color: 'inherit' }}
                    >
                      {result.excerpt.match}
                    </mark>
                    {result.excerpt.after}
                  </span>
                  {result.source === 'translation' && result.languageName ? (
                    <span className="mt-0.5 block font-mono text-[9px] uppercase tracking-[0.1em]" style={{ color: 'var(--reader-muted)' }}>
                      {result.languageName}
                    </span>
                  ) : null}
                </button>
              </li>
            ))}
          </ul>
        </section>
      ))}
    </>
  );
}

function OptionChip({
  active,
  onClick,
  label,
  hint,
  children,
}: {
  active: boolean;
  onClick: () => void;
  label: string;
  hint: string;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-pressed={active}
      title={hint}
      className="flex items-center gap-1.5 border px-2 py-1 font-mono text-[10px] transition-colors"
      style={{
        borderColor: active ? 'var(--reader-link)' : 'var(--reader-rule)',
        color: active ? 'var(--reader-link)' : 'var(--reader-muted)',
        background: active ? 'color-mix(in srgb, var(--reader-link) 10%, transparent)' : 'transparent',
      }}
    >
      <span aria-hidden>{children}</span>
      {label}
    </button>
  );
}

function EmptyState({
  icon: Icon,
  title,
  body,
}: {
  icon: typeof Search;
  title: string;
  body: string;
}) {
  return (
    <div className="flex flex-col items-center px-8 py-12 text-center">
      <span
        className="grid size-11 place-items-center border"
        style={{ borderColor: 'var(--reader-rule)', color: 'var(--reader-muted)' }}
      >
        <Icon className="size-4" aria-hidden />
      </span>
      <p className="mt-3 font-display text-[15px] font-medium" style={{ color: 'var(--reader-fg)' }}>
        {title}
      </p>
      <p className="mt-1.5 max-w-[17rem] text-[12.5px] leading-relaxed" style={{ color: 'var(--reader-muted)' }}>
        {body}
      </p>
    </div>
  );
}

function IconButton({
  label,
  onClick,
  danger,
  children,
}: {
  label: string;
  onClick: () => void;
  danger?: boolean;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-label={label}
      title={label}
      className="grid size-7 place-items-center transition-opacity hover:opacity-65 focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-[var(--reader-link)]"
      style={{ color: danger ? '#c0492f' : 'var(--reader-fg)' }}
    >
      {children}
    </button>
  );
}

function groupAnnotations(annotations: EpubAnnotation[]) {
  const map = new Map<number, { chapterIndex: number; title: string; items: EpubAnnotation[] }>();
  for (const annotation of annotations) {
    const group = map.get(annotation.chapterIndex);
    if (group) group.items.push(annotation);
    else map.set(annotation.chapterIndex, { chapterIndex: annotation.chapterIndex, title: annotation.chapterTitle, items: [annotation] });
  }
  return [...map.values()].sort((a, b) => a.chapterIndex - b.chapterIndex);
}

function truncate(text: string, limit: number): string {
  const clean = text.replace(/\s+/g, ' ').trim();
  return clean.length > limit ? `${clean.slice(0, limit - 1).trimEnd()}…` : clean;
}

function formatDate(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return '';
  return date.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
}

