import { baseLanguage } from './constants';
import type { TtsVoice } from '@/types';

/**
 * Reading an Edge voice name the way a person would.
 *
 * A voice identifier carries everything needed to describe it —
 * `hi-IN-SwaraNeural` is a woman reading Hindi as spoken in India — but the
 * identifier itself is not what anyone wants to read in a list. These helpers
 * split it into parts the picker can filter on and label with.
 */

const languageNames =
  typeof Intl !== 'undefined' && 'DisplayNames' in Intl
    ? new Intl.DisplayNames(['en'], { type: 'language' })
    : null;

const regionNames =
  typeof Intl !== 'undefined' && 'DisplayNames' in Intl
    ? new Intl.DisplayNames(['en'], { type: 'region' })
    : null;

/** English name of a language code, title-cased, falling back to the code. */
export function spokenLanguageName(code: string): string {
  const base = baseLanguage(code);
  if (!base) return 'Unknown';
  try {
    const name = languageNames?.of(base);
    if (name && name !== base) return name;
  } catch {
    /* an unknown code is not a reason to fail rendering */
  }
  return base.charAt(0).toUpperCase() + base.slice(1);
}

export type VoiceGender = 'female' | 'male' | 'other';

/** Normalised Edge gender, so `Female`, `female` and a missing value agree. */
export function voiceGender(voice: Pick<TtsVoice, 'gender'>): VoiceGender {
  const value = voice.gender.trim().toLowerCase();
  if (value === 'female') return 'female';
  if (value === 'male') return 'male';
  return 'other';
}

/**
 * Region subtag of a locale, or null for language-only locales.
 *
 * Script subtags (`Latn`, `Hans`) are four letters and are skipped, so
 * `sr-Latn-RS` still yields `RS` rather than mistaking the script for a region.
 */
export function voiceRegion(locale: string): string | null {
  const parts = locale.split(/[-_]/).slice(1);
  for (const part of parts) {
    if (/^[A-Za-z]{2}$/.test(part) || /^[0-9]{3}$/.test(part)) return part.toUpperCase();
  }
  return null;
}

/** English name of a region code, falling back to the code. */
export function regionName(code: string): string {
  try {
    return regionNames?.of(code) ?? code;
  } catch {
    return code;
  }
}

/**
 * The human part of a voice identifier: `hi-IN-SwaraNeural` → `Swara`.
 *
 * Edge puts the language, the region, and then the speaker's name before a
 * suffix, so everything between the second dash and a trailing `Neural` is the
 * name. A label like "Swara · Hindi" is more useful than the raw identifier.
 */
export function voiceDisplayName(name: string): string {
  const withoutSuffix = name.replace(/(Multilingual)?Neural$/i, '');
  const parts = withoutSuffix.split('-');
  const personal = parts.length > 2 ? parts.slice(2).join('-') : withoutSuffix;
  return personal.replace(/([a-z])([A-Z])/g, '$1 $2') || name;
}

/** A one-line description of a voice for list rows and buttons. */
export function voiceDescription(voice: TtsVoice): string {
  const language = spokenLanguageName(voice.locale);
  const region = voiceRegion(voice.locale);
  const gender = voiceGender(voice);
  return [language, region ? regionName(region) : null, gender === 'other' ? null : gender]
    .filter(Boolean)
    .join(' · ');
}
