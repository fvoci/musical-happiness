/**
 * The list an autocomplete shows under the cursor: people after `@`, emoji
 * after `:`. Plain DOM, styled by editor.css with the design tokens. This
 * layer sits below the app's popover components and cannot use them.
 *
 * The cursor stays in the editor; the list is driven from there. The arrow
 * keys move through it, Enter or Tab takes the entry, Escape closes it. It
 * is placed again whenever the editor reports the cursor's rectangle anew,
 * which it does on every change and when the window is resized.
 */
import type { SuggestionKeyDownProps, SuggestionProps } from '@tiptap/suggestion';

import type { SuggestionEntry, SuggestionRenderer } from './extensions.ts';

export interface SuggestionListOptions {
  /** Told apart in the page by `data-suggestions`. */
  kind: string;
  /** The accessible name of the list. */
  label: () => string;
}

let lists = 0;

/** One opening of the list, with its DOM, selection and owner kept together. */
class SuggestionList {
  #list: HTMLDivElement | undefined;
  #entries: SuggestionEntry[] = [];
  #active = 0;
  #choose: (entry: SuggestionEntry) => void = () => undefined;
  #owner: HTMLElement | undefined;
  readonly #id = `doc-suggestions-${(lists += 1)}`;
  readonly #options: SuggestionListOptions;

  constructor(options: SuggestionListOptions) {
    this.#options = options;
  }

  #place(rect: DOMRect | null | undefined): void {
    if (!this.#list || !rect) return;
    const view = this.#list.ownerDocument.defaultView;
    const width = view?.innerWidth ?? 0;
    const height = view?.innerHeight ?? 0;
    const box = this.#list.getBoundingClientRect();
    // Under the cursor's line; above it when there is no room below. Never past an edge.
    const below = rect.bottom + 4;
    const top = below + box.height > height && rect.top - 4 - box.height >= 0;
    this.#list.style.left = `${Math.max(4, Math.min(rect.left, width - box.width - 4))}px`;
    this.#list.style.top = `${top ? rect.top - 4 - box.height : below}px`;
  }

  #draw(rect: DOMRect | null | undefined): void {
    if (!this.#list) return;
    const page = this.#list.ownerDocument;
    this.#list.replaceChildren(
      ...this.#entries.map((entry, index) => {
        const option = page.createElement('div');
        option.setAttribute('role', 'option');
        option.setAttribute('id', `${this.#id}-${index}`);
        option.setAttribute('aria-selected', String(index === this.#active));
        option.dataset.value = entry.value;
        option.textContent = entry.label;
        // `mousedown`, and prevented: a click must not take the focus out of the editor.
        option.addEventListener('mousedown', (event) => {
          event.preventDefault();
          const choose = this.#choose;
          choose(entry);
        });
        return option;
      }),
    );
    this.#list.hidden = this.#entries.length === 0;
    if (this.#entries.length > 0)
      this.#owner?.setAttribute('aria-activedescendant', `${this.#id}-${this.#active}`);
    else this.#owner?.removeAttribute('aria-activedescendant');
    this.#place(rect);
  }

  #close(): void {
    this.#list?.remove();
    this.#list = undefined;
    this.#owner?.removeAttribute('aria-activedescendant');
    this.#owner?.removeAttribute('aria-controls');
  }

  onStart = (props: SuggestionProps<SuggestionEntry, SuggestionEntry>): void => {
    this.#owner = props.editor.view.dom;
    const page = this.#owner.ownerDocument;
    this.#list = page.createElement('div');
    this.#list.setAttribute('role', 'listbox');
    this.#list.setAttribute('id', this.#id);
    this.#list.setAttribute('aria-label', this.#options.label());
    this.#list.setAttribute('class', 'doc-suggestions');
    this.#list.dataset.suggestions = this.#options.kind;
    page.body.append(this.#list);
    this.#owner.setAttribute('aria-controls', this.#id);
    this.#entries = props.items;
    this.#active = 0;
    this.#choose = props.command;
    this.#draw(props.clientRect?.());
  };

  onUpdate = (props: SuggestionProps<SuggestionEntry, SuggestionEntry>): void => {
    this.#entries = props.items;
    this.#active = Math.min(this.#active, Math.max(0, this.#entries.length - 1));
    this.#choose = props.command;
    this.#draw(props.clientRect?.());
  };

  onKeyDown = ({ event }: SuggestionKeyDownProps): boolean => {
    if (!this.#list || this.#entries.length === 0) return false;
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      const step = event.key === 'ArrowDown' ? 1 : -1;
      this.#active = (this.#active + step + this.#entries.length) % this.#entries.length;
      this.#draw(undefined);
      // Not every page can scroll an element into view (a simulated one cannot).
      const chosen: Partial<Element> | null = this.#list.querySelector('[aria-selected="true"]');
      chosen?.scrollIntoView?.({ block: 'nearest' });
      return true;
    }
    if (event.key === 'Enter' || event.key === 'Tab') {
      const entry = this.#entries[this.#active];
      const choose = this.#choose;
      if (entry) choose(entry);
      return true;
    }
    if (event.key === 'Escape') {
      this.#close();
      return true;
    }
    return false;
  };

  onExit = (): void => {
    this.#close();
  };
}

export function suggestionList(options: SuggestionListOptions): NonNullable<SuggestionRenderer> {
  return () => new SuggestionList(options);
}
