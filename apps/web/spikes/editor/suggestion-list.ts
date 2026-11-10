/**
 * The list an autocomplete shows under the caret. Plain DOM, enough for the
 * spike: the product would draw this with its popover component.
 */
import type { SuggestionEntry, SuggestionRenderer } from '$lib/editor/extensions.ts';

export function suggestionList(kind: string): NonNullable<SuggestionRenderer> {
  return () => {
    let list: HTMLDivElement | undefined;
    let entries: SuggestionEntry[] = [];
    let active = 0;
    let choose: (entry: SuggestionEntry) => void = () => undefined;

    function draw(rect: DOMRect | null | undefined): void {
      if (!list) return;
      list.replaceChildren(
        ...entries.map((entry, index) => {
          const option = document.createElement('div');
          option.setAttribute('role', 'option');
          option.setAttribute('aria-selected', String(index === active));
          option.dataset.value = entry.value;
          option.textContent = entry.label;
          option.className =
            'cursor-pointer rounded-sm px-2 py-1 aria-selected:bg-accent aria-selected:text-accent-foreground';
          option.addEventListener('mousedown', (event) => {
            event.preventDefault();
            choose(entry);
          });
          return option;
        }),
      );
      list.hidden = entries.length === 0;
      if (rect) {
        list.style.left = `${rect.left}px`;
        list.style.top = `${rect.bottom + 4}px`;
      }
    }

    return {
      onStart(props) {
        list = document.createElement('div');
        list.setAttribute('role', 'listbox');
        list.dataset.suggestions = kind;
        list.className =
          'bg-popover text-popover-foreground fixed z-50 min-w-40 rounded-md border p-1 text-sm shadow-md';
        document.body.append(list);
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
        if (entries.length === 0) return false;
        if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
          const step = event.key === 'ArrowDown' ? 1 : -1;
          active = (active + step + entries.length) % entries.length;
          draw(undefined);
          return true;
        }
        if (event.key === 'Enter' || event.key === 'Tab') {
          const entry = entries[active];
          if (entry) choose(entry);
          return true;
        }
        if (event.key === 'Escape') {
          list?.remove();
          list = undefined;
          return true;
        }
        return false;
      },
      onExit() {
        list?.remove();
        list = undefined;
      },
    };
  };
}
