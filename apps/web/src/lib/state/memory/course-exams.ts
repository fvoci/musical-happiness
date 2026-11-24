/**
 * In-memory implementation of course exams and strictly personal exam scores
 * matching `crates/model/src/course_exam.rs`, `crates/store/src/course_exams.rs`,
 * and `crates/server/src/app/course_events.rs`.
 *
 * Invariants:
 * - Exams sorted by `examDate ASC NULLS LAST, createAt ASC, id ASC`.
 * - Personal exam scores are strictly isolated per caller, excluded for unjoined
 *   readers and erased/removed users, and emit zero WebSocket broadcasts.
 */
import { parseCalendarDate } from '../../logic/dates.ts';
import {
  compareCourseExams,
  COURSE_EXAM_DESCRIPTION_MAX_CHARS,
  COURSE_EXAM_MAX_SCORE_LIMIT,
  COURSE_EXAM_TITLE_MAX_CHARS,
  EXAM_SCORE_NOTE_MAX_CHARS,
  type CourseExam,
  type CourseExamPatch,
  type CourseExamsApi,
  type MyExamScore,
  type MyExamScoreUpdate,
  type NewCourseExam,
} from '../course-meeting-types.ts';

import { memberRole } from './container-access.ts';
import {
  activeMemberIds,
  checkMultiline,
  checkTitle,
  isEnrolledActive,
  mayManageCourseEvents,
} from './course-meetings.ts';
import { fail } from './refuse.ts';
import { copy } from './shapes.ts';
import type { WikiHost } from './wikis.ts';
import type { StoredCourseExam, World } from './world.ts';

function checkExamDate(raw: string | null | undefined): string | null {
  if (raw === undefined || raw === null) return null;
  const parsed = parseCalendarDate(raw);
  if (!parsed) {
    fail(400, 'must be a valid calendar date (YYYY-MM-DD)', {
      field: 'examDate',
      reason: 'invalid_format',
    });
  }
  return parsed;
}

function checkMaxScore(value: number): number {
  if (!Number.isFinite(value) || value <= 0 || value > COURSE_EXAM_MAX_SCORE_LIMIT) {
    fail(400, `maxScore must be greater than 0 and at most ${COURSE_EXAM_MAX_SCORE_LIMIT}`, {
      field: 'maxScore',
      reason: 'invalid',
    });
  }
  return value;
}

function buildExamView(
  world: World,
  callerId: string,
  courseId: string,
  exam: StoredCourseExam,
): CourseExam {
  const active = activeMemberIds(world, courseId);
  const enrolled = active.has(callerId);
  const myRaw = enrolled ? world.examScores.get(callerId)?.get(exam.id) : undefined;
  let sum = 0;
  let count = 0;
  for (const memberId of active) {
    const score = world.examScores.get(memberId)?.get(exam.id);
    if (score !== undefined) {
      sum += score.score;
      count += 1;
    }
  }
  return {
    ...copy(exam),
    myScore: myRaw ? copy(myRaw) : null,
    averageScore: count > 0 ? sum / count : null,
    scoreCount: count,
  };
}

function findExam(world: World, courseId: string, examId: string): StoredCourseExam {
  const list = world.courseExams.get(courseId) ?? [];
  const found = list.find((item) => item.id === examId);
  if (!found) fail(404, 'no such exam');
  return found;
}

export function memoryListCourseExams(
  world: World,
  userId: string,
  courseId: string,
): CourseExam[] {
  memberRole(world, userId, courseId, 'course');
  return [...(world.courseExams.get(courseId) ?? [])]
    .map((exam) => buildExamView(world, userId, courseId, exam))
    .sort(compareCourseExams);
}

export function memoryCreateCourseExam(
  world: World,
  userId: string,
  courseId: string,
  draft: NewCourseExam,
): CourseExam {
  mayManageCourseEvents(world, userId, courseId);
  const title = checkTitle(draft.title, COURSE_EXAM_TITLE_MAX_CHARS);
  const examDate = checkExamDate(draft.examDate);
  const maxScore = checkMaxScore(draft.maxScore);
  const description = checkMultiline(
    draft.description ?? '',
    'description',
    COURSE_EXAM_DESCRIPTION_MAX_CHARS,
  );
  const now = world.now();
  const exam: StoredCourseExam = {
    id: world.newId(),
    containerId: courseId,
    title,
    examDate,
    maxScore,
    description,
    createAt: now,
    updateAt: now,
  };
  const existing = world.courseExams.get(courseId) ?? [];
  existing.push(exam);
  world.courseExams.set(courseId, existing);
  return buildExamView(world, userId, courseId, exam);
}

export function memoryPatchCourseExam(
  world: World,
  userId: string,
  courseId: string,
  examId: string,
  patch: CourseExamPatch,
): CourseExam {
  mayManageCourseEvents(world, userId, courseId);
  const exam = findExam(world, courseId, examId);
  // Validate all fields before mutating so a rejected PATCH leaves stored state untouched.
  const nextTitle =
    patch.title === undefined ? undefined : checkTitle(patch.title, COURSE_EXAM_TITLE_MAX_CHARS);
  const hasExamDate = 'examDate' in patch && patch.examDate !== undefined;
  const nextExamDate = hasExamDate ? checkExamDate(patch.examDate) : undefined;
  const nextMaxScore = patch.maxScore === undefined ? undefined : checkMaxScore(patch.maxScore);
  const nextDescription =
    patch.description === undefined
      ? undefined
      : checkMultiline(patch.description, 'description', COURSE_EXAM_DESCRIPTION_MAX_CHARS);

  if (nextTitle !== undefined) exam.title = nextTitle;
  if (hasExamDate) exam.examDate = nextExamDate ?? null;
  if (nextMaxScore !== undefined) exam.maxScore = nextMaxScore;
  if (nextDescription !== undefined) exam.description = nextDescription;
  exam.updateAt = world.now();
  return buildExamView(world, userId, courseId, exam);
}

export function memoryDeleteCourseExam(
  world: World,
  userId: string,
  courseId: string,
  examId: string,
): void {
  mayManageCourseEvents(world, userId, courseId);
  const list = world.courseExams.get(courseId) ?? [];
  const index = list.findIndex((item) => item.id === examId);
  if (index === -1) fail(404, 'no such exam');
  list.splice(index, 1);
  for (const scores of world.examScores.values()) scores.delete(examId);
}

export function memoryPutMyExamScore(
  world: World,
  userId: string,
  courseId: string,
  examId: string,
  update: MyExamScoreUpdate,
): MyExamScore {
  memberRole(world, userId, courseId, 'course');
  if (!isEnrolledActive(world, userId, courseId)) {
    fail(403, 'join the course before recording your exam score');
  }
  const exam = findExam(world, courseId, examId);
  if (!Number.isFinite(update.score) || update.score < 0) {
    fail(400, 'score must be a finite number greater than or equal to 0', {
      field: 'score',
      reason: 'invalid',
    });
  }
  if (update.score > exam.maxScore) {
    fail(400, `score must be at most ${exam.maxScore}`, { field: 'score', reason: 'invalid' });
  }
  const note = checkMultiline(update.note ?? '', 'note', EXAM_SCORE_NOTE_MAX_CHARS);
  let byExam = world.examScores.get(userId);
  if (!byExam) {
    byExam = new Map();
    world.examScores.set(userId, byExam);
  }
  const saved: MyExamScore = { score: update.score, note, updateAt: world.now() };
  byExam.set(examId, saved);
  return copy(saved);
}

export function memoryDeleteMyExamScore(
  world: World,
  userId: string,
  courseId: string,
  examId: string,
): void {
  memberRole(world, userId, courseId, 'course');
  if (!isEnrolledActive(world, userId, courseId)) {
    fail(403, 'join the course before deleting your exam score');
  }
  findExam(world, courseId, examId);
  world.examScores.get(userId)?.delete(examId);
}

/** Deletes all personal exam scores for `userId` in `courseId` (used on member removal). */
export function purgeMemberExamScores(world: World, courseId: string, userId: string): void {
  const byExam = world.examScores.get(userId);
  if (!byExam) return;
  for (const exam of world.courseExams.get(courseId) ?? []) byExam.delete(exam.id);
}

export function memoryCourseExams(host: WikiHost, enter: () => Promise<void>): CourseExamsApi {
  return {
    async list(courseId) {
      await enter();
      return memoryListCourseExams(host.world, host.userId, courseId);
    },
    async create(courseId, draft) {
      await enter();
      return memoryCreateCourseExam(host.world, host.userId, courseId, draft);
    },
    async patch(courseId, examId, patch) {
      await enter();
      return memoryPatchCourseExam(host.world, host.userId, courseId, examId, patch);
    },
    async delete(courseId, examId) {
      await enter();
      memoryDeleteCourseExam(host.world, host.userId, courseId, examId);
    },
    async putMyScore(courseId, examId, update) {
      await enter();
      return memoryPutMyExamScore(host.world, host.userId, courseId, examId, update);
    },
    async deleteMyScore(courseId, examId) {
      await enter();
      memoryDeleteMyExamScore(host.world, host.userId, courseId, examId);
    },
  };
}
