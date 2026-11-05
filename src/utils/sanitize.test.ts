import { describe, expect, it } from 'vitest';

import { fileExtension, isSafeStorageKey, safeNumber, sanitizeText } from './sanitize';

describe('sanitizeText', () => {
  it('trims whitespace', () => {
    expect(sanitizeText('  hello  ')).toBe('hello');
  });

  it('truncates to the max length', () => {
    expect(sanitizeText('a'.repeat(300), 100)).toHaveLength(100);
  });

  it('handles non-string input', () => {
    expect(sanitizeText(null)).toBe('');
    expect(sanitizeText(undefined)).toBe('');
    expect(sanitizeText(42)).toBe('42');
  });
});

describe('safeNumber', () => {
  it('returns the value when within range', () => {
    expect(safeNumber(5, 0, 10, 0)).toBe(5);
  });

  it('clamps rather than falling back when out of range', () => {
    expect(safeNumber(15, 0, 10, 3)).toBe(10);
    expect(safeNumber(-1, 0, 10, 3)).toBe(0);
  });

  it('returns the fallback for NaN', () => {
    expect(safeNumber(NaN, 0, 10, 3)).toBe(3);
  });
});

describe('fileExtension', () => {
  it('extracts the extension', () => {
    expect(fileExtension('song.mp3')).toBe('.mp3');
  });

  it('handles uppercase extensions', () => {
    expect(fileExtension('SONG.MP3')).toBe('.mp3');
  });

  it('returns an empty string when there is no extension', () => {
    expect(fileExtension('noextension')).toBe('');
  });
});

describe('isSafeStorageKey', () => {
  it('accepts a normal key', () => {
    expect(isSafeStorageKey('abc123')).toBe(true);
  });

  it('rejects a key with path separators', () => {
    expect(isSafeStorageKey('../etc/passwd')).toBe(false);
  });

  it('rejects an empty key', () => {
    expect(isSafeStorageKey('')).toBe(false);
  });
});
