/**
 * Contents-row and offset-lookup tests.
 *
 * These are the two places the reader can quietly send a reader to the wrong
 * place: picking the wrong contents row as "the one you are reading", and
 * resolving a character offset to the wrong block. Both are pure lookups, so
 * they are tested here rather than through a browser.
 */

import { describe, expect, it } from 'vitest';

import {
  activeRow,
  activeRowIndex,
  buildContentsRows,
  filterContentsRows,
} from './contents-rows';
import { blockAtOffset, type TextBlock } from './text-index';
import type { EpubChapterEntry, EpubContentsEntry } from '@/types';

const chapters: EpubChapterEntry[] = [
  { id: 'a', index: 0, title: 'Front matter', depth: 0, inToc: true, wordCount: 10, charOffset: 0 },
  { id: 'b', index: 1, title: 'Chapter One', depth: 0, inToc: true, wordCount: 900, charOffset: 10 },
  { id: 'c', index: 2, title: 'Chapter Two', depth: 0, inToc: true, wordCount: 800, charOffset: 910 },
];

const contents: EpubContentsEntry[] = [
  { index: 0, anchor: '', offset: 0, title: 'Cover', depth: 0 },
  { index: 0, anchor: '', offset: 40, title: 'Contents', depth: 0 },
  { index: 1, anchor: '', offset: 0, title: 'Part One', depth: 0 },
  { index: 1, anchor: 's2', offset: 900, title: 'Chapter One', depth: 1 },
  { index: 2, anchor: '', offset: 0, title: 'Chapter Two', depth: 0 },
];

describe('buildContentsRows', () => {
  it('uses the parsed contents rather than the section list', () => {
    const rows = buildContentsRows(contents, chapters);

    expect(rows.map((row) => row.title)).toEqual([
      'Cover',
      'Contents',
      'Part One',
      'Chapter One',
      'Chapter Two',
    ]);
    // The nesting the book asked for survives, so a part can contain chapters.
    expect(rows.map((row) => row.depth)).toEqual([0, 0, 0, 1, 0]);
  });

  it('marks only the first row of each section as leading', () => {
    const rows = buildContentsRows(contents, chapters);

    // One audio control per section, not one per contents row: "Contents" and
    // "Cover" are two rows in the same document and share one chapter.
    expect(rows.filter((row) => row.leads).map((row) => row.index)).toEqual([0, 1, 2]);
  });

  it('keys a row by its place in the text, since a section holds several', () => {
    const rows = buildContentsRows(contents, chapters);

    expect(new Set(rows.map((row) => row.key)).size).toBe(rows.length);
    expect(rows[0]!.key).not.toBe(rows[1]!.key);
  });

  it('falls back to one row per section when there is no parsed contents', () => {
    // A book stored before the parser grew contents rows still navigates.
    expect(buildContentsRows([], chapters).map((row) => row.title)).toEqual([
      'Front matter',
      'Chapter One',
      'Chapter Two',
    ]);
    expect(buildContentsRows(undefined, chapters)).toHaveLength(3);
    expect(buildContentsRows([], []).length).toBe(0);
  });
});

/**
 * Real books do produce rows that share a position.
 *
 * The parser repairs a run of three or more rows collapsed onto one document,
 * but deliberately leaves a pair alone: a part heading and the chapter opening
 * it are genuinely both at the top of that document. Ikigai has 27 such pairs
 * and "Don't Believe Everything You Think" one more, which is why the key has
 * to be unique for a position rather than *be* the position.
 */
describe('buildContentsRows with rows sharing a position', () => {
  const colliding: EpubContentsEntry[] = [
    { index: 0, anchor: '', offset: 0, title: 'Prologue', depth: 0 },
    { index: 1, anchor: '', offset: 0, title: 'Part One: Ikigai', depth: 0 },
    { index: 1, anchor: '', offset: 9431, title: 'Whatever you do, don’t retire!', depth: 1 },
    { index: 1, anchor: '', offset: 9431, title: 'The island of eternal youth', depth: 1 },
    { index: 1, anchor: '', offset: 11984, title: 'The five Blue Zones', depth: 1 },
    { index: 1, anchor: '', offset: 11984, title: 'The 80 percent secret', depth: 1 },
  ];

  it('still gives every row a key React can tell apart', () => {
    const rows = buildContentsRows(colliding, chapters);
    const keys = rows.map((row) => row.key);

    expect(keys).toHaveLength(colliding.length);
    expect(new Set(keys).size).toBe(colliding.length);
  });

  it('keeps rows that do not collide on the plain index@offset key', () => {
    const rows = buildContentsRows(colliding, chapters);

    // Only a genuine repeat is numbered, so a key stays readable in a
    // profiler or a saved-progress record.
    expect(rows[0]!.key).toBe('0@0');
    expect(rows[2]!.key).toBe('1@9431');
    expect(rows[3]!.key).toBe('1@9431#2');
    expect(rows[4]!.key).toBe('1@11984');
  });

  it('still lists the rows in the order the book puts them', () => {
    // Numbering a repeat must not reorder anything: a part heading still comes
    // before the chapter it opens, and the later pair still comes after.
    expect(buildContentsRows(colliding, chapters).map((row) => row.title)).toEqual(
      colliding.map((row) => row.title),
    );
  });

  it('highlights one row of a colliding pair, not both', () => {
    const rows = buildContentsRows(colliding, chapters);
    const active = activeRow(rows, 1, 9431);

    // Being at that offset means the reader is just past it, so the heading that
    // follows is the one they are in — but only that one row is current.
    expect(active?.title).toBe('The island of eternal youth');
    expect(rows.filter((row) => row.key === active?.key)).toHaveLength(1);
  });

  it('does not let the audio control appear twice for one section', () => {
    const rows = buildContentsRows(colliding, chapters);

    // The leading flag is what the panel hangs the play control off, so two
    // leading rows for one section would offer the same audio twice.
    expect(rows.filter((row) => row.leads && row.index === 1)).toHaveLength(1);
  });
});

describe('activeRow', () => {
  const rows = buildContentsRows(contents, chapters);

  it('names the row the top of the viewport has reached', () => {
    expect(activeRow(rows, 1, 0)?.title).toBe('Part One');
    expect(activeRow(rows, 1, 950)?.title).toBe('Chapter One');
  });

  it('stays on the last row of the previous section above a section’s first row', () => {
    // The section opens with folded-in front matter, so the viewport can sit
    // before the section's own first row.
    expect(activeRow(rows, 2, 0)?.title).toBe('Chapter Two');
    expect(activeRow(rows, 0, 0)?.title).toBe('Cover');
  });

  it('is the first row of a section with nothing before it in the contents', () => {
    expect(activeRow(buildContentsRows([contents[4]!], chapters), 2, 0)?.title).toBe('Chapter Two');
  });

  it('reports nothing when there are no rows at all', () => {
    expect(activeRowIndex([], 0, 0)).toBe(-1);
    expect(activeRow([], 0, 0)).toBeNull();
  });

  it('never walks past the section it was asked about', () => {
    // Rows are ordered, so a section index past the end is a clamp, not a scan
    // into the next book.
    expect(activeRowIndex(rows, 99, 0)).toBe(rows.length - 1);
  });
});

describe('filterContentsRows', () => {
  const rows = buildContentsRows(contents, chapters);

  it('returns everything for an empty query', () => {
    expect(filterContentsRows(rows, '   ')).toHaveLength(rows.length);
  });

  it('matches anywhere in a title, ignoring case and surrounding space', () => {
    expect(filterContentsRows(rows, '  CHAPTER ').map((row) => row.title)).toEqual([
      'Chapter One',
      'Chapter Two',
    ]);
  });

  it('returns nothing rather than everything when nothing matches', () => {
    expect(filterContentsRows(rows, 'appendix')).toEqual([]);
  });
});

describe('blockAtOffset', () => {
  // Stands in for an injected chapter: three paragraphs of known length.
  const blocks: TextBlock[] = [
    { start: 0, end: 10, el: {} as HTMLElement },
    { start: 10, end: 40, el: {} as HTMLElement },
    { start: 40, end: 60, el: {} as HTMLElement },
  ];

  it('finds the block an offset falls inside', () => {
    expect(blockAtOffset(blocks, 0)).toBe(blocks[0]);
    expect(blockAtOffset(blocks, 9)).toBe(blocks[0]);
    expect(blockAtOffset(blocks, 10)).toBe(blocks[1]);
    expect(blockAtOffset(blocks, 59)).toBe(blocks[2]);
  });

  it('resolves an offset between blocks to the one just read', () => {
    // Whitespace tidying on the server means offsets can land a character past a
    // block's end. Resolving forward would jump to the next chapter.
    expect(blockAtOffset(blocks, 40)).toBe(blocks[2]);
    expect(blockAtOffset(blocks, 39.9) as TextBlock).toBe(blocks[1]);
  });

  it('clamps before the start and after the end of the text', () => {
    expect(blockAtOffset(blocks, -500)).toBe(blocks[0]);
    expect(blockAtOffset(blocks, 1e9)).toBe(blocks[2]);
  });

  it('has nothing to say about an empty chapter', () => {
    expect(blockAtOffset([], 0)).toBeNull();
  });

  it('agrees with a linear scan over a long index', () => {
    // A binary search that is subtly off by one only shows up on a real chapter.
    const long: TextBlock[] = Array.from({ length: 500 }, (_, i) => ({
      start: i * 7,
      end: i * 7 + 7,
      el: {} as HTMLElement,
    }));
    for (let offset = 0; offset < 500 * 7; offset += 1) {
      const expected = Math.min(499, Math.floor(offset / 7));
      expect(long.indexOf(blockAtOffset(long, offset)!)).toBe(expected);
    }
  });
});