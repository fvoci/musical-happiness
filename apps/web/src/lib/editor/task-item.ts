/**
 * A task of a body, whose box changes the task and nothing else.
 *
 * The library's task (`@tiptap/extension-list`) gives the editor the focus
 * when its box changes, and keeps a press of the mouse from the box itself
 * only, not from the label around it. On a touch screen the label is a
 * finger's square drawn over the box (editor.css), so a finger lands on the
 * label: the press focused the body there and put the selection in it, and
 * on a phone the focus of the body is its keyboard. Here a press anywhere on
 * the label leaves the focus and the selection where they were (none in the
 * body when it had none, the same caret when the reader was writing), and the
 * change is one step of the document, sent and undone as one. Space on a
 * focused box does the same and the box keeps the focus. A body that can
 * only be read keeps its tasks as they are, as before.
 *
 * The library's drawing of the task is kept (its box, its accessible name and
 * what an update redraws); only the press and the change are this file's.
 */
import { TaskItem } from '@tiptap/extension-list';

export const BodyTaskItem = TaskItem.extend({
  addNodeView() {
    const drawn = this.parent?.();
    if (!drawn) return null;
    const name = this.name;
    return (props) => {
      const view = drawn(props);
      const { editor, getPos } = props;
      const item = view.dom;
      const label = item.querySelector(':scope > label');
      const box = label?.querySelector('input[type="checkbox"]');
      if (!(label instanceof HTMLElement) || !(box instanceof HTMLInputElement)) return view;

      // Neither the browser (which would focus the body and put its selection
      // at the label) nor the editor (which leaves an event whose default is
      // prevented alone) takes a press on the label for one in the text.
      label.addEventListener('mousedown', (event) => {
        event.preventDefault();
      });
      // A press on the label passes to the box without the browser's focusing
      // it (WebKit does), which would take the focus from the text being written.
      label.addEventListener('click', (event) => {
        if (event.target === box) return;
        event.preventDefault();
        box.click();
      });

      // Taken on the way down, before the library's own listener on the box,
      // which would focus the editor: the event goes no further.
      item.addEventListener(
        'change',
        (event) => {
          if (event.target !== box) return;
          event.stopPropagation();
          const at = editor.isEditable ? getPos() : undefined;
          const task = at === undefined ? null : editor.state.doc.nodeAt(at);
          if (at === undefined || task?.type.name !== name) {
            box.checked = !box.checked;
            return;
          }
          editor.view.dispatch(
            editor.state.tr.setNodeMarkup(at, undefined, { ...task.attrs, checked: box.checked }),
          );
        },
        { capture: true },
      );
      return view;
    };
  },
});
