/** Where the focus is, when the answer is "nowhere", and moving it onto what was just revealed. */

/**
 * Whether the focus is on nothing: on the page itself, or on a box around `home` that only
 * holds it. Safari does not focus a link or a button that a mouse presses, and leaves the
 * focus on the nearest box around it that can hold one; when what was pressed then goes away,
 * the focus is no more anywhere than it is on the page in the other engines. `home` is where
 * the focus would be sent.
 */
export function focusAdrift(home: Element | null | undefined): boolean {
  const active = document.activeElement;
  if (!active || active === document.body) return true;
  if (!home || active === home || !(active instanceof HTMLElement)) return false;
  return active.tabIndex < 0 && active.contains(home);
}

/**
 * Moves the focus to an element that was just unhidden (`visibility: hidden`) or freed from
 * `inert`. WebKit skips style recalculation inside `focus()` while any `<link rel="stylesheet">`
 * (such as a lazy font sheet) is pending, so `isFocusable()` still sees the stale hidden or
 * inert style and drops focus to `document.body`. Calling `getBoundingClientRect()` forces
 * style and layout to catch up regardless of pending sheets.
 */
export function focusRevealed(
  element: HTMLElement | null | undefined,
  options?: FocusOptions,
): void {
  if (!element?.isConnected) return;
  element.getBoundingClientRect();
  element.focus(options);
}
