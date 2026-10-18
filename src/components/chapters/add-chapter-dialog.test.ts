import { describe, expect, it } from 'vitest';

import { parseChapterLines } from './add-chapter-dialog';

describe('parseChapterLines', () => {
  it('parses a numbered list', () => {
    const { chapters, invalid } = parseChapterLines('1. First\n2. Second');
    expect(chapters).toEqual([
      { chapterNumber: 1, title: 'First' },
      { chapterNumber: 2, title: 'Second' },
    ]);
    expect(invalid).toEqual([]);
  });

  it('parses a "Chapter N: Title" list', () => {
    const { chapters } = parseChapterLines('Chapter 1: The Beginning');
    expect(chapters).toEqual([{ chapterNumber: 1, title: 'The Beginning' }]);
  });

  it('parses a plain list without numbers', () => {
    const { chapters } = parseChapterLines('Prologue\nEpilogue');
    expect(chapters).toEqual([
      { chapterNumber: null, title: 'Prologue' },
      { chapterNumber: null, title: 'Epilogue' },
    ]);
  });

  it('skips blank lines', () => {
    const { chapters } = parseChapterLines('First\n\n\nSecond');
    expect(chapters).toHaveLength(2);
  });

  it('flags unrecognised lines', () => {
    const { chapters, invalid } = parseChapterLines('1. Good\n???');
    expect(chapters).toHaveLength(1);
    expect(invalid).toEqual(['???']);
  });

  it('handles CRLF line endings', () => {
    const { chapters } = parseChapterLines('1. First\r\n2. Second');
    expect(chapters).toHaveLength(2);
  });
});
