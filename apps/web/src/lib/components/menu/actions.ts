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
  /** Marks the line as the current choice. */
  checked?: boolean;
  /** Deleting and the like: drawn in the colour of danger. */
  destructive?: boolean;
  /** A line above this one sets it off from what came before. */
  separated?: boolean;
  disabled?: boolean;
  run: () => void;
}
