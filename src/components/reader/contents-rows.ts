/**
 * Turning the book's table of contents into the rows the reader navigates by.
 *
 * The server merges the spine with the book's own TOC, so a row is a place in
 * the text rather than a file: `index` says which section, `offset` how far into
 * it. A single section can hold several rows, which is why the row — not the
 * section — is the unit of selection, highlighting and audio.
 *
 * Pure, and independent of React, so the selection and highlight rules can be
 * tested without a browser.
 */

import type { EpubChapterEntry, EpubContentsEntry } from '@/types';

export interface ContentsRow {
  /** Section index, matching `EpubChapterEntry.index`. */
  index: number;
  /** Visible-text offset inside that section. */
  offset: number;
  title: string;
  /** Nesting depth from the book's TOC, 0 for a flat one. */
  depth: number;
  /** First row of its section, which is where the audio control belongs. */
  leads: boolean;
  /** Stable across re-renders and usable as a DOM key and a highlight key. */
  key: string;
}

export function buildContentsRows(
  contents: readonly EpubContentsEntry[] | undefined,
  chapters: readonly EpubChapterEntry[],
): ContentsRow[] {
  // No parsed contents — a book stored before the reader grew them, or one with
  // no TOC at all. One row per section still navigates the whole book.
  if (!contents || contents.length === 0) {
    return chapters.map((chapter) => ({
      index: chapter.index,
      offset: 0,
      title: chapter.title,
      depth: chapter.depth,
      leads: true,
      key: `${chapter.index}@0`,
    }));
  }

  let last = -1;
  // A place in the text is not an identity. Two rows can sit on the same spot —
  // a part heading and the chapter that opens it — and the parser only separates
  // a run of three or more, leaving pairs sharing an offset. Keying on position
  // alone then gives React duplicate keys among siblings, and React answers that
  // by reusing whichever node it happens to find: the list scrolls, filters or
  // highlights by showing one heading where a different one belongs.
  //
  // So the position stays the prefix, and repeats are numbered. First come keeps
  // the plain `index@offset`, which is also what the existing keys look like, and
  // only a genuine repeat grows a `#n` suffix.
  const seen = new Map<string, number>();
  return contents.map((row) => {
    const leads = row.index !== last;
    last = row.index;
    const position = `${row.index}@${row.offset}`;
    const occurrence = (seen.get(position) ?? 0) + 1;
    seen.set(position, occurrence);
    return {
      index: row.index,
      offset: row.offset,
      title: row.title,
      depth: row.depth,
      leads,
      key: occurrence === 1 ? position : `${position}#${occurrence}`,
    };
  });
}

/**
 * Which row a reader is in.
 *
 * The last row at or before the current position: the last row of an earlier
 * section while the open one is scrolled above its own first row, and otherwise
 * the row at or above the top of the viewport. -1 when there are no rows.
 */
export function activeRowIndex(
  rows: readonly ContentsRow[],
  sectionIndex: number,
  offset: number,
): number {
  let found = -1;
  for (let i = 0; i < rows.length; i += 1) {
    const row = rows[i]!;
    if (row.index < sectionIndex) {
      found = i;
      continue;
    }
    if (row.index > sectionIndex) break;
    if (row.offset <= offset) found = i;
  }
  return found;
}

/** The row a reader is in, for highlighting and for the chrome's title. */
export function activeRow(
  rows: readonly ContentsRow[],
  sectionIndex: number,
  offset: number,
): ContentsRow | null {
  const index = activeRowIndex(rows, sectionIndex, offset);
  return index === -1 ? null : rows[index]!;
}

/** Row titles matching a search, case- and accent-insensitively enough. */
export function filterContentsRows(
  rows: readonly ContentsRow[],
  query: string,
): ContentsRow[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return [...rows];
  return rows.filter((row) => row.title.toLowerCase().includes(needle));
}