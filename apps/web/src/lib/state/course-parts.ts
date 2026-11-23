/** Making a course in the workspace shown, in the sidebar at once. */
import type { Container, ContainerPatch } from '../logic/wire.ts';

import type { CourseDraft } from './course-types.ts';
import type { Workspace } from './workspace.svelte.ts';

export async function createCourse(
  workspace: Workspace,
  draft: CourseDraft,
): Promise<Container | undefined> {
  const courses = workspace.backend.course;
  const shown = workspace.info;
  if (!courses || !shown) return undefined;
  try {
    const made = await courses.createCourse(shown.id, draft);
    workspace.receiveContainer(made);
    return made;
  } catch (error) {
    workspace.report('save', error);
    return undefined;
  }
}

/** Creates a course from a template and applies optional title/visibility overrides. */
export async function createCourseFromTemplate(
  workspace: Workspace,
  templateId: string,
  draft: CourseDraft,
): Promise<Container | undefined> {
  const made = await workspace.duplicateContainer(templateId);
  if (!made) return undefined;
  const patch: ContainerPatch = {};
  if (draft.title && draft.title !== made.title) patch.title = draft.title;
  if (draft.type !== made.type) patch.type = draft.type;
  if (patch.title === undefined && patch.type === undefined) return made;
  try {
    const updated = await workspace.backend.patchContainer(made.id, patch);
    workspace.receiveContainer(updated);
    return updated;
  } catch (error) {
    workspace.report('save', error);
    return made;
  }
}
