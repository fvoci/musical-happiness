/**
 * The messages between the app and the frame the renderers run in.
 *
 * The frame is a document without an origin of its own (`sandbox` without
 * `allow-same-origin`) and without the network. The app hands it a file's
 * bytes and a few commands; the frame answers with what the app shows around
 * it: how many pages there are, which one is in view, that it failed, and the
 * keys the reader pressed while the frame had the focus. Nothing else crosses.
 *
 * Each side reads what arrives with the reader below, which accepts exactly
 * these shapes and nothing more: a message with an extra field, a field of
 * another type or a number out of range is dropped. The app also checks that
 * a message came from its own frame (`event.source`); the frame that it came
 * from its parent. Their origins say nothing: the frame's is `"null"`.
 */
import { isFormatId, type FormatId } from './formats.ts';

export type Theme = 'light' | 'dark';

/**
 * The keys the frame passes on, for the viewer's own commands: the previous
 * and next page, zooming, closing. The others (the arrows up and down, Page
 * Up, Space) scroll the frame as they would any page.
 */
export const VIEWER_KEYS = ['ArrowLeft', 'ArrowRight', '+', '=', '-', '0', 'Escape'] as const;
export type ViewerKey = (typeof VIEWER_KEYS)[number];

/** Why the frame could not draw a file. */
export const FAILURES = ['unreadable', 'locked', 'too-large', 'renderer'] as const;
export type Failure = (typeof FAILURES)[number];

/** The most pages a document is taken to have; a larger count is a broken message. */
export const PAGE_LIMIT = 100_000;
export const ZOOM_MIN = 0.25;
export const ZOOM_MAX = 4;

export type ToFrame =
  | { kind: 'open'; format: FormatId; bytes: ArrayBuffer; theme: Theme; lang: string }
  | { kind: 'page'; page: number }
  | { kind: 'zoom'; zoom: number }
  | { kind: 'theme'; theme: Theme };

export type FromFrame =
  | { kind: 'ready' }
  | { kind: 'shown'; pages: number }
  | { kind: 'page'; page: number }
  | { kind: 'failed'; reason: Failure }
  | { kind: 'key'; key: ViewerKey };

type Fields = Record<string, unknown>;

/** The fields of `data` when it is a plain object with exactly these keys. */
function exactly(data: unknown, keys: readonly string[]): Fields | undefined {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) return undefined;
  if (Object.getPrototypeOf(data) !== Object.prototype) return undefined;
  const own = Object.keys(data);
  if (own.length !== keys.length || !keys.every((key) => own.includes(key))) return undefined;
  return data as Fields;
}

const isPage = (value: unknown): value is number =>
  Number.isInteger(value) && (value as number) >= 1 && (value as number) <= PAGE_LIMIT;

const isTheme = (value: unknown): value is Theme => value === 'light' || value === 'dark';

/** A language tag such as `ko` or `pt-BR`. */
const isLanguage = (value: unknown): value is string =>
  typeof value === 'string' && /^[A-Za-z]{2,3}(?:-[A-Za-z0-9]{1,8}){0,3}$/.test(value);

const isOneOf = <T extends string>(list: readonly T[], value: unknown): value is T =>
  typeof value === 'string' && (list as readonly string[]).includes(value);

function kindOf(data: unknown): unknown {
  return typeof data === 'object' && data !== null ? (data as Fields).kind : undefined;
}

/** A message from the frame, if `data` is exactly one. */
export function readFromFrame(data: unknown): FromFrame | undefined {
  switch (kindOf(data)) {
    case 'ready':
      return exactly(data, ['kind']) ? { kind: 'ready' } : undefined;
    case 'shown': {
      const pages = exactly(data, ['kind', 'pages'])?.pages;
      return isPage(pages) ? { kind: 'shown', pages } : undefined;
    }
    case 'page': {
      const page = exactly(data, ['kind', 'page'])?.page;
      return isPage(page) ? { kind: 'page', page } : undefined;
    }
    case 'failed': {
      const reason = exactly(data, ['kind', 'reason'])?.reason;
      return isOneOf(FAILURES, reason) ? { kind: 'failed', reason } : undefined;
    }
    case 'key': {
      const key = exactly(data, ['kind', 'key'])?.key;
      return isOneOf(VIEWER_KEYS, key) ? { kind: 'key', key } : undefined;
    }
    default:
      return undefined;
  }
}

/** A message from the app, if `data` is exactly one. */
export function readToFrame(data: unknown): ToFrame | undefined {
  switch (kindOf(data)) {
    case 'open': {
      const fields = exactly(data, ['kind', 'format', 'bytes', 'theme', 'lang']);
      if (!fields || !isFormatId(fields.format) || !isTheme(fields.theme)) return undefined;
      if (!(fields.bytes instanceof ArrayBuffer) || !isLanguage(fields.lang)) return undefined;
      const { format, bytes, theme, lang } = fields;
      return { kind: 'open', format, bytes, theme, lang };
    }
    case 'page': {
      const page = exactly(data, ['kind', 'page'])?.page;
      return isPage(page) ? { kind: 'page', page } : undefined;
    }
    case 'zoom': {
      const zoom = exactly(data, ['kind', 'zoom'])?.zoom;
      return typeof zoom === 'number' && zoom >= ZOOM_MIN && zoom <= ZOOM_MAX
        ? { kind: 'zoom', zoom }
        : undefined;
    }
    case 'theme': {
      const theme = exactly(data, ['kind', 'theme'])?.theme;
      return isTheme(theme) ? { kind: 'theme', theme } : undefined;
    }
    default:
      return undefined;
  }
}
