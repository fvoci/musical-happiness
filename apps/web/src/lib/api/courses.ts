/**
 * The courses and their chapters and units on the server (`Backend.course`):
 * the operations under `/w/{workspaceId}/courses` (`getCourses`, `createCourse`,
 * `getUnitTree`, `createUnit`, `getUnit`, `patchUnit`, `deleteUnit`, `moveUnit`,
 * `getDeletedUnits`, `undeleteUnit`). What a course shares with a board or wiki
 * (reading, changing, deleting, restoring, members, joining, sidebar, export)
 * goes through the container operations.
 */
import type { Container } from '../logic/wire.ts';
import type {
  CourseDraft,
  Courses,
  CourseUnit,
  CourseUnitDetail,
  UnitDraft,
  UnitMove,
  UnitPatch,
} from '../state/course-types.ts';
import type { DeletedItem, Paging } from '../state/trash-reads.ts';

import { bodyOf, containerOf, wireBody, type Schemas } from './convert.ts';
import type { WikiRequest } from './wikis.ts';

/** A unit or chapter as the outline lists it; `courseId` is the course it was read from. */
export function unitOf(node: Schemas['UnitNode'], courseId: string): CourseUnit {
  return {
    id: node.id,
    courseId,
    kind: node.kind,
    parentId: node.parentId,
    title: node.title,
    order: node.order,
    createAt: node.createAt,
    updateAt: node.updateAt,
  };
}

export function unitDetailOf(unit: Schemas['Unit']): CourseUnitDetail {
  return {
    ...unitOf(unit, unit.courseId),
    body: bodyOf(unit.body),
    createdBy: unit.createdBy,
    modifiedBy: unit.modifiedBy,
  };
}

/**
 * A unit or chapter as a change message (`UPDATE_BLOCK`) brings it: a block of
 * type `unit` or `chapter` whose `order` is among its `fields`.
 */
export function unitOfBlock(block: Schemas['Block']): CourseUnit {
  const fields = block.fields as { order?: unknown };
  return unitOf(
    {
      id: block.id,
      kind: block.type === 'chapter' ? 'chapter' : 'unit',
      parentId: block.parentId,
      title: block.title,
      order: typeof fields.order === 'string' ? fields.order : '',
      createAt: block.createAt,
      updateAt: block.updateAt,
    },
    block.containerId,
  );
}

function deletedUnitOf(unit: Schemas['DeletedUnit']): DeletedItem {
  return {
    id: unit.id,
    title: unit.title,
    deletedAt: unit.deletedAt,
    deletedBy: unit.deletedBy,
    below: unit.below,
  };
}

function newCourseOf(draft: CourseDraft): Schemas['NewCourse'] {
  return {
    title: draft.title,
    type: draft.type,
    ...(draft.description ? { description: draft.description } : {}),
    ...(draft.icon ? { icon: draft.icon } : {}),
    ...(draft.minimumRole ? { minimumRole: draft.minimumRole } : {}),
  };
}

function unitPatchOf(patch: UnitPatch): Schemas['UnitPatch'] {
  const body = patch.body === undefined ? {} : { body: patch.body && wireBody(patch.body) };
  return {
    ...(patch.title === undefined ? {} : { title: patch.title }),
    ...body,
    ...(patch.readAt === undefined ? {} : { readAt: patch.readAt }),
  };
}

export class ServerCourses implements Courses {
  readonly #request: WikiRequest;
  readonly #learn: (courses: readonly Schemas['Container'][]) => void;

  constructor(request: WikiRequest, learn: (courses: readonly Schemas['Container'][]) => void) {
    this.#request = request;
    this.#learn = learn;
  }

  async courses(workspaceId: string): Promise<Container[]> {
    const courses = await this.#request<Schemas['Container'][]>('/courses', {}, {}, workspaceId);
    this.#learn(courses);
    return courses.map(containerOf);
  }

  async createCourse(workspaceId: string, draft: CourseDraft): Promise<Container> {
    const course = await this.#request<Schemas['Container']>(
      '/courses',
      {},
      { method: 'POST', json: newCourseOf(draft) },
      workspaceId,
    );
    this.#learn([course]);
    return containerOf(course);
  }

  async units(courseId: string): Promise<CourseUnit[]> {
    const nodes = await this.#request<Schemas['UnitNode'][]>('/courses/{courseId}/units', {
      courseId,
    });
    return nodes.map((node) => unitOf(node, courseId));
  }

  async unit(courseId: string, unitId: string): Promise<CourseUnitDetail> {
    return unitDetailOf(
      await this.#request<Schemas['Unit']>('/courses/{courseId}/units/{unitId}', {
        courseId,
        unitId,
      }),
    );
  }

  async createUnit(courseId: string, draft: UnitDraft): Promise<CourseUnitDetail> {
    const request: Schemas['NewUnit'] = {
      ...(draft.kind ? { kind: draft.kind } : {}),
      ...(draft.title ? { title: draft.title } : {}),
      ...(draft.parentId ? { parentId: draft.parentId } : {}),
      ...(draft.after === undefined ? {} : { after: draft.after }),
    };
    return unitDetailOf(
      await this.#request<Schemas['Unit']>(
        '/courses/{courseId}/units',
        { courseId },
        { method: 'POST', json: request },
      ),
    );
  }

  async patchUnit(courseId: string, unitId: string, patch: UnitPatch): Promise<CourseUnitDetail> {
    return unitDetailOf(
      await this.#request<Schemas['Unit']>(
        '/courses/{courseId}/units/{unitId}',
        { courseId, unitId },
        { method: 'PATCH', json: unitPatchOf(patch) },
      ),
    );
  }

  async deleteUnit(courseId: string, unitId: string): Promise<void> {
    await this.#request(
      '/courses/{courseId}/units/{unitId}',
      { courseId, unitId },
      { method: 'DELETE' },
    );
  }

  async moveUnit(courseId: string, unitId: string, to: UnitMove): Promise<CourseUnitDetail> {
    const request: Schemas['UnitMove'] = {
      ...(to.parentId ? { parentId: to.parentId } : {}),
      ...(to.after === undefined ? {} : { after: to.after }),
    };
    return unitDetailOf(
      await this.#request<Schemas['Unit']>(
        '/courses/{courseId}/units/{unitId}/move',
        { courseId, unitId },
        { method: 'POST', json: request },
      ),
    );
  }

  async deletedUnits(courseId: string, paging?: Paging): Promise<DeletedItem[]> {
    const units = await this.#request<Schemas['DeletedUnit'][]>(
      '/courses/{courseId}/trash/units',
      { courseId },
      { query: { ...paging } },
    );
    return units.map(deletedUnitOf);
  }

  async undeleteUnit(courseId: string, unitId: string): Promise<CourseUnitDetail> {
    return unitDetailOf(
      await this.#request<Schemas['Unit']>(
        '/courses/{courseId}/units/{unitId}/undelete',
        { courseId, unitId },
        { method: 'POST' },
      ),
    );
  }
}
