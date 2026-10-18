/**
 * Language code tests.
 *
 * The codes here go straight into an outbound request as the target language, so
 * a code that loses its region or its script is not a formatting slip — it is the
 * app asking for Portuguese instead of Brazilian Portuguese, or Simplified
 * Chinese instead of Traditional.
 */

import { describe, expect, it } from 'vitest';

import {
  canonicalLanguageCode,
  translationLanguageName,
  TRANSLATION_LANGUAGES,
} from './constants';

describe('canonicalLanguageCode', () => {
  it('leaves a plain language alone', () => {
    expect(canonicalLanguageCode('es')).toBe('es');
  });

  it('keeps the region of a regional language', () => {
    // Brazilian Portuguese is a different language from Portuguese, and the app
    // has both on offer, so the code has to survive.
    expect(canonicalLanguageCode('pt-BR')).toBe('pt-BR');
    expect(canonicalLanguageCode('pt-br')).toBe('pt-BR');
    expect(canonicalLanguageCode('PT-br')).toBe('pt-BR');
    expect(canonicalLanguageCode('en-GB')).toBe('en-GB');
  });

  it('keeps the script of a scripted language', () => {
    expect(canonicalLanguageCode('zh-cn')).toBe('zh-CN');
    expect(canonicalLanguageCode('zh-tw')).toBe('zh-TW');
    expect(canonicalLanguageCode('sr-latn')).toBe('sr-Latn');
  });

  it('keeps a numeric region', () => {
    expect(canonicalLanguageCode('es-419')).toBe('es-419');
  });

  it('trims what a reader pasted around a code', () => {
    expect(canonicalLanguageCode('  fr  ')).toBe('fr');
  });

  it('gives the same code for the same language however it was typed', () => {
    expect(canonicalLanguageCode('ZH-cn')).toBe(canonicalLanguageCode('zh-CN'));
  });
});

describe('translationLanguageName', () => {
  it('names a code from the list, however it was typed', () => {
    expect(translationLanguageName('pt-BR')).toBe('Brazilian Portuguese');
    expect(translationLanguageName('PT-br')).toBe('Brazilian Portuguese');
  });

  it('shows the code itself for a language the list does not name', () => {
    // The provider understands far more than the list offers.
    expect(translationLanguageName('sw-KE')).toBe('sw-KE');
  });

  it('has something to say about nothing at all', () => {
    expect(translationLanguageName('  ')).toBe('Unknown');
  });
});

describe('TRANSLATION_LANGUAGES', () => {
  it('has no duplicate codes', () => {
    const codes = TRANSLATION_LANGUAGES.map((item) => item.code);
    expect(new Set(codes).size).toBe(codes.length);
  });

  it('stores every code in its canonical form', () => {
    // Otherwise the row would be written under a code nothing else can find.
    for (const item of TRANSLATION_LANGUAGES) {
      expect(item.code).toBe(canonicalLanguageCode(item.code));
    }
  });

  it('is a code the provider could accept', () => {
    for (const item of TRANSLATION_LANGUAGES) {
      expect(item.code).toMatch(/^[a-z]{2,3}(-[A-Za-z0-9]{2,8})*$/);
    }
  });

  it('names every language in English and in its own script', () => {
    // A list of bare codes is a list a reader cannot choose from.
    for (const item of TRANSLATION_LANGUAGES) {
      expect(item.name.length).toBeGreaterThan(1);
      expect(item.native.length).toBeGreaterThan(1);
    }
  });
});
