import { describe, expect, it } from 'vitest';

import { clamp, formatDuration, formatDurationShort, formatPercent, pluralize } from './format';

describe('formatDuration', () => {
  it('formats zero', () => {
    expect(formatDuration(0)).toBe('0:00');
  });

  it('formats seconds only', () => {
    expect(formatDuration(45)).toBe('0:45');
  });

  it('formats minutes and seconds', () => {
    expect(formatDuration(125)).toBe('2:05');
  });

  it('formats hours', () => {
    expect(formatDuration(3661)).toBe('1:01:01');
  });

  it('handles null and undefined', () => {
    expect(formatDuration(null)).toBe('0:00');
    expect(formatDuration(undefined)).toBe('0:00');
  });
});

describe('formatDurationShort', () => {
  it('formats minutes', () => {
    expect(formatDurationShort(125)).toBe('2m');
  });

  it('formats hours', () => {
    expect(formatDurationShort(3661)).toBe('1h 1m');
  });
});

describe('formatPercent', () => {
  it('formats a fraction as a percentage', () => {
    expect(formatPercent(0.5)).toBe('50%');
  });

  it('rounds to the nearest integer', () => {
    expect(formatPercent(0.333)).toBe('33%');
  });

  it('handles null', () => {
    expect(formatPercent(null)).toBe('0%');
  });
});

describe('pluralize', () => {
  it('returns the singular form for 1', () => {
    expect(pluralize(1, 'chapter')).toBe('1 chapter');
  });

  it('returns the plural form for 0', () => {
    expect(pluralize(0, 'chapter')).toBe('0 chapters');
  });

  it('returns the plural form for many', () => {
    expect(pluralize(5, 'chapter')).toBe('5 chapters');
  });

  it('supports a custom plural', () => {
    expect(pluralize(2, 'child', 'children')).toBe('2 children');
  });
});

describe('clamp', () => {
  it('returns the value when within range', () => {
    expect(clamp(5, 0, 10)).toBe(5);
  });

  it('clamps to the minimum', () => {
    expect(clamp(-5, 0, 10)).toBe(0);
  });

  it('clamps to the maximum', () => {
    expect(clamp(15, 0, 10)).toBe(10);
  });
});
