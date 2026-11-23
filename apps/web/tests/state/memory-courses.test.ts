/**
 * The courses of the backend in memory keep the server's rules
 * (`crates/store/src/units.rs`): a course is a container of kind `course` that
 * board/card operations and duplication refuse, its nodes are a 2-level tree
 * (`chapter` at top only, `unit` at top or under a chapter), chapters have no
 * body, counts are capped separately (200 chapters / 2000 units), deleting a
 * chapter takes its units along, restoring an orphaned unit moves it to top,
 * and open courses are readable without joining.
 */
import { describe, expect, it } from 'vitest';

import { pageTree } from '$lib/logic/page-tree.ts';
import { BackendError, type CourseUnit } from '$lib/state/index.ts';

import { setup, text } from './support.ts';

const rejection = async (promise: Promise<unknown>): Promise<BackendError> => {
  try {
    await promise;
  } catch (error) {
    if (error instanceof BackendError) return error;
    throw error;
  }
  throw new Error('the call did not reject');
};

async function aCourse(made = setup(), type: 'O' | 'P' = 'P') {
  const workspaceId = made.world.workspace.id;
  const course = await made.backend.course.createCourse(workspaceId, {
    title: 'Onboarding',
    type,
  });
  return { ...made, workspaceId, course };
}

/** The titles of a course's tree, as nested lists. */
async function shape(made: Awaited<ReturnType<typeof aCourse>>): Promise<unknown[]> {
  const units = await made.backend.course.units(made.course.id);
  const walk = (nodes: ReturnType<typeof pageTree<CourseUnit>>): unknown[] =>
    nodes.map((node) =>
      node.children.length > 0 ? [node.page.title, walk(node.children)] : node.page.title,
    );
  return walk(pageTree(units));
}

describe('a course in memory', () => {
  it('is filed in the maker’s default category with admin role, outside boards and templates', async () => {
    const made = await aCourse();
    expect(made.course.kind).toBe('course');
    expect(made.course.cardProperties).toEqual([]);
    const categories = await made.backend.categories(made.workspaceId);
    const system = categories.find((category) => category.type === 'system');
    expect(system?.containerIds).toContain(made.course.id);
    const [member] = await made.backend.members(made.course.id);
    expect(member?.schemeAdmin).toBe(true);
    expect((await made.backend.course.courses(made.workspaceId)).map((c) => c.id)).toEqual([
      made.course.id,
    ]);
    expect((await made.backend.boards(made.workspaceId)).some((b) => b.id === made.course.id)).toBe(
      false,
    );
    expect(
      (await made.backend.templates(made.workspaceId)).some((b) => b.id === made.course.id),
    ).toBe(false);
    // Courses cannot be duplicated or turned into templates.
    expect(
      (await rejection(made.backend.duplicateContainer(made.course.id, false, made.workspaceId)))
        .status,
    ).toBe(404);
    expect(
      (await rejection(made.backend.duplicateContainer(made.course.id, true, made.workspaceId)))
        .status,
    ).toBe(404);
    // Board/card endpoints do not reach a course.
    expect((await rejection(made.backend.cards(made.course.id))).status).toBe(404);
  });

  it('enforces depth-2, chapter-only parenting, top-only chapters, and bodyless chapters', async () => {
    const made = await aCourse();
    const courses = made.backend.course;
    const ch1 = await courses.createUnit(made.course.id, { kind: 'chapter', title: 'Chapter 1' });
    const u1 = await courses.createUnit(made.course.id, {
      kind: 'unit',
      title: 'Unit 1',
      parentId: ch1.id,
    });
    const u2 = await courses.createUnit(made.course.id, { kind: 'unit', title: 'Unit 2' });
    expect(await shape(made)).toEqual([['Chapter 1', ['Unit 1']], 'Unit 2']);

    // Chapter cannot be created under a chapter or unit.
    const nestedChapter = await rejection(
      courses.createUnit(made.course.id, { kind: 'chapter', title: 'Sub', parentId: ch1.id }),
    );
    expect([nestedChapter.status, nestedChapter.refusalAt('/parentId') !== undefined]).toEqual([
      400,
      true,
    ]);

    // Unit cannot be created under another unit.
    const nestedUnit = await rejection(
      courses.createUnit(made.course.id, { kind: 'unit', title: 'Sub', parentId: u1.id }),
    );
    expect([nestedUnit.status, nestedUnit.refusalAt('/parentId') !== undefined]).toEqual([
      400,
      true,
    ]);

    // Moving a chapter under anything is refused.
    const ch2 = await courses.createUnit(made.course.id, { kind: 'chapter', title: 'Chapter 2' });
    expect(
      (await rejection(courses.moveUnit(made.course.id, ch2.id, { parentId: ch1.id }))).status,
    ).toBe(400);

    // Moving a unit under another unit is refused; under a chapter succeeds.
    expect(
      (await rejection(courses.moveUnit(made.course.id, u2.id, { parentId: u1.id }))).status,
    ).toBe(400);
    await courses.moveUnit(made.course.id, u2.id, { parentId: ch2.id });
    expect(await shape(made)).toEqual([
      ['Chapter 1', ['Unit 1']],
      ['Chapter 2', ['Unit 2']],
    ]);

    // Chapters have no body: unit() returns body null and patchUnit(body) rejects.
    const chDetail = await courses.unit(made.course.id, ch1.id);
    expect(chDetail.body).toBeNull();
    expect(
      (
        await rejection(
          courses.patchUnit(made.course.id, ch1.id, {
            body: text('nope'),
            readAt: chDetail.updateAt,
          }),
        )
      ).status,
    ).toBe(400);

    // Units do have bodies and enforce readAt conflict checks.
    const detail = await courses.unit(made.course.id, u1.id);
    expect(detail.body).toBeNull();
    const updated = await courses.patchUnit(made.course.id, u1.id, {
      body: text('Hello learners'),
      readAt: detail.updateAt,
    });
    expect(updated.body).toEqual(text('Hello learners'));
    expect(
      (
        await rejection(
          courses.patchUnit(made.course.id, u1.id, {
            body: text('stale'),
            readAt: detail.updateAt,
          }),
        )
      ).status,
    ).toBe(409);
  });

  it('caps chapters and units separately as the server does', async () => {
    const made = await aCourse();
    const courses = made.backend.course;
    courses.limits = { depth: 2, chapters: 1, units: 2 };
    const ch = await courses.createUnit(made.course.id, { kind: 'chapter', title: 'Only chapter' });
    const tooManyChapters = await rejection(
      courses.createUnit(made.course.id, { kind: 'chapter', title: 'Second' }),
    );
    expect([tooManyChapters.reason, tooManyChapters.limit]).toEqual(['too_many_chapters', 1]);
    await courses.createUnit(made.course.id, { kind: 'unit', title: 'U1', parentId: ch.id });
    await courses.createUnit(made.course.id, { kind: 'unit', title: 'U2' });
    const tooManyUnits = await rejection(
      courses.createUnit(made.course.id, { kind: 'unit', title: 'U3' }),
    );
    expect([tooManyUnits.reason, tooManyUnits.limit]).toEqual(['too_many_units', 2]);
  });

  it('deletes a chapter with its units, lists them in trash, and moves orphaned restored units to top', async () => {
    const made = await aCourse();
    const courses = made.backend.course;
    const ch = await courses.createUnit(made.course.id, { kind: 'chapter', title: 'Basics' });
    const u1 = await courses.createUnit(made.course.id, {
      kind: 'unit',
      title: 'Welcome',
      parentId: ch.id,
    });
    await courses.createUnit(made.course.id, {
      kind: 'unit',
      title: 'Included',
      parentId: ch.id,
    });
    await courses.createUnit(made.course.id, { kind: 'unit', title: 'Standalone' });

    // Delete u1 first, then delete its chapter (taking Included along).
    await courses.deleteUnit(made.course.id, u1.id);
    await courses.deleteUnit(made.course.id, ch.id);
    expect(await shape(made)).toEqual(['Standalone']);

    const trash = await courses.deletedUnits(made.course.id);
    expect(trash.map((entry) => [entry.title, entry.below])).toEqual([
      ['Basics', 1],
      ['Welcome', 0],
    ]);

    // Restoring u1 while its parent chapter is still deleted places it at top.
    const restoredUnit = await courses.undeleteUnit(made.course.id, u1.id);
    expect(restoredUnit.parentId).toBe('');
    expect(await shape(made)).toEqual(['Standalone', 'Welcome']);

    // Restoring the chapter brings it and its child back.
    await courses.undeleteUnit(made.course.id, ch.id);
    expect(await shape(made)).toEqual([['Basics', ['Included']], 'Standalone', 'Welcome']);
    expect(await courses.deletedUnits(made.course.id)).toEqual([]);
  });

  it('allows workspace members to read an open course without joining and exports/imports archives', async () => {
    const made = await aCourse(setup(), 'O');
    const ch = await made.backend.course.createUnit(made.course.id, {
      kind: 'chapter',
      title: 'Intro',
    });
    await made.backend.course.createUnit(made.course.id, {
      kind: 'unit',
      title: 'Start',
      parentId: ch.id,
    });

    // Another workspace member can read units of an open course without membership.
    expect((await made.other.course.units(made.course.id)).map((u) => u.title)).toEqual([
      'Intro',
      'Start',
    ]);
    // But cannot write until joining.
    expect(
      (await rejection(made.other.course.createUnit(made.course.id, { title: 'Blocked' }))).status,
    ).toBe(403);

    // Archive export & import roundtrip preserves course kind and 2-level tree.
    const saved = await made.backend.exportContainer(made.course.id);
    const file = new File([saved.bytes], saved.name);
    await made.backend.importArchive(made.workspaceId, file);
    const all = await made.backend.course.courses(made.workspaceId);
    const imported = all.find((c) => c.id !== made.course.id);
    expect(imported).toMatchObject({ kind: 'course', title: 'Onboarding' });
    expect(await shape({ ...made, course: imported ?? made.course })).toEqual([
      ['Intro', ['Start']],
    ]);
  });
});
