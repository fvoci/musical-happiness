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
import type { SuggestionEntry, SuggestionRenderer } from './extensions.ts';

export interface SuggestionListOptions {
  /** Told apart in the page by `data-suggestions`. */
  kind: string;
  /** The accessible name of the list. */
  label: () => string;
}

let lists = 0;

export function suggestionList(options: SuggestionListOptions): NonNullable<SuggestionRenderer> {
  return () => {
    let list: HTMLDivElement | undefined;
    let entries: SuggestionEntry[] = [];
    let active = 0;
    let choose: (entry: SuggestionEntry) => void = () => undefined;
    let owner: HTMLElement | undefined;
    const id = `doc-suggestions-${(lists += 1)}`;

    function place(rect: DOMRect | null | undefined): void {
      if (!list || !rect) return;
      const view = list.ownerDocument.defaultView;
      const width = view?.innerWidth ?? 0;
      const height = view?.innerHeight ?? 0;
      const box = list.getBoundingClientRect();
      // Under the cursor's line; above it when there is no room below. Never past an edge.
      const below = rect.bottom + 4;
      const top = below + box.height > height && rect.top - 4 - box.height >= 0;
      list.style.left = `${Math.max(4, Math.min(rect.left, width - box.width - 4))}px`;
      list.style.top = `${top ? rect.top - 4 - box.height : below}px`;
    }

    function draw(rect: DOMRect | null | undefined): void {
      if (!list) return;
      const page = list.ownerDocument;
      list.replaceChildren(
        ...entries.map((entry, index) => {
          const option = page.createElement('div');
          option.setAttribute('role', 'option');
          option.setAttribute('id', `${id}-${index}`);
          option.setAttribute('aria-selected', String(index === active));
          option.dataset.value = entry.value;
          option.textContent = entry.label;
          // `mousedown`, and prevented: a click must not take the focus out of the editor.
          option.addEventListener('mousedown', (event) => {
            event.preventDefault();
            choose(entry);
          });
          return option;
        }),
      );
      list.hidden = entries.length === 0;
      if (entries.length > 0) owner?.setAttribute('aria-activedescendant', `${id}-${active}`);
      else owner?.removeAttribute('aria-activedescendant');
      place(rect);
    }

    function close(): void {
      list?.remove();
      list = undefined;
      owner?.removeAttribute('aria-activedescendant');
      owner?.removeAttribute('aria-controls');
    }

    return {
      onStart(props) {
        owner = props.editor.view.dom;
        const page = owner.ownerDocument;
        list = page.createElement('div');
        list.setAttribute('role', 'listbox');
        list.setAttribute('id', id);
        list.setAttribute('aria-label', options.label());
        list.setAttribute('class', 'doc-suggestions');
        list.dataset.suggestions = options.kind;
        page.body.append(list);
        owner.setAttribute('aria-controls', id);
        entries = props.items;
        active = 0;
        choose = props.command;
        draw(props.clientRect?.());
      },
      onUpdate(props) {
        entries = props.items;
        active = Math.min(active, Math.max(0, entries.length - 1));
        choose = props.command;
        draw(props.clientRect?.());
      },
      onKeyDown({ event }) {
        if (!list || entries.length === 0) return false;
        if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
          const step = event.key === 'ArrowDown' ? 1 : -1;
          active = (active + step + entries.length) % entries.length;
          draw(undefined);
          // Not every page can scroll an element into view (a simulated one cannot).
          const chosen: Partial<Element> | null = list.querySelector('[aria-selected="true"]');
          chosen?.scrollIntoView?.({ block: 'nearest' });
          return true;
        }
        if (event.key === 'Enter' || event.key === 'Tab') {
          const entry = entries[active];
          if (entry) choose(entry);
          return true;
        }
        if (event.key === 'Escape') {
          close();
          return true;
        }
        return false;
      },
      onExit() {
        close();
      },
    };
  };
}
