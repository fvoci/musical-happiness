/**
 * Requests to the server's API (`contracts/openapi.json`): its paths, the
 * header a changing request needs, and its errors as `BackendError`.
 *
 * The session is the cookie the server set at login. The browser sends it by
 * itself on a request to the page's own origin; a script never sees it. A
 * changing request goes in `fetch`'s default mode, `cors`: the server sends
 * its pages with `Referrer-Policy: no-referrer`, and under that policy any
 * other mode writes `Origin: null`, which the server refuses (the contract's
 * preamble, "Requests that change something").
 */
import {
  BackendError,
  ERROR_STATUSES,
  type ErrorStatus,
  type InvalidInput,
  type Problem,
} from '../state/backend.ts';

import type { paths } from './contract.ts';

/** The paths of the contract, as they are written there. */
export type ApiPath = keyof paths;

/** The media type of the server's error answers (RFC 9457). */
const PROBLEM = 'application/problem+json';

/** The status an error answer is told by. One the boundary does not know is read as the server's own failure. */
function errorStatus(status: number): ErrorStatus {
  return (ERROR_STATUSES as readonly number[]).includes(status) ? (status as ErrorStatus) : 500;
}

const isText = (value: unknown): value is string => typeof value === 'string';
const isCount = (value: unknown): value is number => typeof value === 'number';

/** The entries of `errors` that say where they are and why; anything else is left out. */
function invalidInputs(value: unknown): InvalidInput[] {
  if (!Array.isArray(value)) return [];
  const entries: InvalidInput[] = [];
  for (const item of value as unknown[]) {
    if (typeof item !== 'object' || item === null) continue;
    const { pointer, parameter, header, detail, reason } = item as Record<string, unknown>;
    if (!isText(reason)) continue;
    entries.push({
      ...(isText(pointer) ? { pointer } : {}),
      ...(isText(parameter) ? { parameter } : {}),
      ...(isText(header) ? { header } : {}),
      detail: isText(detail) ? detail : '',
      reason,
    });
  }
  return entries;
}

/**
 * The server's problem details of an error answer. An answer that is not one
 * (a proxy's page in the server's place, say) has only its status to go by.
 */
async function problemOf(response: Response): Promise<Partial<Problem> | undefined> {
  if (!(response.headers.get('Content-Type') ?? '').startsWith(PROBLEM)) return undefined;
  let body: unknown;
  try {
    body = await response.json();
  } catch {
    return undefined;
  }
  if (typeof body !== 'object' || body === null) return undefined;
  const { type, title, detail, reason, errors, limit, retryAfter, requestId } = body as Record<
    string,
    unknown
  >;
  return {
    ...(isText(type) ? { type } : {}),
    ...(isText(title) ? { title } : {}),
    ...(isText(detail) ? { detail } : {}),
    ...(isText(reason) ? { reason } : {}),
    errors: invalidInputs(errors),
    ...(isCount(limit) ? { limit } : {}),
    ...(isCount(retryAfter) ? { retryAfter } : {}),
    ...(isText(requestId) ? { requestId } : {}),
  };
}

async function errorOf(response: Response): Promise<BackendError> {
  return new BackendError(errorStatus(response.status), await problemOf(response));
}

export interface RequestOptions {
  method?: 'GET' | 'POST' | 'PATCH' | 'PUT' | 'DELETE';
  /** Query parameters; an undefined one is left out. */
  query?: Record<string, string | number | undefined>;
  /** Sent as JSON. */
  json?: unknown;
  /** Sent as they are (`application/octet-stream`), in place of `json`. */
  bytes?: Blob;
  /** More headers. */
  headers?: Record<string, string>;
  /** Gives the request up; it then rejects with the signal's reason. */
  signal?: AbortSignal;
}

/** Talks to one server: the one the page came from, under the path the app is served at. */
export class Http {
  /** The path the app is served under, without a trailing slash: `''` at the root. */
  readonly basePath: string;

  constructor(basePath: string) {
    this.basePath = basePath.replace(/\/$/, '');
  }

  /** The address of an API path, its `{name}` parts filled in from `params`. */
  url(path: ApiPath, params: Record<string, string> = {}, query: RequestOptions['query'] = {}) {
    const filled = path.replace(/\{(\w+)\}/g, (_, name: string) => {
      const value = params[name];
      if (value === undefined) throw new Error(`no value for {${name}} in ${path}`);
      return encodeURIComponent(value);
    });
    const search = new URLSearchParams();
    for (const [name, value] of Object.entries(query)) {
      if (value !== undefined) search.set(name, String(value));
    }
    const text = search.toString();
    return `${this.basePath}${filled}${text ? `?${text}` : ''}`;
  }

  /** Reads the bytes at a path (a file). Rejects as `request` does. */
  async bytes(path: ApiPath, params: Record<string, string> = {}): Promise<Blob> {
    let response: Response;
    try {
      response = await fetch(this.url(path, params), { credentials: 'same-origin' });
    } catch {
      throw new BackendError(0, { detail: `GET ${path}: no answer` });
    }
    if (!response.ok) throw await errorOf(response);
    return response.blob();
  }

  /**
   * Sends a request and reads the answer: its JSON, or undefined for 204.
   * Rejects with a `BackendError`: the server's problem details, or status 0
   * when no answer came.
   */
  async request<T>(
    path: ApiPath,
    params: Record<string, string> = {},
    options: RequestOptions = {},
  ): Promise<T> {
    const method = options.method ?? 'GET';
    const headers: Record<string, string> = { Accept: `application/json, ${PROBLEM}` };
    // Every request but a reading one needs it; with it, a page of another
    // origin cannot make the browser send one (contract, "Requests that change something").
    if (method !== 'GET') headers['X-Requested-With'] = 'XMLHttpRequest';
    Object.assign(headers, options.headers);
    let body: string | Blob | undefined;
    if (options.bytes !== undefined) {
      headers['Content-Type'] = 'application/octet-stream';
      body = options.bytes;
    } else if (options.json !== undefined) {
      headers['Content-Type'] = 'application/json';
      body = JSON.stringify(options.json);
    }
    let response: Response;
    try {
      response = await fetch(this.url(path, params, options.query), {
        method,
        headers,
        body,
        credentials: 'same-origin',
        signal: options.signal,
      });
    } catch (error) {
      if (options.signal?.aborted) throw error;
      throw new BackendError(0, { detail: `${method} ${path}: no answer` });
    }
    if (!response.ok) throw await errorOf(response);
    if (response.status === 204) return undefined as T;
    try {
      return (await response.json()) as T;
    } catch {
      // Something between answered in the server's place (a proxy's page, say).
      throw new BackendError(0, { detail: `${method} ${path}: not the server's answer` });
    }
  }
}
