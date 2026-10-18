/**
 * Typed fetch wrapper for the app's JSON API.
 *
 * Every route responds with either `{ data }` or `{ error: { code, message } }`,
 * so a single helper can unwrap success and turn failures into a real `Error`
 * carrying the friendly server message plus a machine-readable code.
 */

export class ApiError extends Error {
  readonly code: string;
  readonly status: number;
  readonly details?: Array<{ path: string; message: string }>;

  constructor(message: string, code: string, status: number, details?: Array<{ path: string; message: string }>) {
    super(message);
    this.name = 'ApiError';
    this.code = code;
    this.status = status;
    this.details = details;
  }
}

export function apiUrl(path: string, params?: Record<string, string | number | undefined | null>): string {
  const url = path.startsWith('/api') ? path : `/api${path}`;
  if (!params) return url;
  const search = new URLSearchParams();
  for (const [key, value] of Object.entries(params)) {
    if (value === undefined || value === null || value === '') continue;
    search.set(key, String(value));
  }
  const qs = search.toString();
  return qs ? `${url}?${qs}` : url;
}

async function parseError(response: Response): Promise<ApiError> {
  let message = 'Something went wrong. Please try again.';
  let code = 'unknown';
  let details: Array<{ path: string; message: string }> | undefined;
  try {
    const body = (await response.json()) as {
      error?: { message?: string; code?: string; details?: Array<{ path: string; message: string }> };
    };
    if (body?.error) {
      message = body.error.message ?? message;
      code = body.error.code ?? code;
      details = body.error.details;
    }
  } catch {
    // Non-JSON error body (e.g. the streaming route's plain-text responses).
  }
  return new ApiError(message, code, response.status, details);
}

async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
  let response: Response;
  try {
    response = await fetch(path, {
      ...init,
      headers: {
        ...(init.body && !(init.body instanceof FormData) ? { 'content-type': 'application/json' } : {}),
        ...init.headers}});
  } catch {
    throw new ApiError('You appear to be offline. Check your connection and try again.', 'network', 0);
  }

  if (!response.ok) throw await parseError(response);

  if (response.status === 204) return undefined as T;
  const body = (await response.json()) as { data: T };
  return body.data;
}

export const api = {
  get: <T>(path: string, params?: Record<string, string | number | undefined | null>) =>
    request<T>(apiUrl(path, params), { method: 'GET' }),

  post: <T>(path: string, body?: unknown) =>
    request<T>(path, { method: 'POST', body: body === undefined ? undefined : JSON.stringify(body) }),

  put: <T>(path: string, body?: unknown) =>
    request<T>(path, { method: 'PUT', body: body === undefined ? undefined : JSON.stringify(body) }),

  patch: <T>(path: string, body?: unknown) =>
    request<T>(path, { method: 'PATCH', body: body === undefined ? undefined : JSON.stringify(body) }),

  delete: <T>(path: string, body?: unknown) =>
    request<T>(path, { method: 'DELETE', body: body === undefined ? undefined : JSON.stringify(body) }),

  /**
   * POST to a route that answers with newline-delimited progress frames.
   *
   * Used for work that takes long enough that a reader needs to see it moving:
   * translating a chapter is a dozen or more requests, and an opaque spinner for
   * half a minute is indistinguishable from a hung connection. Frames are
   * `{type:'progress', done, total}`, then one `{type:'done', data}` or one
   * `{type:'error', code, message}` — a failure after the stream has opened
   * cannot change the status code, so it arrives as a frame.
   */
  stream: async <T>(
    path: string,
    body: unknown,
    onProgress?: (progress: { done: number; total: number }) => void,
    signal?: AbortSignal,
  ): Promise<T> => {
    let response: Response;
    try {
      response = await fetch(path, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify(body ?? {}),
        signal,
      });
    } catch (error) {
      if ((error as Error).name === 'AbortError') throw new ApiError('Cancelled.', 'aborted', 0);
      throw new ApiError('You appear to be offline. Check your connection and try again.', 'network', 0);
    }

    if (!response.ok) throw await parseError(response);
    if (!response.body) throw new ApiError('The server sent no progress.', 'bad_response', response.status);

    const reader = response.body.getReader();
    const decoder = new TextDecoder();
    let buffered = '';
    let result: T | undefined;

    const readFrames = () => {
      for (;;) {
        const newline = buffered.indexOf('\n');
        if (newline === -1) return;
        const line = buffered.slice(0, newline).trim();
        buffered = buffered.slice(newline + 1);
        if (!line) continue;

        let frame: {
          type?: string;
          done?: number;
          total?: number;
          data?: T;
          code?: string;
          message?: string;
        };
        try {
          frame = JSON.parse(line) as typeof frame;
        } catch {
          continue; // a partial or stray line is not worth failing the whole job over
        }
        if (frame.type === 'progress') {
          onProgress?.({ done: frame.done ?? 0, total: frame.total ?? 0 });
        } else if (frame.type === 'error') {
          throw new ApiError(frame.message ?? 'That did not work.', frame.code ?? 'failed', 502);
        } else if (frame.type === 'done') {
          result = frame.data as T;
        }
      }
    };

    try {
      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        buffered += decoder.decode(value, { stream: true });
        readFrames();
      }
      readFrames();
    } catch (error) {
      if (error instanceof ApiError) throw error;
      if ((error as Error).name === 'AbortError') throw new ApiError('Cancelled.', 'aborted', 0);
      throw new ApiError('The connection dropped partway through.', 'network', 0);
    }

    if (result === undefined) {
      throw new ApiError('The server stopped without finishing.', 'incomplete', 502);
    }
    return result;
  },

  /** Multipart upload with real progress events, which `fetch` cannot report. */
  upload: <T>(
    path: string,
    file: File,
    fields: Record<string, string> = {},
    onProgress?: (percent: number) => void,
    signal?: AbortSignal,
  ) =>
    new Promise<T>((resolve, reject) => {
      const form = new FormData();
      for (const [key, value] of Object.entries(fields)) form.append(key, value);
      form.append('file', file, file.name);

      const xhr = new XMLHttpRequest();
      xhr.open('POST', path, true);
      xhr.responseType = 'text';

      xhr.upload.addEventListener('progress', (event) => {
        if (event.lengthComputable && onProgress) {
          onProgress(Math.round((event.loaded / event.total) * 100));
        }
      });

      xhr.addEventListener('load', () => {
        if (xhr.status >= 200 && xhr.status < 300) {
          onProgress?.(100);
          try {
            resolve((JSON.parse(xhr.responseText) as { data: T }).data);
          } catch {
            reject(new ApiError('The upload finished but the response was unreadable.', 'bad_response', xhr.status));
          }
        } else {
          let message = 'The upload failed. Please try again.';
          let code = 'upload_failed';
          try {
            const body = JSON.parse(xhr.responseText) as { error?: { message?: string; code?: string } };
            message = body.error?.message ?? message;
            code = body.error?.code ?? code;
          } catch {
            /* keep defaults */
          }
          reject(new ApiError(message, code, xhr.status));
        }
      });

      xhr.addEventListener('error', () =>
        reject(new ApiError('The upload was interrupted. Check your connection.', 'network', 0)),
      );
      xhr.addEventListener('abort', () =>
        reject(new ApiError('Upload cancelled.', 'aborted', 0)),
      );

      signal?.addEventListener('abort', () => xhr.abort(), { once: true });
      xhr.send(form);
    })};

/** Turn any thrown value into a message safe to show a user. */
export function errorMessage(error: unknown, fallback = 'Something went wrong. Please try again.'): string {
  if (error instanceof ApiError) return error.message;
  if (error instanceof Error && error.message) return error.message;
  return fallback;
}
