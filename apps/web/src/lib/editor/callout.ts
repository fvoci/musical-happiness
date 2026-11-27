/**
 * The callout of a body: blocks set apart under an icon and a colour that
 * say what they are, a note, a tip, a warning or a danger (its tone, the
 * one attribute the document holds of it, `schema.ts`).
 *
 * A callout stands where a table does, in the document itself and in what
 * a toggle hides, and holds blocks, as a quote does: no table, toggle or
 * callout. A pasted one of a tone not on the list is a note.
 *
 * Where the body is edited the icon is a button: it shows the four tones,
 * each a button that gives the callout its tone (Escape, or a press
 * elsewhere, closes them). Where it can only be read the icon is a picture.
 */
import { mergeAttributes, Node as BodyNode, type NodeViewRenderer } from '@tiptap/core';
import type { EditorView } from '@tiptap/pm/view';

import { m } from '$lib/i18n/paraglide/messages.js';

import { toneIcon } from './block-icons.ts';
import { toneName, toneOf } from './callout-tones.ts';
import { CALLOUT_TONES, DEFAULT_TONE, isCalloutTone, type CalloutTone } from './schema-rules.ts';
import { DOCUMENT_ONLY } from './table.ts';

export const CALLOUT = 'callout';

declare module '@tiptap/core' {
  interface Commands<ReturnType> {
    callout: {
      /** Wraps the blocks of the selection in a callout, or takes them out of one. */
      toggleCallout: (tone?: CalloutTone) => ReturnType;
    };
  }
}

function toneButton(page: Document, tone: CalloutTone): HTMLButtonElement {
  const button = page.createElement('button');
  button.type = 'button';
  button.className = 'doc-callout-tone';
  button.dataset.tone = tone;
  button.setAttribute('aria-label', toneName(tone));
  button.title = toneName(tone);
  button.append(toneIcon(page, tone));
  return button;
}

/** The node view: the icon's button, the tones it shows, and the blocks. */
const calloutView: NodeViewRenderer = ({ node, HTMLAttributes, view, getPos }) => {
  const page = view.dom.ownerDocument;
  const dom = page.createElement('div');
  for (const [name, value] of Object.entries(HTMLAttributes))
    if (typeof value === 'string') dom.setAttribute(name, value);
  dom.classList.add('doc-callout');
  dom.setAttribute('data-callout', '');
  const icon = page.createElement('button');
  icon.type = 'button';
  icon.className = 'doc-callout-icon';
  icon.setAttribute('aria-haspopup', 'true');
  const tones = page.createElement('div');
  tones.className = 'doc-callout-tones';
  tones.setAttribute('role', 'group');
  tones.setAttribute('aria-label', m.editor_callout_tones());
  tones.hidden = true;
  const buttons = CALLOUT_TONES.map((tone) => toneButton(page, tone));
  tones.append(...buttons);
  const controls = page.createElement('div');
  controls.className = 'doc-callout-controls';
  controls.contentEditable = 'false';
  controls.append(icon, tones);
  const contentDOM = page.createElement('div');
  contentDOM.className = 'doc-callout-content';
  dom.append(controls, contentDOM);

  let tone = toneOf(node.attrs.tone);
  const show = (): void => {
    dom.dataset.tone = tone;
    icon.replaceChildren(toneIcon(page, tone));
    icon.setAttribute('aria-label', m.editor_callout_change({ tone: toneName(tone) }));
    for (const button of buttons)
      button.setAttribute('aria-pressed', String(button.dataset.tone === tone));
  };
  const outside = (event: Event): void => {
    if (!(event.target instanceof Node) || !controls.contains(event.target)) close();
  };
  const close = (): void => {
    tones.hidden = true;
    icon.setAttribute('aria-expanded', 'false');
    page.removeEventListener('pointerdown', outside, true);
  };
  const openTones = (): void => {
    tones.hidden = false;
    icon.setAttribute('aria-expanded', 'true');
    page.addEventListener('pointerdown', outside, true);
  };
  show();
  close();

  controls.addEventListener('mousedown', (event) => {
    event.preventDefault();
  });
  icon.addEventListener('click', () => {
    if (!view.editable) return;
    if (tones.hidden) openTones();
    else close();
  });
  tones.addEventListener('click', (event) => {
    const chosen = event.target instanceof Element ? event.target.closest('button') : null;
    const next = chosen?.dataset.tone;
    close();
    if (isCalloutTone(next)) setTone(view, getPos(), next);
  });
  controls.addEventListener('keydown', (event) => {
    if (event.key !== 'Escape' || tones.hidden) return;
    event.preventDefault();
    close();
    icon.focus();
  });

  return {
    dom,
    contentDOM,
    update: (next) => {
      if (next.type.name !== CALLOUT) return false;
      tone = toneOf(next.attrs.tone);
      show();
      return true;
    },
    stopEvent: (event) => event.target instanceof Node && controls.contains(event.target),
    ignoreMutation: (mutation) =>
      mutation.type !== 'selection' &&
      (!contentDOM.contains(mutation.target) ||
        (mutation.type === 'attributes' && mutation.target === contentDOM)),
    destroy: close,
  };
};

/** Gives the callout at `at` a tone: a change of the document, undone as one. */
function setTone(view: EditorView, at: number | undefined, tone: CalloutTone): void {
  const callout = at === undefined ? null : view.state.doc.nodeAt(at);
  if (at === undefined || callout?.type.name !== CALLOUT || callout.attrs.tone === tone) return;
  view.dispatch(view.state.tr.setNodeMarkup(at, undefined, { ...callout.attrs, tone }));
  view.focus();
}

export const Callout = BodyNode.create({
  name: CALLOUT,
  group: DOCUMENT_ONLY,
  content: 'block+',
  defining: true,

  addAttributes() {
    return {
      tone: {
        default: DEFAULT_TONE,
        parseHTML: (element: HTMLElement) => toneOf(element.getAttribute('data-tone')),
        renderHTML: (attributes: { tone?: unknown }) => ({ 'data-tone': attributes.tone }),
      },
    };
  },

  parseHTML() {
    return [{ tag: 'div[data-callout]' }];
  },

  renderHTML({ HTMLAttributes }) {
    return [
      'div',
      mergeAttributes(HTMLAttributes, { 'data-callout': '', class: 'doc-callout' }),
      0,
    ];
  },

  addNodeView() {
    return calloutView;
  },

  addCommands() {
    return {
      toggleCallout:
        (tone = DEFAULT_TONE) =>
        ({ commands }) =>
          commands.toggleWrap(CALLOUT, { tone }),
    };
  },
});
