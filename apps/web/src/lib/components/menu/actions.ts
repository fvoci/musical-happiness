/** One line of a menu of commands. */
import type { Component } from 'svelte';

export interface MenuAction {
  /** Names the line among its siblings; also `data-action` in the markup, for tests. */
  id: string;
  label: string;
  /** What a screen reader says in place of `label`, where the line means more than it shows. */
  ariaLabel?: string;
  icon?: Component<{ class?: string }>;
  /** Shown at the end of the line: a shortcut, a count, the current choice. */
  hint?: string;
  /** Marks the line as the current choice; a link, as the page that is shown. */
  checked?: boolean;
  /** Deleting and the like: drawn in the colour of danger. */
  destructive?: boolean;
  /** A line above this one sets it off from what came before. */
  separated?: boolean;
  disabled?: boolean;
  /**
   * The line is a link to this address. A click that asks for a new tab or
   * window (a key held, the middle button) is the browser's; any other, and
   * Enter, runs `run` in place of following it.
   */
  href?: string;
  run: () => void;
}

/** A click on a link that the page answers itself: the main button, and no key held. */
export function followedHere(event: MouseEvent): boolean {
  return event.button === 0 && !event.metaKey && !event.ctrlKey && !event.shiftKey && !event.altKey;
}
