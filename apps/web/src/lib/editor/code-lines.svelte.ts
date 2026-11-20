/**
 * Whether code blocks show the numbers of their lines: the reader's choice,
 * off unless they turn it on (the switch in the settings).
 *
 * It is the reader's, not the document's: nothing of it is stored in a
 * body. It is remembered on the device, as the layout readout is, and not
 * with the account: the account's settings are a list the server keeps, and
 * this is not on it. A reader by a public link, who has chosen nothing on
 * this device, sees no numbers.
 *
 * The choice is put on the root element (`data-code-lines`), where the style
 * sheet shows the numbers of a body that is only shown, and where an editor
 * watches it to draw them (`onCodeLinesChange`).
 */

/** The attribute of the root element that is there while the numbers are shown. */
export const CODE_LINES_ATTRIBUTE = 'data-code-lines';

/** Where the device remembers the choice. */
export const CODE_LINES_STORAGE_KEY = 'fvoci:code-lines';

function remembered(): boolean {
  try {
    return localStorage.getItem(CODE_LINES_STORAGE_KEY) === '1';
  } catch {
    return false;
  }
}

class CodeLines {
  shown = $state(remembered());

  set(shown: boolean): void {
    this.shown = shown;
    try {
      localStorage.setItem(CODE_LINES_STORAGE_KEY, shown ? '1' : '0');
    } catch {
      // Not remembered: it still holds for this visit.
    }
  }
}

export const codeLines = new CodeLines();

/** Starts keeping the root element in step with the choice. Returns the function that stops. */
export function watchCodeLines(page: Document = document): () => void {
  return $effect.root(() => {
    $effect(() => {
      page.documentElement.toggleAttribute(CODE_LINES_ATTRIBUTE, codeLines.shown);
    });
  });
}

/** Whether the numbers are shown on this page now. */
export const codeLinesShown = (page: Document): boolean =>
  page.documentElement.hasAttribute(CODE_LINES_ATTRIBUTE);

/** Calls back with the new answer whenever the numbers are turned on or off. Returns what stops it. */
export function onCodeLinesChange(page: Document, callback: (shown: boolean) => void): () => void {
  if (typeof MutationObserver === 'undefined') return () => undefined;
  let last = codeLinesShown(page);
  const observer = new MutationObserver(() => {
    const now = codeLinesShown(page);
    if (now === last) return;
    last = now;
    callback(now);
  });
  observer.observe(page.documentElement, {
    attributes: true,
    attributeFilter: [CODE_LINES_ATTRIBUTE],
  });
  return () => {
    observer.disconnect();
  };
}
