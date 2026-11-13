/** Reading the keyboard. */

/**
 * Whether a key press belongs to an input method that is composing text
 * (Hangul, kana, pinyin). Such a press must not be taken as a command: the
 * Enter that confirms a syllable is not the Enter that saves a field.
 *
 * Safari reports the confirming Enter with `isComposing` already false, and
 * marks it only with the key code 229, so both are checked.
 */
export function isComposing(
  event: Partial<Pick<KeyboardEvent, 'isComposing' | 'keyCode'>>,
): boolean {
  // eslint-disable-next-line @typescript-eslint/no-deprecated -- the only mark Safari leaves on that key press
  return event.isComposing === true || event.keyCode === 229;
}

/** The platform is read when needed, so rendering without a browser is safe. */
export function isApplePlatform(
  platform = typeof navigator === 'undefined' ? '' : navigator.platform,
): boolean {
  return /Mac|iPhone|iPad|iPod/.test(platform);
}

/** Command on Apple devices, Control elsewhere; the other modifier is never an alias. */
export function primaryModifier(
  event: Pick<KeyboardEvent, 'ctrlKey' | 'metaKey'>,
  platform?: string,
): boolean {
  return isApplePlatform(platform)
    ? event.metaKey && !event.ctrlKey
    : event.ctrlKey && !event.metaKey;
}

type ShortcutOptions = { shift?: boolean; alt?: boolean };

/** Native symbols for Apple keyboards; familiar key names elsewhere. */
export function shortcutLabel(
  key: string,
  options: ShortcutOptions = {},
  platform?: string,
): string {
  if (isApplePlatform(platform)) {
    return `${options.alt ? '⌥' : ''}${options.shift ? '⇧' : ''}⌘${key}`;
  }
  return `Ctrl+${options.alt ? 'Alt+' : ''}${options.shift ? 'Shift+' : ''}${key}`;
}

/** aria-keyshortcuts uses key names even where the visible hint uses symbols. */
export function shortcutKeys(
  key: string,
  options: ShortcutOptions = {},
  platform?: string,
): string {
  return `${isApplePlatform(platform) ? 'Meta' : 'Control'}+${options.alt ? 'Alt+' : ''}${options.shift ? 'Shift+' : ''}${key}`;
}

const FIELDS = 'input, textarea, select, [contenteditable]:not([contenteditable="false"])';

/** Whether an event started in something the reader types into. */
export function inField(target: EventTarget | null): boolean {
  return target instanceof Element && target.closest(FIELDS) !== null;
}
