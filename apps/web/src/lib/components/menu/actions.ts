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
   * The line is a link to this address. A click or Enter that asks for a new
   * tab or window (Ctrl, ⌘ or Shift held, the middle button) is the
   * browser's; any other click, and Enter, runs `run` in place of following it.
   */
  href?: string;
  run: () => void;
  /** Commands about the line's thing, at its end, that leave the menu open: moving it, say. */
  tools?: readonly MenuTool[];
}

/**
 * A command at the end of a line, shown as its icon and named by `label`,
 * that leaves the menu open. Its keys run it from its line, where the focus
 * stays, wherever the line goes; the button is for a pointer, and in a sheet
 * also for Tab, the focus then staying with the line's tools.
 */
export interface MenuTool {
  /** Names the tool among its line's; also `data-action` in the markup, for tests. */
  id: string;
  label: string;
  icon: Component<{ class?: string }>;
  /** The keys that run it from its line, as `aria-keyshortcuts` writes them: `Alt+ArrowUp`. */
  keys: string;
  disabled?: boolean;
  run: () => void;
}

/** A click on a link that the page answers itself: the main button, and no key held. */
export function followedHere(event: MouseEvent): boolean {
  return event.button === 0 && !event.metaKey && !event.ctrlKey && !event.shiftKey && !event.altKey;
}
