/**
 * The store of an open course (`state/course.svelte.ts`) over the backend in
 * memory: loads chapters and units as a 2-level tree, joins open courses,
 * follows live updates from others, undoes creation/rename/move/delete, and
 * rejects non-course container IDs.
 */
import { describe, expect, it } from 'vitest';

import { pageRows } from '$lib/logic/page-tree.ts';
import { CourseStore, Workspace, type CourseUnit } from '$lib/state/index.ts';

import { settle, setup } from './support.ts';

async function openCourse(type: 'O' | 'P' = 'P') {
  const made = setup();
  await made.workspace.load();
  const workspaceId = made.world.workspace.id;
  const course = await made.backend.course.createCourse(workspaceId, {
    title: 'Onboarding',
    type,
  });
  const store = new CourseStore(made.workspace, course.id);
  await store.load();
  return { ...made, workspaceId, courseId: course.id, store };
}

const titles = (store: CourseStore): string[] =>
  pageRows<CourseUnit>(store.tree, () => true).map(
    (row) =>
      `${'  '.repeat(row.depth)}${row.page.kind === 'chapter' ? '[C] ' : ''}${row.page.title}`,
  );

describe('an open course', () => {
  it('reads the course, its chapters and units as a tree, and the reader’s role', async () => {
    const made = setup();
    await made.workspace.load();
    const course = await made.backend.course.createCourse(made.world.workspace.id, {
      title: 'Onboarding',
      type: 'P',
    });
    const ch = await made.backend.course.createUnit(course.id, {
      kind: 'chapter',
      title: 'Chapter 1',
    });
    await made.backend.course.createUnit(course.id, {
      kind: 'unit',
      title: 'Unit 1',
      parentId: ch.id,
    });
    const store = new CourseStore(made.workspace, course.id);
    await store.load();
    expect(store.status).toBe('ready');
    expect(store.course?.title).toBe('Onboarding');
    expect(titles(store)).toEqual(['[C] Chapter 1', '  Unit 1']);
    expect([store.role, store.canEdit, store.canManage]).toEqual(['admin', true, true]);
    store.close();
  });

  it('reports missing when given a board ID instead of a course ID', async () => {
    const { workspace, backend, world } = setup();
    await workspace.load();
    const [board] = await backend.boards(world.workspace.id);
    const store = new CourseStore(workspace, board?.id ?? '');
    await store.load();
    expect(store.missing).toBe(true);
    expect(store.status).toBe('failed');
    store.close();
  });

  it('reads an open course unjoined and lets a workspace member join to edit', async () => {
    const made = await openCourse('O');
    const otherWorkspace = new Workspace({ backend: made.other, locale: () => 'en' });
    await otherWorkspace.load();
    const store = new CourseStore(otherWorkspace, made.courseId);
    await store.load();
    expect([store.joinable, store.missing, store.status, store.canEdit]).toEqual([
      true,
      false,
      'ready',
      false,
    ]);
    expect(await store.join()).toBe(true);
    expect([store.joinable, store.status, store.role, store.canEdit]).toEqual([
      false,
      'ready',
      'editor',
      true,
    ]);
    store.close();
    made.store.close();
  });

  it('creates, moves, deletes, and undoes chapters and units', async () => {
    const made = await openCourse();
    const ch = await made.store.createUnit({ kind: 'chapter', title: 'Part 1' });
    const u1 = await made.store.createUnit({ kind: 'unit', title: 'Lesson 1', parentId: ch?.id });
    const u2 = await made.store.createUnit({ kind: 'unit', title: 'Lesson 2' });
    if (!ch || !u1 || !u2) throw new Error('expected units');
    expect(titles(made.store)).toEqual(['[C] Part 1', '  Lesson 1', 'Lesson 2']);

    await made.store.moveUnit(u2.id, { parentId: ch.id });
    expect(titles(made.store)).toEqual(['[C] Part 1', '  Lesson 1', '  Lesson 2']);

    await made.workspace.undoLast();
    expect(titles(made.store)).toEqual(['[C] Part 1', '  Lesson 1', 'Lesson 2']);

    const shown = made.store.showUnit(u1.id);
    await settle();
    expect(shown?.status).toBe('ready');

    await made.store.deleteUnit(ch.id);
    expect(titles(made.store)).toEqual(['Lesson 2']);
    expect(shown?.removed).toEqual({ by: 'reader', title: 'Lesson 1' });

    await made.workspace.undoLast();
    expect(titles(made.store)).toEqual(['[C] Part 1', '  Lesson 1', 'Lesson 2']);
    made.store.close();
  });

  it('follows live changes made by another editor', async () => {
    const made = await openCourse();
    await made.backend.addMember(made.courseId, made.other.userId, 'editor');
    const ch = await made.other.course.createUnit(made.courseId, {
      kind: 'chapter',
      title: 'Their Chapter',
    });
    const u = await made.other.course.createUnit(made.courseId, {
      kind: 'unit',
      title: 'Their Unit',
      parentId: ch.id,
    });
    await settle();
    expect(titles(made.store)).toEqual(['[C] Their Chapter', '  Their Unit']);

    const shown = made.store.showUnit(u.id);
    await settle();
    await made.other.course.deleteUnit(made.courseId, ch.id);
    await settle();
    expect(titles(made.store)).toEqual([]);
    expect(shown?.missing).toBe(true);
    expect(shown?.removed).toEqual({ by: 'others', title: 'Their Unit' });
    made.store.close();
  });
});
