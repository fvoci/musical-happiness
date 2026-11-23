/**
 * The course on the page, for parts of the shell that are not its own (the
 * sidebar asks whether the reader may delete the open course), as `shownWiki`
 * is for a wiki.
 */
import type { CourseStore } from './course.svelte.ts';
import type { Workspace } from './workspace.svelte.ts';

export class ShownCourse {
  current = $state.raw<CourseStore | undefined>(undefined);
}

const shown = new WeakMap<Workspace, ShownCourse>();

export function shownCourse(workspace: Workspace): ShownCourse {
  let holder = shown.get(workspace);
  if (!holder) {
    holder = new ShownCourse();
    shown.set(workspace, holder);
  }
  return holder;
}
