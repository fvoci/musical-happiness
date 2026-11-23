/**
 * The courses of a workspace and their chapters and units, part of the data
 * boundary (`backend.ts`, `Backend.course`).
 *
 * A course is a container of the kind `course` (`Container.kind`): it is read,
 * changed, deleted, brought back, joined and filed in the sidebar as a board
 * or wiki is, by the container operations. What only a course has is here: its
 * outline of chapters (title-only top-level groupings) and units (each with a
 * body edited together in its room via `Backend.openDoc`).
 */
import type { DocNode } from '../editor/schema.ts';
import type { Container, ContainerType, MemberRole } from '../logic/wire.ts';

import type { DeletedItem, Paging } from './trash-reads.ts';

/** Whether an outline entry is a unit (with a body) or a chapter (a top-level heading). */
export type UnitKind = 'unit' | 'chapter';

/** A unit or chapter as the outline of its course lists it: without its body. */
export interface CourseUnit {
  id: string;
  courseId: string;
  kind: UnitKind;
  /** The chapter it is below; empty for a chapter or a top-level unit. */
  parentId: string;
  title: string;
  /**
   * Its place among siblings of the same parent: compared by code unit, then
   * by `id` (`logic/page-tree.ts`). The backend makes it; screens only sort by it.
   */
  order: string;
  createAt: number;
  updateAt: number;
}

/** A unit or chapter read on its own: with its body (`null` for a chapter) and authors. */
export interface CourseUnitDetail extends CourseUnit {
  body: DocNode | null;
  createdBy: string;
  modifiedBy: string;
}

/** What a course is made from. Whoever makes it is its admin. */
export interface CourseDraft {
  title: string;
  description?: string;
  icon?: string;
  /** `O`: anyone in the workspace can read and join; `P`: invited only. */
  type: ContainerType;
  minimumRole?: Exclude<MemberRole, '' | 'admin'>;
}

/** A unit or chapter to make; the backend gives it its identifier, times and `order`. */
export interface UnitDraft {
  /** Whether to make a `unit` (default) or a `chapter`. */
  kind?: UnitKind;
  title?: string;
  /** The chapter it goes below; empty or left out for the top. Must be empty for a chapter. */
  parentId?: string;
  /** The sibling it goes right after: empty for the first place, left out for the last. */
  after?: string;
}

/** Where a unit or chapter moves within its course. Chapters stay at the top (`parentId` empty). */
export interface UnitMove {
  parentId?: string;
  after?: string;
}

/** What a unit or chapter is changed to; omitted fields stay. Chapters take no `body`. */
export interface UnitPatch {
  title?: string;
  body?: DocNode | null;
  /** The `updateAt` the unit was read at: 409 if the unit changed since. */
  readAt?: number;
}

export interface Courses {
  /** The courses of the workspace the reader can see (member or open). */
  courses(workspaceId: string): Promise<Container[]>;
  /** Makes a course, filed under the reader's default category. */
  createCourse(workspaceId: string, draft: CourseDraft): Promise<Container>;
  /** Every chapter and unit of the course, in no promised order. */
  units(courseId: string): Promise<CourseUnit[]>;
  unit(courseId: string, unitId: string): Promise<CourseUnitDetail>;
  /** For editors. Invalid `parentId`/`after` is 400; limits are `too_many_units` / `too_many_chapters`. */
  createUnit(courseId: string, draft: UnitDraft): Promise<CourseUnitDetail>;
  patchUnit(courseId: string, unitId: string, patch: UnitPatch): Promise<CourseUnitDetail>;
  /** Moves a unit, or a chapter with all units below it, to the course's trash. */
  deleteUnit(courseId: string, unitId: string): Promise<void>;
  /** Moves a unit or chapter within its course; moving a chapter keeps its units below it. */
  moveUnit(courseId: string, unitId: string, to: UnitMove): Promise<CourseUnitDetail>;
  /** The course's deleted chapters and units, newest first. */
  deletedUnits(courseId: string, paging?: Paging): Promise<DeletedItem[]>;
  /** Restores a unit (or a chapter with the units deleted with it) from the trash. */
  undeleteUnit(courseId: string, unitId: string): Promise<CourseUnitDetail>;
}

/** Where a backend keeps its courses. Public link readers do not have courses until t5. */
export interface CoursesOfBackend {
  readonly course?: Courses;
}
