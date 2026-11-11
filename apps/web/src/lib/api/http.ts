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
import { BackendError, ERROR_KINDS, type ErrorBody, type ErrorStatus } from '../state/backend.ts';

import type { paths } from './contract.ts';

/** The paths of the contract, as they are written there. */
export type ApiPath = keyof paths;

/** The status an error answer is told by. One the boundary has no kind for is read as the server's own failure. */
function errorStatus(status: number): ErrorStatus {
  return status in ERROR_KINDS ? (status as ErrorStatus) : 500;
}

async function errorOf(response: Response): Promise<BackendError> {
  let body: Partial<ErrorBody> | undefined;
  try {
    body = (await response.json()) as Partial<ErrorBody>;
  } catch {
    body = undefined;
  }
  return new BackendError(errorStatus(response.status), body);
}

export interface RequestOptions {
  method?: 'GET' | 'POST' | 'PATCH' | 'PUT' | 'DELETE';
  /** Query parameters; an undefined one is left out. */
  query?: Record<string, string | number | undefined>;
  /** Sent as JSON. */
  json?: unknown;
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

  /**
   * Sends a request and reads the answer: its JSON, or undefined for 204.
   * Rejects with a `BackendError`: the server's error body, or `network`
   * when no answer came.
   */
  async request<T>(
    path: ApiPath,
    params: Record<string, string> = {},
    options: RequestOptions = {},
  ): Promise<T> {
    const method = options.method ?? 'GET';
    const headers: Record<string, string> = { Accept: 'application/json' };
    // Every request but a reading one needs it; with it, a page of another
    // origin cannot make the browser send one (contract, "Requests that change something").
    if (method !== 'GET') headers['X-Requested-With'] = 'XMLHttpRequest';
    let body: string | undefined;
    if (options.json !== undefined) {
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
      });
    } catch {
      throw new BackendError(0, { message: `${method} ${path}: no answer` });
    }
    if (!response.ok) throw await errorOf(response);
    if (response.status === 204) return undefined as T;
    try {
      return (await response.json()) as T;
    } catch {
      // Something between answered in the server's place (a proxy's page, say).
      throw new BackendError(0, { message: `${method} ${path}: not the server's answer` });
    }
  }
}
