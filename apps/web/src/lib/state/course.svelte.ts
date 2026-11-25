/**
 * One open course: the course container itself, its members (what the reader
 * may do), and its outline of chapters and units (`tree`, `logic/page-tree.ts`).
 * The open unit is a store of its own (`unit.svelte.ts`), opened from here.
 *
 * Implements `TreeSource<CourseUnit>` so `PageTree`, `MovePageDialog`, and
 * `PageViewers` work directly with a `CourseStore`.
 */
import type { PagePlace } from '../logic/page-moves.ts';
import { pageTree } from '../logic/page-tree.ts';
import type { Container, ContainerMember, MemberRole } from '../logic/wire.ts';

import {
  isBackendError,
  type Change,
  type ContainerRole,
  type Courses,
  type CourseUnit,
  type CourseUnitDetail,
  type DeletedItem,
  type Paging,
  type UnitDraft,
} from './backend.ts';
import type {
  CourseLooks,
  CourseProgressSummary,
  ScheduledUnit,
  UnitProgress,
} from './course-types.ts';
export type { CourseLooks };
import { containerOfWorkspace, roleOf } from './board-parts.ts';
import { ContainerPresence } from './board-presence.svelte.ts';
import { isCommentBlock } from './changes.ts';
import { CourseMeetingsState } from './course-meetings.svelte.ts';
import { CourseProgress } from './course-progress.svelte.ts';
import { CourseScheduleState } from './course-schedule.svelte.ts';
import { CourseWrites } from './course-writes.ts';
import { MemberWrites } from './member-writes.ts';
import { orderMembers } from './order.ts';
import type { TreeSource } from './tree-source.ts';
import { UnitStore } from './unit.svelte.ts';
import { memberId } from './user-names.ts';
import type { LoadStatus, Workspace } from './workspace.svelte.ts';
import { reread, take } from './write.ts';

export class CourseStore implements TreeSource<CourseUnit> {
  readonly workspace: Workspace;
  readonly courseId: string;

  status = $state<LoadStatus>('idle');
  missing = $state(false);
  joining = $state(false);
  course = $state.raw<Container | undefined>(undefined);
  /** Every chapter and unit of the course, in no guaranteed order. */
  units = $state.raw<CourseUnit[]>([]);
  members = $state.raw<ContainerMember[]>([]);
  /** The unit shown, if one is. */
  openUnit = $state.raw<UnitStore | undefined>(undefined);

  /** The chapters and units as a tree, siblings ordered by `order` then `id`. */
  readonly tree = $derived(pageTree(this.units));
  readonly orderedMembers = $derived.by(() =>
    orderMembers(
      this.members,
      (userId) => this.workspace.userName(userId) ?? '',
      this.workspace.locale,
    ),
  );
  readonly role = $derived<MemberRole>(
    roleOf(this.members.find((member) => member.userId === this.workspace.me?.id)),
  );
  readonly canEdit = $derived(this.role === 'admin' || this.role === 'editor');
  readonly canManage = $derived(this.role === 'admin');
  readonly joinable = $derived(
    this.status === 'ready' && this.course?.visibility === 'open' && this.role === '',
  );
  readonly canTrackProgress = $derived(this.status === 'ready' && this.role !== '');

  #stopWatching: (() => void) | undefined;
  #closed = false;
  #readings = 0;

  readonly presence: ContainerPresence;
  readonly progress: CourseProgress;
  readonly schedule: CourseScheduleState;
  readonly events: CourseMeetingsState;
  readonly #writes: CourseWrites;
  readonly #people = new MemberWrites(this);

  constructor(workspace: Workspace, courseId: string) {
    this.workspace = workspace;
    this.courseId = courseId;
    const base = {
      courseId,
      workspace,
      courses: () => this.#courses,
      canTrack: () => this.canTrackProgress,
      isClosed: () => this.#closed,
    };
    this.presence = new ContainerPresence({ workspace, containerId: courseId });
    this.progress = new CourseProgress({
      ...base,
      units: () => this.units,
      onCompletionChanged: (unitId, completed) => {
        this.schedule.reconcileCompletion(unitId, completed);
      },
    });
    this.schedule = new CourseScheduleState({
      ...base,
      canEdit: () => this.canEdit,
      takeUnit: (unit) => {
        this.#receive(unit);
      },
      applyProgress: (row) => {
        this.progress.applyRow(row);
      },
      refreshMine: () => void this.progress.refreshMine(),
      syncUnitDates: (units) => {
        this.#syncUnitDates(units);
      },
    });
    this.events = new CourseMeetingsState({ ...base, canEdit: () => this.canEdit });
    this.#writes = new CourseWrites({
      courseId,
      workspace,
      courses: () => this.#courses,
      units: () => this.units,
      setUnits: (units) => {
        this.units = units;
      },
      openUnit: () => this.openUnit,
      takeUnit: (unit) => {
        this.#takeUnit(unit);
      },
      reread: () => void this.#refresh(),
    });
  }

  get #courses(): Courses {
    const courses = this.workspace.backend.course;
    if (!courses) throw new Error('this backend has no courses');
    return courses;
  }

  /** `TreeSource.pages` view of the course's chapters and units. */
  get pages(): readonly CourseUnit[] {
    return this.units;
  }

  /** `TreeSource.page` lookup by node id. */
  page(id: string | undefined): CourseUnit | undefined {
    return this.unit(id);
  }

  unit(unitId: string | undefined): CourseUnit | undefined {
    return this.units.find((unit) => unit.id === unitId);
  }

  async load(): Promise<void> {
    this.status = 'loading';
    this.missing = false;
    this.#readings += 1;
    const reading = this.#readings;
    const membersTold = this.#people.told;
    const progressMark = this.progress.writeVersion;
    const scheduleMark = this.schedule.writeVersion;
    try {
      this.#stopWatching?.();
      this.#stopWatching = this.workspace.backend.watchContainer(this.courseId, (change) => {
        this.#apply(change);
      });
      const backend = this.workspace.backend;
      const workspaceId = this.workspace.info?.id;
      const [course, units, members, progress, summary, schedule] = await Promise.all([
        containerOfWorkspace(backend, this.courseId, workspaceId, 'course'),
        this.#courses.units(this.courseId),
        backend.members(this.courseId),
        this.#courses.myProgress(this.courseId, workspaceId),
        this.#courses.progressSummary(this.courseId, workspaceId),
        this.#courses.schedule(this.courseId, workspaceId),
      ]);
      if (this.#closed || reading !== this.#readings) return;
      void this.workspace.learnUsers(members.map(memberId), { members: true });
      this.#takeCourse(course);
      this.units = units;
      this.#writes.remember(this.units);
      if (membersTold === this.#people.told) this.members = members;
      this.progress.take(progress, summary, progressMark);
      this.schedule.take(schedule, scheduleMark);
      this.status = 'ready';
      void this.events.refreshAll();
    } catch (error) {
      if (this.#closed || reading !== this.#readings) return;
      this.openUnit?.close();
      this.openUnit = undefined;
      if (isBackendError(error) && (error.status === 403 || error.status === 404)) {
        this.workspace.evictContainer(this.courseId);
        this.missing = true;
      } else this.workspace.report('load', error);
      this.status = 'failed';
    }
  }

  async join(): Promise<boolean> {
    if (this.joining) return false;
    this.joining = true;
    try {
      await this.workspace.backend.joinContainer(this.courseId);
    } catch (error) {
      if (isBackendError(error) && error.status === 404) {
        this.openUnit?.close();
        this.openUnit = undefined;
        this.workspace.evictContainer(this.courseId);
        [this.missing, this.status] = [true, 'failed'];
      } else this.workspace.report('save', error);
      return false;
    } finally {
      this.joining = false;
    }
    if (this.#closed) return false;
    await this.load();
    return this.status === 'ready';
  }

  #isClosed(): boolean {
    return this.#closed;
  }

  close(): void {
    this.#closed = true;
    this.#stopWatching?.();
    this.#stopWatching = undefined;
    this.openUnit?.close();
    this.openUnit = undefined;
  }

  /** Shows a unit: its store is made, and the one before closed. None for `undefined`. */
  showUnit(unitId: string | undefined): UnitStore | undefined {
    const open = this.openUnit;
    if (open && open.unitId === unitId && !open.missing) return open;
    this.openUnit?.close();
    this.openUnit = unitId === undefined ? undefined : new UnitStore(this, unitId);
    void this.openUnit?.load();
    return this.openUnit;
  }

  // ------------------------------------------------------------ changes

  #takeCourse(course: Container): void {
    if (!this.course || this.course.updatedAt <= course.updatedAt) this.course = course;
  }

  #apply(change: Change): void {
    switch (change.kind) {
      case 'container':
        if (change.container.id !== this.courseId) break;
        if (this.missing) void this.load();
        else this.#takeCourse(change.container);
        break;
      case 'containerDeleted':
        if (change.containerId !== this.courseId) break;
        if (this.course?.visibility === 'open' && !this.#closed) void this.load();
        else {
          this.workspace.evictContainer(this.courseId);
          this.openUnit?.close();
          this.openUnit = undefined;
          [this.missing, this.status] = [true, 'failed'];
        }
        break;
      case 'units':
        for (const unit of change.units) this.#receive(unit);
        void this.schedule.refresh();
        break;
      case 'blocks':
        for (const block of change.blocks) {
          if (isCommentBlock(block)) this.openUnit?.comments.receive(block);
        }
        break;
      case 'blocksDeleted':
        this.#writes.forget(change.blockIds, 'others');
        this.openUnit?.comments.forget(change.blockIds);
        void this.schedule.refresh();
        break;
      case 'members': {
        if (change.containerId !== this.courseId) break;
        const next = change.members;
        const removed = this.members.some((p) => !next.some((n) => memberId(n) === memberId(p)));
        this.#people.told += 1;
        this.members = next;
        const meId = this.workspace.me?.id;
        if (meId && !next.some((m) => memberId(m) === meId)) {
          this.progress.clearMine();
          this.schedule.clearMine();
          this.events.clearMine();
          if (this.course?.visibility !== 'open') this.workspace.evictContainer(this.courseId);
        } else if (removed) void this.events.refreshExams();
        void this.workspace.learnUsers(next.map(memberId), { members: true });
        break;
      }
      case 'resync':
        void this.#refresh();
        break;
      default:
        break;
    }
  }

  #receive(unit: CourseUnit): void {
    if (this.#writes.receive(unit)) this.#takeUnit(unit);
  }

  #takeUnit(unit: CourseUnit): void {
    if (unit.courseId !== this.courseId || !this.#writes.admits(unit)) return;
    const isNew = unit.kind === 'unit' && !this.units.some((existing) => existing.id === unit.id);
    this.units = take(this.units, unit);
    if (isNew && this.canTrackProgress) void this.progress.refreshMine();
    const shown = this.openUnit;
    if (shown?.unitId !== unit.id) return;
    if (shown.missing) void shown.load();
    else shown.unitChanged(unit);
  }

  async #refresh(): Promise<void> {
    if (this.status !== 'ready' || this.#closed) return;
    this.#readings += 1;
    const reading = this.#readings;
    const before = this.units.map((unit) => unit.id);
    const mark = this.#writes.forgettings;
    const membersTold = this.#people.told;
    const progressMark = this.progress.writeVersion;
    const scheduleMark = this.schedule.writeVersion;
    try {
      const workspaceId = this.workspace.info?.id;
      const [course, units, members, progress, summary, schedule] = await Promise.all([
        this.workspace.backend.container(this.courseId),
        this.#courses.units(this.courseId),
        this.workspace.backend.members(this.courseId),
        this.#courses.myProgress(this.courseId, workspaceId),
        this.#courses.progressSummary(this.courseId, workspaceId),
        this.#courses.schedule(this.courseId, workspaceId),
      ]);
      if (this.#isClosed() || reading !== this.#readings) return;
      this.#takeCourse(course);
      if (membersTold === this.#people.told) this.members = members;
      this.progress.take(progress, summary, progressMark);
      this.schedule.take(schedule, scheduleMark);
      void this.events.refreshAll();
      const shown = this.openUnit;
      const title = (shown && this.unit(shown.unitId)?.title) ?? '';
      const read = this.#writes.admitted(units, mark);
      this.units = reread(this.units, read, before, (unit) => this.#writes.receive(unit));
      this.#writes.remember(this.units);
      if (shown && !this.unit(shown.unitId)) {
        if (!this.#writes.isDeleting(shown.unitId)) shown.gone('others', title);
      } else if (shown) void shown.refresh();
    } catch (error) {
      if (this.#isClosed() || reading !== this.#readings) return;
      if (isBackendError(error) && error.status === 401) {
        this.workspace.report('load', error);
        return;
      }
      if (isBackendError(error) && (error.status === 403 || error.status === 404)) {
        this.workspace.evictContainer(this.courseId);
        void this.load();
      }
    }
  }

  // ------------------------------------------------------------- writes

  createUnit(draft: UnitDraft): Promise<CourseUnitDetail | undefined> {
    return this.#writes.createUnit(draft);
  }

  moving = $state(0);
  readonly settling = $derived.by(() => this.moving > 0 || this.workspace.undoBusy);

  async moveUnit(unitId: string, to: PagePlace): Promise<boolean> {
    this.moving += 1;
    try {
      return await this.#writes.moveUnit(unitId, to);
    } finally {
      this.moving -= 1;
    }
  }

  deleteUnit(unitId: string): Promise<boolean> {
    return this.#writes.deleteUnit(unitId);
  }

  async deletedUnits(paging?: Paging): Promise<DeletedItem[]> {
    await this.#writes.deletionsAnswered();
    return this.#courses.deletedUnits(this.courseId, paging);
  }

  restoreUnit(unitId: string): Promise<void> {
    return this.#writes.restoreUnit(unitId);
  }

  changeUnitTitle(unitId: string, title: string): Promise<void> {
    return this.#writes.changeUnitTitle(unitId, title);
  }

  async changeCourse(
    looks: CourseLooks,
    step: 'changeCourse' | 'changeVisibility' | 'changeMinimumRole' = 'changeCourse',
  ): Promise<void> {
    const before = this.course;
    if (!before) return;
    const back: CourseLooks = {
      ...(looks.title === undefined ? {} : { title: before.title }),
      ...(looks.description === undefined ? {} : { description: before.description }),
      ...(looks.icon === undefined ? {} : { icon: before.icon ?? '' }),
      ...(looks.visibility === undefined ? {} : { visibility: before.visibility }),
      ...(looks.minimumRole === undefined ? {} : { minimumRole: before.minimumRole }),
    };
    const write = async (to: CourseLooks, from: CourseLooks): Promise<void> => {
      if (this.course) this.course = { ...this.course, ...to };
      try {
        const changed = await this.workspace.backend.patchContainer(this.courseId, to);
        this.#takeCourse(changed);
        this.workspace.receiveContainer(changed);
      } catch (error) {
        if (this.course) this.course = { ...this.course, ...from };
        throw error;
      }
    };
    try {
      await this.workspace.undo.perform(
        () => write(looks, back),
        () => write(back, looks),
        step,
      );
    } catch (error) {
      this.workspace.report('save', error);
    }
  }

  // ------------------------------------------------------------ sharing / members

  get containerId(): string {
    return this.courseId;
  }

  get container(): Container | undefined {
    return this.course;
  }

  set container(course: Container | undefined) {
    this.course = course;
  }

  setOpen(open: boolean): Promise<void> {
    return this.changeCourse({ visibility: open ? 'open' : 'private' }, 'changeVisibility');
  }

  setMinimumRole(role: ContainerRole): Promise<void> {
    return this.changeCourse({ minimumRole: role }, 'changeMinimumRole');
  }

  addMember(userId: string, role: ContainerRole): Promise<void> {
    return this.#people.add(userId, role);
  }

  setMemberRole(userId: string, role: ContainerRole): Promise<void> {
    return this.#people.setRole(userId, role);
  }

  removeMember(userId: string): Promise<void> {
    return this.#people.remove(userId);
  }

  #syncUnitDates(scheduled: readonly ScheduledUnit[]): void {
    const byId: Record<string, ScheduledUnit | undefined> = Object.fromEntries(
      scheduled.map((item) => [item.unitId, item]),
    );
    this.units = this.units.map((unit) => {
      const next = byId[unit.id];
      return next ? { ...unit, scheduledDate: next.scheduledDate } : unit;
    });
  }

  get myProgress(): ReadonlyMap<string, UnitProgress> {
    return this.progress.myProgress;
  }
  get progressSummary(): CourseProgressSummary | null {
    return this.progress.progressSummary;
  }
  toggleUnitComplete(unitId: string): Promise<boolean> {
    return this.progress.toggleComplete(unitId);
  }
  setUnitLastPosition(unitId: string, position: string | null): Promise<boolean> {
    return this.progress.setLastPosition(unitId, position);
  }
  refreshProgressSummary(): Promise<void> {
    return this.progress.refreshSummary();
  }
}
