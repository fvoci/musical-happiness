import type { Component } from 'svelte';

/** A Lucide icon, or anything else that takes a class. `data-icon` is the button's hint of where it sits. */
export type IconComponent = Component<{ class?: string; 'data-icon'?: string }>;

/** One line of a menu of commands. Choosing it does nothing in the mock-ups but close the menu. */
export interface Action {
  id: string;
  label: string;
  icon: IconComponent;
  /** At the far end of the line: the current value of the setting the line opens. */
  hint?: string;
  /** The line is the current choice. */
  checked?: boolean;
  destructive?: boolean;
  /** A rule above this line. */
  separated?: boolean;
}
