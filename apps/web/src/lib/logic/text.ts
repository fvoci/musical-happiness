/** Helpers that work on plain text. */

/** Adds `https://` to an address that has no scheme. */
export function ensureProtocol(url: string): string {
  return /^.+:\/\//.test(url) ? url : `https://${url}`;
}

const ILLEGAL_IN_FILE_NAMES = ['\\', '/', '?', ':', '<', '>', '*', '|', '"', '.'];

/**
 * A name for a downloaded file, made from a title.
 *
 * QUIRK(Q7): only the first occurrence of each character is removed, so
 * `a.b.c` becomes `ab.c`. Kept as in the original.
 */
export function sanitizeFilename(filename: string): string {
  let sanitized = filename;
  for (const character of ILLEGAL_IN_FILE_NAMES) sanitized = sanitized.replace(character, '');
  return sanitized;
}

export function htmlEncode(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

export function htmlDecode(text: string): string {
  return text
    .replace(/&amp;/g, '&')
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&quot;/g, '"');
}

/** A 32-bit hash of a text. For picking a colour or an icon, not for security. */
export function hashCode(text: string): number {
  let hash = 0;
  for (let index = 0; index < text.length; index += 1) {
    hash = (Math.imul(31, hash) + text.charCodeAt(index)) | 0;
  }
  return hash;
}

const FENCE = /^\s{0,3}(```|~~~)/;
const TASK_ITEM = /^\s*(?:[-+*]|\d{1,9}[.)])\s+\[([ xX])\]\s/;

/**
 * Counts the task list items of a Markdown text (`- [ ] open`, `- [x] done`).
 * Lines inside a fenced code block do not count.
 */
export function countCheckboxesInMarkdown(text: string): { total: number; checked: number } {
  let total = 0;
  let checked = 0;
  let inFence = false;
  for (const line of text.split('\n')) {
    if (FENCE.test(line)) {
      inFence = !inFence;
      continue;
    }
    if (inFence) continue;
    const item = TASK_ITEM.exec(line);
    if (!item) continue;
    total += 1;
    if (item[1] !== ' ') checked += 1;
  }
  return { total, checked };
}
