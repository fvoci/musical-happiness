/**
 * While editing, a code block whose language is that of diagrams shows the
 * drawn diagram under its source.
 *
 * The picture is no part of the document: it is a decoration behind the
 * block. It is drawn again a moment after the source last changed, and when
 * the page turns light or dark. The picture that is there stays until the
 * next one is ready, so that typing does not make it blink; a source that
 * cannot be drawn shows why in its place.
 */
import { Extension } from '@tiptap/core';
import type { Node as DocumentNode } from '@tiptap/pm/model';
import { Plugin, PluginKey } from '@tiptap/pm/state';
import { Decoration, DecorationSet } from '@tiptap/pm/view';

import { createDiagramFigure, DIAGRAM_REDRAW_DELAY, fillDiagram, onLookChange } from './embeds.ts';
import { DIAGRAM_LANGUAGE } from './schema.ts';

/** The diagrams of a document in order: the source, and the position right behind the block. */
function diagramsOf(doc: DocumentNode): { source: string; end: number }[] {
  const found: { source: string; end: number }[] = [];
  doc.descendants((node, position) => {
    if (node.type.name !== 'codeBlock') return true;
    if (node.attrs.language === DIAGRAM_LANGUAGE)
      found.push({ source: node.textContent, end: position + node.nodeSize });
    return false;
  });
  return found;
}

export const DiagramPreview = Extension.create({
  name: 'diagramPreview',

  addProseMirrorPlugins() {
    return [
      new Plugin({
        key: new PluginKey('diagramPreview'),
        props: {
          decorations: (state) =>
            DecorationSet.create(
              state.doc,
              diagramsOf(state.doc).map(({ end }, index) =>
                Decoration.widget(
                  end,
                  (view) => {
                    const figure = createDiagramFigure(view.dom.ownerDocument);
                    figure.dataset.diagram = String(index);
                    figure.contentEditable = 'false';
                    return figure;
                  },
                  // The same key for the same diagram, so that its figure stays in the page.
                  { key: `doc-diagram-${index}`, side: -1, ignoreSelection: true },
                ),
              ),
            ),
        },
        view: (view) => {
          /** What each figure shows, or is about to. */
          const shown = new WeakMap<HTMLElement, string>();
          const timers = new Map<HTMLElement, ReturnType<typeof setTimeout>>();
          const draw = (figure: HTMLElement, source: string, wait: number): void => {
            shown.set(figure, source);
            clearTimeout(timers.get(figure));
            timers.set(
              figure,
              setTimeout(() => {
                timers.delete(figure);
                void fillDiagram(figure, source, () => shown.get(figure) === source);
              }, wait),
            );
          };
          const figures = (): HTMLElement[] =>
            Array.from(view.dom.querySelectorAll<HTMLElement>('.doc-diagram[data-diagram]'));
          const sync = (): void => {
            const diagrams = diagramsOf(view.state.doc);
            figures().forEach((figure, index) => {
              const source = diagrams[index]?.source ?? '';
              const before = shown.get(figure);
              if (before !== source)
                draw(figure, source, before === undefined ? 0 : DIAGRAM_REDRAW_DELAY);
            });
          };
          sync();
          const stopWatching = onLookChange(view.dom.ownerDocument, () => {
            for (const figure of figures()) draw(figure, shown.get(figure) ?? '', 0);
          });
          return {
            update: sync,
            destroy: () => {
              stopWatching();
              for (const timer of timers.values()) clearTimeout(timer);
              timers.clear();
            },
          };
        },
      }),
    ];
  },
});
