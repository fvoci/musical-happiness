/**
 * The keys of a bar of buttons (`role="toolbar"`): the arrows go from one
 * button to the next, Home and End to the first and the last. A button that
 * cannot be pressed now is passed by.
 */

const STEPS: Readonly<Record<string, number>> = { ArrowLeft: -1, ArrowRight: 1 };

/** Moves the focus along the bar `event` came from; true when the key was the bar's. */
export function moveAlongBar(event: KeyboardEvent, bar: HTMLElement): boolean {
  const buttons = [...bar.querySelectorAll<HTMLButtonElement>('button:not(:disabled)')];
  const at = buttons.findIndex((button) => button === event.target);
  if (at === -1 || buttons.length === 0) return false;
  const step = STEPS[event.key];
  let next: number;
  if (step !== undefined) next = (at + step + buttons.length) % buttons.length;
  else if (event.key === 'Home') next = 0;
  else if (event.key === 'End') next = buttons.length - 1;
  else return false;
  event.preventDefault();
  buttons[next]?.focus();
  return true;
}

/** A press that must not take the focus, nor the selection, out of the editor. */
export function keepEditorFocus(event: MouseEvent): void {
  event.preventDefault();
}
