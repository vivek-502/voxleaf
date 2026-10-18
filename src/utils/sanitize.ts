/**
 * Input sanitisation and validation helpers.
 *
 * VoxLeaf renders a lot of provider-supplied and user-supplied text
 * (chapter titles, bookmark notes, book descriptions). React escapes text
 * nodes for us, so the job here is to strip control characters, normalise
 * whitespace, and bound the length so a hostile paste cannot bloat the DB or
 * break layout.
 */

/** Remove C0/C1 control chars (keep tab/newline), strip bidi overrides & zero-widths. */
const CONTROL_CHARS = /[\u0000-\u0008\u000B\u000C\u000E-\u001F\u007F-\u009F\u202A-\u202E\u2066-\u2069\u200B-\u200D\uFEFF]/g;

/**
 * Normalise free text: strip control/bidi characters, collapse runs of
 * whitespace, and trim. Optionally clamp to `maxLength`.
 */
export function sanitizeText(input: unknown, maxLength = 2000): string {
  if (input === null || input === undefined) return '';
  let value = String(input);
  value = value.replace(CONTROL_CHARS, '');
  // Normalise newlines and tabs to single spaces for single-line fields.
  value = value.replace(/[\r\n\t]+/g, ' ');
  value = value.replace(/ {2,}/g, ' ');
  return value.trim().slice(0, maxLength);
}

/** Like {@link sanitizeText} but preserves paragraph breaks (descriptions). */
export function sanitizeMultiline(input: unknown, maxLength = 20000): string {
  if (input === null || input === undefined) return '';
  let value = String(input);
  value = value.replace(CONTROL_CHARS, '');
  value = value.replace(/\r\n/g, '\n').replace(/\n{3,}/g, '\n\n');
  return value.trim().slice(0, maxLength);
}

/**
 * Make a user-supplied filename safe to *display* only.
 * The actual storage key is always generated server-side, never derived from
 * this value, so this exists purely to avoid confusing UI and log output.
 */
export function sanitizeFileName(input: unknown, maxLength = 120): string {
  const base = sanitizeText(input, 400).replace(/[/\\]/g, '-').replace(/[^\p{L}\p{N}._()\- ]/gu, '');
  const trimmed = base.replace(/^[.\s-]+/, '').trim();
  return (trimmed || 'audio').slice(0, maxLength);
}

/** File extension, lowercased, including the dot. `""` when there is none. */
export function fileExtension(name: string): string {
  const match = /\.([A-Za-z0-9]{1,8})$/.exec(name.trim());
  return match ? `.${match[1]!.toLowerCase()}` : '';
}

/**
 * Reject traversal and absolute paths in any key we generate or accept.
 * Returns true when `candidate` is safe to join under a storage root.
 */
export function isSafeStorageKey(candidate: string): boolean {
  if (!candidate || candidate.length > 512) return false;
  if (candidate.includes('\0')) return false;
  if (candidate.startsWith('/') || candidate.startsWith('\\')) return false;
  if (/^[A-Za-z]:/.test(candidate)) return false;
  if (candidate.startsWith('~')) return false;
  const segments = candidate.split(/[/\\]/);
  if (segments.some((segment) => segment === '..' || segment === '.' || segment === '')) return false;
  return /^[A-Za-z0-9/_.-]+$/.test(candidate);
}

/** Clamp a finite number into a range, falling back when input is unusable. */
export function safeNumber(value: unknown, min: number, max: number, fallback: number): number {
  const parsed = typeof value === 'number' ? value : Number(value);
  if (!Number.isFinite(parsed)) return fallback;
  return Math.min(max, Math.max(min, parsed));
}

/** Parse a JSON column defensively, always returning an array of strings. */
export function parseStringArray(value: unknown): string[] {
  if (Array.isArray(value)) {
    return value.map((item) => sanitizeText(item, 80)).filter(Boolean).slice(0, 32);
  }
  if (typeof value !== 'string' || !value.trim()) return [];
  try {
    const parsed = JSON.parse(value);
    if (Array.isArray(parsed)) {
      return parsed.map((item) => sanitizeText(item, 80)).filter(Boolean).slice(0, 32);
    }
  } catch {
    // fall through: some providers return a plain comma-separated string
  }
  return value
    .split(',')
    .map((item) => sanitizeText(item, 80))
    .filter(Boolean)
    .slice(0, 32);
}
