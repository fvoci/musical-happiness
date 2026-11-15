/**
 * The keys of the search: ⌘K or Ctrl+K open it, from anywhere on the page,
 * inside an editor too. On a Mac both are taken, as the reader of a PC's
 * keyboard on one presses Ctrl.
 *
 * A press something under it already took (`defaultPrevented`: an editor
 * whose own ⌘K makes a link of what is chosen) is left to it, and so is one
 * while letters are being put together by an input method (`isComposing`).
 */
import { isComposing } from '$lib/keys.ts';

/**
 * Whether a key press opens the search. By the letter the key gives; a key
 * that gives a letter of another script (a Korean layout gives ㅏ) by where
 * it is, the K of a Latin keyboard. A key held down asks again: the one who
 * listens takes the repeats too while the search is open, or the browser's
 * own Ctrl+K gets them.
 */
export function opensSearch(event: KeyboardEvent): boolean {
  if (event.defaultPrevented || isComposing(event)) return false;
  if (!(event.ctrlKey || event.metaKey) || event.altKey || event.shiftKey) return false;
  const key = event.key.toLowerCase();
  if (key === 'k') return true;
  return !/^[a-z]$/.test(key) && event.code === 'KeyK';
}

/** What a key pressed in the field asks of the list of what was found. */
export type ListMove = 'next' | 'previous' | 'go' | 'goNewTab';

/**
 * What a key in the field of the search does to the list. Nothing while an
 * input method puts letters together: its Enter takes the letters, and its
 * arrows move among the candidates (`isComposing`).
 */
export function listMove(event: KeyboardEvent): ListMove | undefined {
  if (isComposing(event) || event.altKey) return undefined;
  const command = event.ctrlKey || event.metaKey;
  if (event.key === 'Enter') return command ? 'goNewTab' : event.shiftKey ? undefined : 'go';
  if (command || event.shiftKey) return undefined;
  if (event.key === 'ArrowDown') return 'next';
  if (event.key === 'ArrowUp') return 'previous';
  return undefined;
}

/** The keys that open the search as a device's keyboard names them: ⌘K on Apple's, else Ctrl+K. */
export function searchKeysLabel(userAgent: string): string {
  return /Mac|iPhone|iPad/.test(userAgent) ? '⌘K' : 'Ctrl+K';
}
