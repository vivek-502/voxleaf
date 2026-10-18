/** Formatting helpers. All pure — safe to use on client and server. */

/**
 * Human-readable duration.
 * 0 -> "0:00", 65 -> "1:05", 3725 -> "1:02:05", 72000 -> "20:00:00"
 * Negative / non-finite input is clamped to zero.
 */
export function formatDuration(totalSeconds: number | null | undefined): string {
  const seconds = Math.max(0, Math.floor(Number(totalSeconds) || 0));
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const secs = seconds % 60;
  const mm = hours > 0 ? String(minutes).padStart(2, '0') : String(minutes);
  return `${hours > 0 ? `${hours}:` : ''}${mm}:${String(secs).padStart(2, '0')}`;
}

/** Compact duration for cards: "1h 2m", "48m", "35s". */
export function formatDurationShort(totalSeconds: number | null | undefined): string {
  const seconds = Math.max(0, Math.floor(Number(totalSeconds) || 0));
  if (seconds < 60) return `${seconds}s`;
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.round((seconds % 3600) / 60);
  if (hours === 0) return `${minutes}m`;
  if (minutes === 0) return `${hours}h`;
  return `${hours}h ${minutes}m`;
}

/** "-12:04" style remaining time, for the player. */
export function formatRemaining(position: number, duration: number): string {
  return `-${formatDuration(Math.max(0, duration - position))}`;
}

const BYTE_UNITS = ['B', 'KB', 'MB', 'GB', 'TB'] as const;

export function formatBytes(bytes: number | null | undefined): string {
  const value = Number(bytes) || 0;
  if (value <= 0) return '0 B';
  const exponent = Math.min(Math.floor(Math.log(value) / Math.log(1024)), BYTE_UNITS.length - 1);
  const size = value / 1024 ** exponent;
  return `${exponent === 0 ? size : size.toFixed(size >= 10 ? 0 : 1)} ${BYTE_UNITS[exponent]}`;
}

export function formatPercent(fraction: number | null | undefined, digits = 0): string {
  const value = Math.max(0, Math.min(1, Number(fraction) || 0)) * 100;
  return `${value.toFixed(digits)}%`;
}

/** "12 chapters", "1 chapter" */
export function pluralize(count: number, singular: string, plural = `${singular}s`): string {
  return `${count} ${count === 1 ? singular : plural}`;
}

/**
 * Relative time: "just now", "12m ago", "3h ago", "2 days ago", then a date.
 */
export function formatRelativeTime(input: string | number | Date | null | undefined): string {
  if (!input) return 'never';
  const date = input instanceof Date ? input : new Date(input);
  if (Number.isNaN(date.getTime())) return 'never';
  const diffMs = Date.now() - date.getTime();
  const diffSeconds = Math.round(diffMs / 1000);
  if (diffSeconds < 45) return 'just now';
  const units: Array<[Intl.RelativeTimeFormatUnit, number]> = [
    ['minute', 60],
    ['hour', 3600],
    ['day', 86400],
    ['week', 604800],
    ['month', 2592000],
    ['year', 31536000],
  ];
  let chosen: [Intl.RelativeTimeFormatUnit, number] = units[0];
  for (const unit of units) {
    if (diffSeconds >= unit[1]) chosen = unit;
  }
  const rtf = new Intl.RelativeTimeFormat('en', { numeric: 'auto' });
  return rtf.format(-Math.round(diffSeconds / chosen[1]), chosen[0]);
}

/** "12 Mar 2024" */
export function formatDate(input: string | number | Date | null | undefined): string {
  if (!input) return '—';
  const date = input instanceof Date ? input : new Date(input);
  if (Number.isNaN(date.getTime())) return '—';
  return new Intl.DateTimeFormat('en', {
    day: 'numeric',
    month: 'short',
    year: 'numeric',
  }).format(date);
}

/** Parse provider publication dates into an ISO-ish `YYYY-MM-DD` when possible. */
export function normalizePublicationDate(value: string | null | undefined): string | null {
  if (!value) return null;
  const trimmed = String(value).trim();
  if (!trimmed) return null;
  const yearMatch = trimmed.match(/(\d{4})/);
  if (!yearMatch) return null;
  const year = Number(yearMatch[1]);
  if (!Number.isFinite(year) || year < -3000 || year > 3000) return null;
  const parsed = new Date(trimmed);
  if (!Number.isNaN(parsed.getTime()) && /\d{1,2}[/-]\d{1,2}/.test(trimmed)) {
    return parsed.toISOString().slice(0, 10);
  }
  return `${year}-01-01`;
}

/** Trailing-edge debounce. */
export function debounce<A extends unknown[]>(
  fn: (...args: A) => void,
  waitMs: number,
): ((...args: A) => void) & { cancel: () => void; flush: () => void } {
  let timer: ReturnType<typeof setTimeout> | null = null;
  let pending: A | null = null;

  const debounced = (...args: A) => {
    pending = args;
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => {
      timer = null;
      const call = pending;
      pending = null;
      if (call) fn(...call);
    }, waitMs);
  };

  debounced.cancel = () => {
    if (timer) clearTimeout(timer);
    timer = null;
    pending = null;
  };

  debounced.flush = () => {
    if (timer) clearTimeout(timer);
    timer = null;
    const call = pending;
    pending = null;
    if (call) fn(...call);
  };

  return debounced;
}

/** Clamp helper. */
export function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

/** Initials for the fallback cover, e.g. "Atomic Habits" -> "AH". */
export function initialsFromTitle(title: string): string {
  const words = title
    .replace(/[^\p{L}\p{N}\s]/gu, ' ')
    .split(/\s+/)
    .filter(Boolean);
  if (words.length === 0) return '?';
  if (words.length === 1) return words[0]!.slice(0, 2).toUpperCase();
  return `${words[0]![0]}${words[1]![0]}`.toUpperCase();
}
