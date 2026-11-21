/**
 * The code blocks of a body that is only shown, once it is in the page: the
 * name of the language, a copy button, the numbers of the lines when the
 * reader has asked for them, and the colours.
 *
 * `renderBody` draws a code block as the editor stores it, `<pre><code>`
 * with the text, and leaves the rest to this, which is fetched only when a
 * body has a code block (code-later.ts). Each block is put in a frame
 * (`doc-code`) with a bar above the text; the style sheet keeps the room for
 * the bar on every `<pre>` of a body from the start, so nothing moves when
 * it comes. The colours come when the highlighter has them
 * (code-highlighter.ts): at once for a text it has coloured before, a moment
 * later otherwise. Until then, and for a language it does not know, the text
 * is plain, as it was before there were colours. A diagram is not touched.
 *
 * Built from elements and text nodes only, as `renderBody` is. It imports no
 * messages and nothing of the framework (the words of its button are given):
 * fetched on its own, it would otherwise split what the first screen loads
 * of them into more files.
 */
import { copyControls, type CopyControls, type CopyTexts } from './code-copy.ts';
import { knownTokens, tokensFor } from './code-highlighter.ts';
import { CODE_BLOCK_SELECTOR } from './code-later.ts';
import { codeLanguageOf } from './code-languages.ts';
import { codeLinesShown, onCodeLinesChange } from './code-lines.ts';
import './code-block.css';
import { codeBar, codeFrame, paintCode } from './code-paint.ts';
import type { CodeTokens } from './code-tokens.ts';

/** The stored language of a block, from the class `renderBody` gives its `<code>`. */
function storedLanguage(code: HTMLElement): string | undefined {
  for (const name of code.classList) if (name.startsWith('language-')) return name.slice(9);
  return undefined;
}

interface Shown {
  pre: HTMLElement;
  frame: HTMLElement;
  code: HTMLElement;
  text: string;
  /** What `renderBody` put in the `<code>`, to put back. */
  drawn: Node[];
  tokens: CodeTokens;
  copy: CopyControls;
}

function selectText(code: HTMLElement): void {
  const selection = code.ownerDocument.getSelection();
  selection?.selectAllChildren(code);
}

function show(page: Document, code: HTMLElement, texts: CopyTexts): Shown | undefined {
  const pre = code.parentElement;
  if (!pre || pre.parentElement?.classList.contains('doc-code')) return undefined;
  const text = code.textContent;
  const stored = storedLanguage(code);
  const named = page.createElement('span');
  named.setAttribute('class', 'doc-code-name');
  if (stored !== undefined)
    named.append(page.createTextNode(codeLanguageOf(stored)?.label ?? stored));
  const copy = copyControls(page, {
    texts,
    text: () => text,
    select: () => {
      selectText(code);
    },
  });
  const drawn = [...code.childNodes];
  const place = page.createComment('');
  pre.replaceWith(place);
  const frame = codeFrame(page, codeBar(page, named, copy.status, copy.button), pre);
  // The block has the digits of its numbers from `renderBody`, which drew it.
  place.replaceWith(frame);
  return { pre, frame, code, text, drawn, tokens: [], copy };
}

/**
 * Draws the code blocks under `root`, a body that `renderBody` drew, with
 * `texts` for their copy buttons (code-copy-texts.ts). Returns what undoes
 * it: call it before the body is replaced or removed.
 */
export function drawCodeBlocks(root: HTMLElement, texts: CopyTexts): () => void {
  const page = root.ownerDocument;
  const shown = Array.from(root.querySelectorAll<HTMLElement>(CODE_BLOCK_SELECTOR)).flatMap(
    (code) => show(page, code, texts) ?? [],
  );
  const stop = new AbortController();
  const paint = (block: Shown): void => {
    paintCode(block.code, block.text, block.tokens, codeLinesShown(page));
  };
  for (const block of shown) {
    const language = codeLanguageOf(storedLanguage(block.code));
    const now = language ? knownTokens(language, block.text) : null;
    block.tokens = now ?? [];
    paint(block);
    if (!language || now !== undefined) continue;
    void tokensFor(language, block.text, stop.signal).then((tokens) => {
      if (stop.signal.aborted || !tokens) return;
      block.tokens = tokens;
      paint(block);
    });
  }
  const stopWatching = onCodeLinesChange(page, () => {
    for (const block of shown) paint(block);
  });
  return () => {
    stop.abort();
    stopWatching();
    for (const block of shown) {
      block.copy.destroy();
      block.code.replaceChildren(...block.drawn);
      block.frame.replaceWith(block.pre);
    }
  };
}
