/**
 * The Word worker's HTML against docx-preview drawing the same document on
 * a real DOM (jsdom): the same elements, attributes, styles and text, in the
 * same order, but for the pictures' addresses.
 */
import { parseAsync, renderDocument } from 'docx-preview';
import { describe, expect, it } from 'vitest';

import { OPTIONS } from '../../src/viewer-frame/docx-draw.ts';
import { koreanDocx } from '../support/viewer-files.ts';
import { drawAsWorker } from '../support/worker-globals.ts';

/** A tree as text: each element with its attributes sorted (and its style as parsed), then its text. */
function canonical(root: Element): string {
  const lines: string[] = [];
  const walk = (node: ChildNode, depth: number): void => {
    const pad = ' '.repeat(depth);
    if (node instanceof Text) {
      if (node.data !== '') lines.push(`${pad}"${node.data}"`);
      return;
    }
    if (!(node instanceof Element)) return;
    const attributes = [...node.attributes]
      .filter((attribute) => attribute.name !== 'style' && attribute.name !== 'src')
      .map((attribute) => `${attribute.name}=${attribute.value}`)
      .sort();
    const style =
      node instanceof HTMLElement || node instanceof SVGElement ? node.style.cssText : '';
    lines.push(`${pad}<${node.tagName.toLowerCase()} ${attributes.join(' ')} {${style}}`);
    for (const child of node.childNodes) walk(child, depth + 1);
  };
  for (const child of root.childNodes) walk(child, 0);
  return lines.join('\n');
}

const parsed = (html: string): Element => {
  const holder = document.createElement('div');
  holder.innerHTML = html;
  return holder;
};

describe('the Word worker draws what docx-preview draws', () => {
  it('the Korean sample, element for element', async () => {
    const file = koreanDocx();
    const nodes = await renderDocument(
      await parseAsync(Uint8Array.from(file).buffer, OPTIONS),
      OPTIONS,
    );
    const isStyle = (node: Node): boolean => node.nodeName === 'STYLE';
    const native = { styles: document.createElement('div'), body: document.createElement('div') };
    native.styles.append(...nodes.filter(isStyle));
    native.body.append(...nodes.filter((node) => !isStyle(node)));

    const answer = await drawAsWorker(file);
    if (answer.kind !== 'drawn') throw new Error(JSON.stringify(answer));
    const drawn = canonical(parsed(answer.body));
    // Pages, header, table, picture: a tree of some size, compared line for line.
    expect(drawn.split('\n').length).toBeGreaterThan(60);
    expect(drawn).toMatch(/<img [^\n]*width: 150pt/);
    expect(drawn).toBe(canonical(parsed(native.body.innerHTML)));
    expect(parsed(answer.styles).textContent).toBe(native.styles.textContent);
  });
});
