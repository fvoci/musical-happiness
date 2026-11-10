/**
 * What is added when a body is drawn and is no part of the stored document.
 *
 * The editor (extensions.ts) and the read-only view (render.ts) both draw
 * from here, so that a link opens the same way and an image loads the same
 * way wherever a body is shown.
 */

/**
 * A link opens in a new tab, and the page it opens gets no handle on this
 * one and no credit for the link.
 */
export const LINK_ATTRIBUTES = { target: '_blank', rel: 'noopener noreferrer nofollow' } as const;

/** An image is fetched when it comes near the screen and decoded off the main thread. */
export const IMAGE_ATTRIBUTES = { loading: 'lazy', decoding: 'async' } as const;

/**
 * A formula in the page, before and after it is drawn. Both the editor and
 * the read-only view first put this element where the formula goes, with the
 * source as its text and in `data-latex`; the source shows until the formula
 * has been drawn and stays when it cannot be. The markers are the ones the
 * editor library's own formula nodes use, so that a formula copied out of one
 * editor is recognised by another.
 */
export const MATH_SLOTS = {
  inlineMath: { tag: 'span', type: 'inline-math', display: false },
  blockMath: { tag: 'div', type: 'block-math', display: true },
} as const;
export type MathNodeName = keyof typeof MATH_SLOTS;
export const MATH_CLASS = 'doc-math';
export const MATH_SOURCE_ATTRIBUTE = 'data-latex';

/** The attributes of a formula's element, the same wherever it is made. */
export function mathSlotAttributes(name: MathNodeName, latex: string): Record<string, string> {
  return {
    'data-type': MATH_SLOTS[name].type,
    [MATH_SOURCE_ATTRIBUTE]: latex,
    class: MATH_CLASS,
  };
}

/** The class a code block carries when its language is the one of diagrams (`language-mermaid`). */
export const DIAGRAM_CODE_SELECTOR = 'pre > code.language-mermaid';
