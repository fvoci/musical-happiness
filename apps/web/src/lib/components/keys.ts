/** Reading the keyboard. */

/**
 * Whether a key press belongs to an input method that is composing text
 * (Hangul, kana, pinyin). Such a press must not be taken as a command: the
 * Enter that confirms a syllable is not the Enter that saves a field.
 *
 * Safari reports the confirming Enter with `isComposing` already false, and
 * marks it only with the key code 229, so both are checked.
 */
export function isComposing(event: KeyboardEvent): boolean {
  // eslint-disable-next-line @typescript-eslint/no-deprecated -- the only mark Safari leaves on that key press
  return event.isComposing || event.keyCode === 229;
}
