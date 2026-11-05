import { afterEach, describe, expect, it, vi } from 'vitest';

import { api } from './use-api';

/**
 * The `api` helpers unwrap the server's `{ data: ... }` envelope and hand the
 * caller `T` directly.
 *
 * This is worth pinning down because getting it wrong is invisible to
 * TypeScript: asking for `api.get<{ data: Foo[] }>` type-checks perfectly and
 * returns `undefined` at runtime, which is how the voice selector ended up
 * permanently disabled with an empty dropdown.
 */

function respondWith(body: unknown, init: Partial<Response> = {}): void {
  vi.stubGlobal(
    'fetch',
    vi.fn(async () => new Response(JSON.stringify(body), { status: 200, ...init })),
  );
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('api response unwrapping', () => {
  it('returns the unwrapped value of a list endpoint, not the envelope', async () => {
    const voices = [{ name: 'en-US-GuyNeural', locale: 'en-US', gender: 'Male' }];
    respondWith({ data: voices });

    const result = await api.get<typeof voices>('/api/tts/voices');

    // The regression: `result` used to be read as `result.data`, which is
    // undefined, leaving the select with nothing to render.
    expect(Array.isArray(result)).toBe(true);
    expect(result).toEqual(voices);
    expect(result).toHaveLength(1);
  });

  it('returns a bare object for a single-resource endpoint', async () => {
    const settings = { volume: 0.8, ttsVoice: 'en-GB-SoniaNeural' };
    respondWith({ data: settings });

    const result = await api.get<typeof settings>('/api/settings');

    expect(result.ttsVoice).toBe('en-GB-SoniaNeural');
  });

  it('returns undefined for 204 rather than a parse error', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(null, { status: 204 })));

    await expect(api.delete('/api/chapters/abc')).resolves.toBeUndefined();
  });

  it('turns an error envelope into an ApiError carrying the server message', async () => {
    respondWith(
      { error: { message: 'That chapter no longer exists.', code: 'not_found' } },
      { status: 404 },
    );

    await expect(api.get('/api/chapters/missing')).rejects.toThrow('That chapter no longer exists.');
  });
});