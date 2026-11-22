/**
 * The whole of a name that is shown cut short (`truncate`), as its tooltip:
 * only while it is cut. A tooltip of the very words shown tells the eye
 * nothing, and some screen readers read it after the name, twice over.
 *
 * Use: `{@attach cutTitle(label)}` on the element the pointer rests on; the
 * text measured is its `.truncate` part, or the element itself. It is
 * measured as the pointer comes over it, before a tooltip shows: a width
 * that changed meanwhile (the window, the name) is measured then.
 */
import type { Attachment } from 'svelte/attachments';
import { on } from 'svelte/events';

export function cutTitle(label: string): Attachment<HTMLElement> {
  return (element) => {
    const measure = (): void => {
      const text = element.querySelector<HTMLElement>('.truncate') ?? element;
      if (text.scrollWidth > text.clientWidth) element.title = label;
      else element.removeAttribute('title');
    };
    return on(element, 'pointerenter', measure);
  };
}
