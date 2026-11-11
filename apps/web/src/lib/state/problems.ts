/**
 * The errors of the data boundary: the problem details of RFC 9457 the
 * server answers with, and the `BackendError` every method of a backend
 * rejects with. `backend.ts` re-exports them.
 */
/** The statuses the server answers an error with (server design 1.3). */
export const ERROR_STATUSES = [400, 401, 403, 404, 405, 409, 412, 413, 416, 429, 500] as const;

export type ErrorStatus = (typeof ERROR_STATUSES)[number];

/**
 * One thing of a request that was refused, and where it is: exactly one of
 * `pointer` (a JSON Pointer into the body, `/username`), `parameter` (of the
 * query or the path) and `header` says.
 */
export interface InvalidInput {
  pointer?: string;
  parameter?: string;
  header?: string;
  /** English, for logs and developers. Never shown as it is. */
  detail: string;
  /** Why, stable: `required`, `too_short`, …, or `invalid` when no finer cause is told. */
  reason: string;
}

/** The body of an error answer: the problem details of RFC 9457, as the contract has them (`Problem`). */
export interface Problem {
  /** The kind of problem, `/problems/<name>`: the same for every problem of the kind. */
  type: string;
  /** The name of the type, in English. */
  title: string;
  status: number;
  /** English, for logs and developers. Never shown as it is. */
  detail: string;
  /** The request, as a URN of its identifier. */
  instance?: string;
  /** A stable reason within the status, for choosing the text to show. */
  reason?: string;
  /** What of the request was refused, one entry for each. */
  errors?: InvalidInput[];
  limit?: number;
  requestId?: string;
  /** Seconds, with 429. */
  retryAfter?: number;
}

/** The problems of the statuses as the server names them: the last part of `type`, and the title. */
const STATUS_PROBLEMS: Record<ErrorStatus, readonly [name: string, title: string]> = {
  400: ['bad-request', 'Bad request'],
  401: ['unauthorized', 'No valid session'],
  403: ['forbidden', 'Forbidden'],
  404: ['not-found', 'Not found'],
  405: ['method-not-allowed', 'Method not allowed'],
  409: ['conflict', 'Conflict'],
  412: ['precondition-failed', 'Precondition failed'],
  413: ['payload-too-large', 'Payload too large'],
  416: ['range-not-satisfiable', 'Range not satisfiable'],
  429: ['rate-limited', 'Too many requests'],
  500: ['internal', 'Internal server error'],
};

/** The problem the server answers with for a status that has no `reason`. */
export function problemOf(status: ErrorStatus, detail: string): Problem {
  const [name, title] = STATUS_PROBLEMS[status];
  return { type: `/problems/${name}`, title, status, detail };
}

/** The entry of `errors` for a member of the request body, by its name. */
export function refusedMember(member: string, detail: string, reason = 'invalid'): InvalidInput {
  return { pointer: `/${member.replaceAll('~', '~0').replaceAll('/', '~1')}`, detail, reason };
}

/** What every method of the backend rejects with. */
export class BackendError extends Error {
  /** 0 when no answer arrived at all. */
  readonly status: ErrorStatus | 0;
  /** The kind of problem; `about:blank` when the answer named none (RFC 9457, 4.2). */
  readonly type: string;
  readonly reason: string | undefined;
  /** What of the request was refused; empty when the error is about nothing of it. */
  readonly errors: readonly InvalidInput[];
  readonly limit: number | undefined;
  readonly retryAfter: number | undefined;
  /** The server's name for the request, to report a failure by. */
  readonly requestId: string | undefined;

  constructor(status: ErrorStatus | 0, problem?: Partial<Problem>) {
    super(problem?.detail ?? problem?.title ?? (status === 0 ? 'no answer' : `status ${status}`));
    this.name = 'BackendError';
    this.status = status;
    this.type = problem?.type ?? 'about:blank';
    this.reason = problem?.reason;
    this.errors = problem?.errors ?? [];
    this.limit = problem?.limit;
    this.retryAfter = problem?.retryAfter;
    this.requestId = problem?.requestId;
  }

  /** The entry of `errors` about the member of the request body at this JSON Pointer, if there is one. */
  refusalAt(pointer: string): InvalidInput | undefined {
    return this.errors.find((entry) => entry.pointer === pointer);
  }
}

export function isBackendError(error: unknown): error is BackendError {
  return error instanceof BackendError;
}

/** The stored version differs from the one the write was based on. */
export function isConflict(error: unknown): error is BackendError {
  return isBackendError(error) && error.status === 409;
}
