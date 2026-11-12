/**
 * The editing place: where the source of a diagram or of a block formula is
 * changed, opened right under the picture and inside the flow of the body.
 *
 * At most one place is open in an editor. Which one is a state of the screen,
 * kept by this plugin beside the document and never in it: opening and
 * closing a place changes no node, no attribute and no text, and so nothing
 * that is stored. The position of the open block is mapped through every
 * change; a change that takes the block away closes the place.
 *
 * Closed, a diagram is its picture and a block formula its drawing. Moving
 * through the body with the arrow keys selects such a block as a whole
 * (Enter then opens it) and never puts the cursor into the hidden source.
 *
 * - Opens: a press on the picture (the node views), Enter while the block is
 *   selected, and a new diagram (an empty one just made) opens by itself.
 * - Closes: Escape, Ctrl+Enter or Cmd+Enter, a press anywhere else, the
 *   focus going elsewhere, or the cursor leaving the block. Closed by a key,
 *   the block is selected again and the editor has the focus.
 */
import { Extension } from '@tiptap/core';
import type { Node as DocumentNode, ResolvedPos } from '@tiptap/pm/model';
import {
  NodeSelection,
  Plugin,
  PluginKey,
  Selection,
  TextSelection,
  type EditorState,
  type Transaction,
} from '@tiptap/pm/state';
import { closeHistory } from '@tiptap/pm/history';
import { Decoration, DecorationSet, type EditorView } from '@tiptap/pm/view';

import { m } from '$lib/i18n/paraglide/messages.js';

import { diagramKind } from './diagram-kind.ts';
import { DIAGRAM_LANGUAGE } from './schema.ts';

/** A diagram, a block formula, or a block formula that is still being written. */
export type PlaceKind = 'diagram' | 'formula' | 'new-formula';

/** What a block formula holds in its own field, which may differ from the document (empty). */
export interface Draft {
  /** The source as written, trimmed. Set by whoever holds the field. */
  value: () => string;
}

/** The place of a formula that is not in the document yet, drawn on its empty line. */
export interface NewFormulaPanel extends Draft {
  dom: HTMLElement;
  focus: () => void;
}

export interface Place {
  kind: PlaceKind;
  /** Where the block starts: the diagram, the formula, or the empty line a new formula goes into. */
  pos: number;
  /** For a new formula: its place, which the document does not hold yet. */
  panel?: NewFormulaPanel;
  /** For a formula: its field, set by its node view while the place is open. */
  draft?: Draft;
}

type Meta = { open: Place } | { close: true };

export const placeKey = new PluginKey<Place | null>('editingPlace');

export const isDiagramNode = (node: DocumentNode | null | undefined): boolean =>
  node?.type.name === 'codeBlock' && node.attrs.language === DIAGRAM_LANGUAGE;

const isEmptyParagraph = (node: DocumentNode | null | undefined): boolean =>
  node?.type.name === 'paragraph' && node.content.size === 0;

/** Whether the node at a place is still the kind of block the place is for. */
function fits(kind: PlaceKind, node: DocumentNode | null | undefined): boolean {
  if (kind === 'diagram') return isDiagramNode(node);
  if (kind === 'formula') return node?.type.name === 'blockMath';
  return isEmptyParagraph(node);
}

/** The place that is open, or null. */
export const placeOf = (state: EditorState): Place | null => placeKey.getState(state) ?? null;

/** Whether the selection belongs to the open place: inside its block, or on it. */
function selectionStays(place: Place, state: EditorState): boolean {
  const node = state.doc.nodeAt(place.pos);
  if (!node) return false;
  const { from, to } = state.selection;
  if (place.kind === 'formula')
    return state.selection instanceof NodeSelection && from === place.pos;
  return from > place.pos && to < place.pos + node.nodeSize;
}

/** The diagram whose source the position is in, when that diagram is closed. */
function closedDiagramAt(state: EditorState, $position: ResolvedPos): number | undefined {
  if (!isDiagramNode($position.parent) || $position.depth === 0) return undefined;
  const pos = $position.before();
  const place = placeOf(state);
  return place?.kind === 'diagram' && place.pos === pos ? undefined : pos;
}

function nextPlace(value: Place | null, tr: Transaction, state: EditorState): Place | null {
  const meta = tr.getMeta(placeKey) as Meta | undefined;
  if (meta && 'close' in meta) return null;
  let place = meta?.open ?? value;
  if (!place) return null;
  if (!meta) {
    const mapped = tr.mapping.mapResult(place.pos, 1);
    if (mapped.deleted) return null;
    if (mapped.pos !== place.pos) place = { ...place, pos: mapped.pos };
  }
  if (!fits(place.kind, state.doc.nodeAt(place.pos))) return null;
  return selectionStays(place, state) ? place : null;
}

/**
 * What a formula's place leaves in the document when it is closed on
 * purpose: what its field holds, as one step of undo of its own. A formula
 * closed empty is removed; a new one that was written goes in place of its
 * empty line. Gives the position of the formula, if there is one.
 */
function finishFormula(tr: Transaction, place: Place): number | undefined {
  const written = (place.panel ?? place.draft)?.value() ?? '';
  const node = tr.doc.nodeAt(place.pos);
  if (!node) return undefined;
  const { pos } = place;
  if (place.kind === 'formula') {
    if (written === String(node.attrs.latex)) return pos;
    closeHistory(tr);
    if (written === '') {
      tr.delete(pos, pos + node.nodeSize);
      return undefined;
    }
    tr.setNodeAttribute(pos, 'latex', written);
    return pos;
  }
  const formula =
    written === '' ? undefined : tr.doc.type.schema.nodes.blockMath?.create({ latex: written });
  if (!formula || !isEmptyParagraph(node)) return undefined;
  closeHistory(tr);
  tr.replaceWith(pos, pos + node.nodeSize, formula);
  // Somewhere to go on writing when the formula ends what holds it.
  if (tr.doc.resolve(pos + formula.nodeSize).nodeAfter === null) {
    const paragraph = tr.doc.type.schema.nodes.paragraph?.create();
    if (paragraph) tr.insert(pos + formula.nodeSize, paragraph);
  }
  return pos;
}

/**
 * Whether the block at `pos` is the line the cursor was in, made a diagram by
 * typing its fence (three backticks and the language): not a diagram that was
 * already there, nor one that a new document or a paste brought.
 */
function madeHere(transactions: readonly Transaction[], before: EditorState, pos: number): boolean {
  const { $from } = before.selection;
  if ($from.depth === 0 || $from.parent.type.name !== 'paragraph') return false;
  if (!/^(?:```|~~~)/.test($from.parent.textContent)) return false;
  let at = $from.before();
  for (const tr of transactions) {
    const mapped = tr.mapping.mapResult(at, 1);
    if (mapped.deletedAcross) return false;
    at = mapped.pos;
  }
  return at === pos;
}

/**
 * The cursor never rests in the source of a closed diagram: it selects the
 * diagram instead. A diagram that has just been made, empty, opens.
 */
function keepOutOfClosed(
  transactions: readonly Transaction[],
  before: EditorState,
  state: EditorState,
): Transaction | null {
  const { selection } = state;
  if (!(selection instanceof TextSelection)) return null;
  const pos = closedDiagramAt(state, selection.$from);
  if (pos === undefined || !selection.$to.sameParent(selection.$from)) return null;
  const node = state.doc.nodeAt(pos);
  if (!node) return null;
  if (madeHere(transactions, before, pos) && node.content.size === 0)
    return state.tr.setMeta(placeKey, { open: { kind: 'diagram', pos } });
  return state.tr.setSelection(NodeSelection.create(state.doc, pos));
}

/**
 * Opens the place of the block at `pos`: a diagram's with the cursor at the
 * end of its source, a formula's with the focus in its field.
 */
export function openPlace(view: EditorView, pos: number): boolean {
  const node = view.state.doc.nodeAt(pos);
  if (!view.editable || !node) return false;
  if (isDiagramNode(node)) {
    const tr = view.state.tr.setMeta(placeKey, { open: { kind: 'diagram', pos } });
    tr.setSelection(TextSelection.create(tr.doc, pos + node.nodeSize - 1));
    view.dispatch(tr.scrollIntoView());
    view.focus();
    return true;
  }
  if (node.type.name !== 'blockMath') return false;
  const latex = String(node.attrs.latex);
  // Until the formula's field says otherwise, what it holds is the stored source.
  const place: Place = { kind: 'formula', pos, draft: { value: () => latex } };
  const tr = view.state.tr.setMeta(placeKey, { open: place });
  view.dispatch(tr.setSelection(NodeSelection.create(tr.doc, pos)).scrollIntoView());
  return true;
}

/**
 * Closes the place that is open, on purpose: a formula keeps what its field
 * holds. With `focusBlock` (a key, its button) the block is selected and the
 * editor takes the focus; without (a press or the focus elsewhere), the
 * selection and the focus are left to what was pressed. A place that only
 * goes away (its block is deleted, the document replaced, the editor made
 * read-only) is dropped instead (`dropPlace`): nothing is written then.
 */
export function closePlace(view: EditorView, focusBlock: boolean): void {
  const place = placeOf(view.state);
  if (!place) return;
  const tr = view.state.tr.setMeta(placeKey, { close: true });
  const at = place.kind === 'diagram' ? place.pos : finishFormula(tr, place);
  if (focusBlock) {
    if (at !== undefined) tr.setSelection(NodeSelection.create(tr.doc, at));
    else if (place.kind === 'formula')
      tr.setSelection(Selection.near(tr.doc.resolve(Math.min(place.pos, tr.doc.content.size))));
  }
  view.dispatch(tr);
  if (focusBlock) view.focus();
}

/** Closes the place that is open and writes nothing: what a field holds is given up. */
export function dropPlace(view: EditorView): void {
  if (placeOf(view.state)) view.dispatch(view.state.tr.setMeta(placeKey, { close: true }));
}

/** Opens the place of a new formula on the empty line at `pos`, as part of `tr`. */
export function openNewFormula(tr: Transaction, pos: number, panel: NewFormulaPanel): void {
  tr.setMeta(placeKey, { open: { kind: 'new-formula', pos, panel } });
}

const ARROWS: Record<string, { dir: -1 | 1; side: 'up' | 'down' | 'left' | 'right' }> = {
  ArrowUp: { dir: -1, side: 'up' },
  ArrowDown: { dir: 1, side: 'down' },
  ArrowLeft: { dir: -1, side: 'left' },
  ArrowRight: { dir: 1, side: 'right' },
};

/**
 * An arrow key at the edge of a line that would go into a closed diagram
 * selects the diagram. (The browser would skip its hidden source.)
 */
function arrowOntoDiagram(view: EditorView, event: KeyboardEvent): boolean {
  const arrow = ARROWS[event.key];
  if (!arrow || event.shiftKey || event.altKey || event.ctrlKey || event.metaKey) return false;
  const { selection, doc } = view.state;
  if (!(selection instanceof TextSelection) || !selection.empty) return false;
  const { $head } = selection;
  if (!$head.parent.inlineContent || $head.depth === 0) return false;
  if (!view.endOfTextblock(arrow.side)) return false;
  const $start = doc.resolve(arrow.dir > 0 ? $head.after() : $head.before());
  const found = Selection.findFrom($start, arrow.dir);
  const pos = found ? closedDiagramAt(view.state, found.$from) : undefined;
  if (pos === undefined) return false;
  view.dispatch(view.state.tr.setSelection(NodeSelection.create(doc, pos)).scrollIntoView());
  return true;
}

/** Whether a key closes the place: Escape, or Ctrl+Enter or Cmd+Enter. Never one an input method is composing with. */
export const closesPlace = (event: KeyboardEvent): boolean =>
  !event.isComposing &&
  (event.key === 'Escape' || (event.key === 'Enter' && (event.ctrlKey || event.metaKey)));

/**
 * The keys that open and close a place, heard before the editor's own
 * handling: the editor leaves Enter on Android to the change it makes in the
 * page, which a selected diagram does not make.
 */
function placeKeys(view: EditorView, event: KeyboardEvent): boolean {
  if (event.isComposing) return false;
  const place = placeOf(view.state);
  const { selection } = view.state;
  if (place && closesPlace(event)) {
    // The key belongs to the place: it must not also close what the editor is in.
    event.stopPropagation();
    closePlace(view, true);
  } else if (
    !place &&
    event.key === 'Enter' &&
    !event.shiftKey &&
    !event.altKey &&
    !event.ctrlKey &&
    !event.metaKey &&
    selection instanceof NodeSelection &&
    view.editable
  ) {
    if (!openPlace(view, selection.from)) return false;
  } else return false;
  event.preventDefault();
  return true;
}

function handleKeyDown(view: EditorView, event: KeyboardEvent): boolean {
  if (event.isComposing) return false;
  if (
    placeOf(view.state)?.kind === 'diagram' &&
    event.key === 'Enter' &&
    !event.shiftKey &&
    !event.altKey
  ) {
    // A new line, always: a source may have empty lines, and three Enters do not leave it.
    view.dispatch(view.state.tr.insertText('\n').scrollIntoView());
    return true;
  }
  return arrowOntoDiagram(view, event);
}

/** The element a press or the focus has to stay inside for the place to stay open. */
function holderOf(view: EditorView, place: Place): Node | null {
  return place.panel?.dom ?? view.nodeDOM(place.pos);
}

/** What a screen reader is told when a place opens, or a block is selected. */
function announcement(view: EditorView, before: EditorState): string | undefined {
  const { state } = view;
  // Where nothing can be edited there is nothing to offer.
  if (!view.editable) return undefined;
  const place = placeOf(state);
  if (place) return placeOf(before) ? undefined : m.editor_place_opened();
  const { selection } = state;
  if (!(selection instanceof NodeSelection)) return undefined;
  if (before.selection.eq(selection) && !placeOf(before)) return undefined;
  if (isDiagramNode(selection.node))
    return m.editor_diagram_selected({ kind: diagramKind(selection.node.textContent) });
  if (selection.node.type.name === 'blockMath')
    return m.editor_math_selected({ latex: String(selection.node.attrs.latex) });
  return undefined;
}

/** What the plugin does in the page: the focus of a place that opens, closing from outside, the live region. */
function placeView(view: EditorView) {
  const page = view.dom.ownerDocument;
  const live = page.createElement('div');
  live.setAttribute('class', 'doc-announce');
  // Not a `status`: the editor's own line of status is that (BodyEditor.svelte).
  live.setAttribute('aria-live', 'polite');
  view.dom.after(live);
  const outside = (target: EventTarget | null): boolean => {
    const place = placeOf(view.state);
    if (!place || !(target instanceof Node)) return false;
    return !holderOf(view, place)?.contains(target);
  };
  // A press, not a touch: a finger that only scrolls the page leaves the place open.
  const onClick = (event: Event): void => {
    if (outside(event.target) && !view.composing) closePlace(view, false);
  };
  const onFocusIn = (event: Event): void => {
    // The editor itself is "elsewhere" only for a formula, whose field has the focus; for a
    // diagram a press in the editor moves the selection, which closes.
    const place = placeOf(view.state);
    const away = event.target !== view.dom || (place !== null && place.kind !== 'diagram');
    if (outside(event.target) && away && !view.composing) closePlace(view, false);
  };
  page.addEventListener('click', onClick, true);
  page.addEventListener('focusin', onFocusIn);
  return {
    update: (_: EditorView, before: EditorState) => {
      const place = placeOf(view.state);
      if (place && !view.editable) {
        // The editor became read-only with a place open. Not from inside this update.
        queueMicrotask(() => {
          if (!view.isDestroyed) dropPlace(view);
        });
        return;
      }
      const said = announcement(view, before);
      if (said !== undefined) live.textContent = said;
      if (!place || placeOf(before)) return;
      // Just opened: the field of a formula takes the focus (a diagram's source is the editor's).
      if (place.panel) place.panel.focus();
      else if (place.kind === 'formula') {
        const holder = holderOf(view, place);
        if (holder instanceof HTMLElement) holder.querySelector('textarea')?.focus();
      }
    },
    destroy: () => {
      page.removeEventListener('click', onClick, true);
      page.removeEventListener('focusin', onFocusIn);
      live.remove();
    },
  };
}

export const EditingPlace = Extension.create({
  name: 'editingPlace',
  // Before the keys of the blocks themselves: Enter on a selected block, Escape in a source.
  priority: 1000,

  addProseMirrorPlugins() {
    return [
      new Plugin<Place | null>({
        key: placeKey,
        state: {
          init: () => null,
          apply: (tr, value, _old, state) => nextPlace(value, tr, state),
        },
        appendTransaction: keepOutOfClosed,
        props: {
          handleDOMEvents: { keydown: placeKeys },
          handleKeyDown,
          // Text typed while a closed diagram or block formula is selected does not take its place: passing
          // over one with the arrows is not one key away from losing it.
          handleTextInput: (view) => {
            const { selection } = view.state;
            return (
              selection instanceof NodeSelection &&
              (isDiagramNode(selection.node) || selection.node.type.name === 'blockMath')
            );
          },
          decorations: (state) => {
            const place = placeOf(state);
            if (!place) return null;
            if (place.panel) {
              return DecorationSet.create(state.doc, [
                Decoration.widget(place.pos + 1, place.panel.dom, {
                  side: -1,
                  key: 'new-formula',
                  ignoreSelection: true,
                  stopEvent: () => true,
                }),
              ]);
            }
            const node = state.doc.nodeAt(place.pos);
            if (!node) return null;
            return DecorationSet.create(state.doc, [
              Decoration.node(place.pos, place.pos + node.nodeSize, {}, { place }),
            ]);
          },
        },
        view: placeView,
      }),
    ];
  },
});

/** The open place a node view's decorations carry, if they carry one. */
export function placeIn(decorations: readonly Decoration[]): Place | undefined {
  for (const decoration of decorations) {
    const spec = decoration.spec as { place?: Place };
    if (spec.place) return spec.place;
  }
  return undefined;
}
