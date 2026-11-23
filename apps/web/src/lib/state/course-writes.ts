/**
 * The writes of an open course's outline (`course.svelte.ts`): making a chapter
 * or unit, moving one (a chapter keeps its units), deleting one (a chapter with
 * its units), and bringing one back from the trash. Each shows optimistically
 * and reverts on refusal, with undo/redo support.
 */
import { isEmptyDoc } from '../editor/schema.ts';
import { isPlaced, moved, movesAnywhere, placeOf, type PagePlace } from '../logic/page-moves.ts';
import { withDescendants } from '../logic/page-tree.ts';

import {
  isBackendError,
  type Courses,
  type CourseUnit,
  type CourseUnitDetail,
  type UnitDraft,
} from './backend.ts';
import type { UnitStore } from './unit.svelte.ts';
import { unlessGone } from './wiki-parts.ts';
import { PendingWrites } from './write.ts';
import type { Workspace } from './workspace.svelte.ts';

/** What the outline writes need of `CourseStore`. */
export interface CourseTreeHost {
  readonly courseId: string;
  readonly workspace: Workspace;
  courses(): Courses;
  units(): CourseUnit[];
  setUnits(units: CourseUnit[]): void;
  openUnit(): UnitStore | undefined;
  takeUnit(unit: CourseUnit): void;
  reread(): void;
}

export class CourseWrites {
  readonly #host: CourseTreeHost;

  constructor(host: CourseTreeHost) {
    this.#host = host;
  }

  #unit(unitId: string | undefined): CourseUnit | undefined {
    return this.#host.units().find((unit) => unit.id === unitId);
  }

  forget(unitIds: readonly string[], by: 'others' | 'reader' | 'held'): void {
    const gone = unitIds.flatMap((unitId) => [...withDescendants(this.#host.units(), unitId)]);
    this.#forgettings += 1;
    for (const unitId of [...unitIds, ...gone]) {
      const shownAt = this.#unit(unitId)?.updateAt ?? -Infinity;
      const updateAt = Math.max(shownAt, this.#forgottenAt.get(unitId)?.updateAt ?? -Infinity);
      this.#forgottenAt.set(unitId, { at: this.#forgettings, updateAt });
    }
    const shown = this.#host.openUnit();
    const own = unitIds.find((unitId) => this.#deleting.has(unitId));
    const title = own === undefined ? this.#unit(shown?.unitId)?.title : this.#deleting.get(own);
    this.#host.setUnits(this.#host.units().filter((unit) => !gone.includes(unit.id)));
    if (by === 'held' || !shown || !gone.includes(shown.unitId)) return;
    shown.gone(own === undefined ? by : 'reader', title ?? '');
  }

  readonly #deleting = new Map<string, string>();

  isDeleting(unitId: string): boolean {
    return this.#deleting.has(unitId);
  }

  #forgettings = 0;
  readonly #forgottenAt = new Map<string, { at: number; updateAt: number }>();

  get forgettings(): number {
    return this.#forgettings;
  }

  forgottenSince(mark: number, unitId: string): boolean {
    return (this.#forgottenAt.get(unitId)?.at ?? 0) > mark;
  }

  admits(unit: CourseUnit): boolean {
    const forgotten = this.#forgottenAt.get(unit.id);
    if (forgotten && unit.updateAt <= forgotten.updateAt) return false;
    if (unit.parentId !== '' && this.#forgottenAt.has(unit.parentId)) {
      if (this.#unit(unit.id)) this.forget([unit.id], 'others');
      const kept = this.#forgottenAt.get(unit.id)?.updateAt ?? -Infinity;
      this.#forgottenAt.set(unit.id, {
        at: this.#forgettings,
        updateAt: Math.max(kept, unit.updateAt),
      });
      return false;
    }
    this.#forgottenAt.delete(unit.id);
    return true;
  }

  remember(units: readonly CourseUnit[]): void {
    for (const unit of units) this.#forgottenAt.delete(unit.id);
  }

  admitted(units: readonly CourseUnit[], mark: number): CourseUnit[] {
    const byId = new Map(units.map((unit) => [unit.id, unit]));
    const gone = (unit: CourseUnit): boolean => {
      const seen = new Set<string>();
      for (
        let at: CourseUnit | undefined = unit;
        at && !seen.has(at.id);
        at = byId.get(at.parentId)
      ) {
        seen.add(at.id);
        if (this.forgottenSince(mark, at.id)) return true;
        if (at.parentId !== '' && !byId.has(at.parentId) && this.#forgottenAt.has(at.parentId)) {
          return true;
        }
      }
      return false;
    };
    return units.filter((unit) => !gone(unit));
  }

  readonly #leaving = new Set<Promise<void>>();

  async deletionsAnswered(): Promise<void> {
    await Promise.allSettled([...this.#leaving]);
  }

  async #deleteUnit(
    unitId: string,
    title = this.#unit(this.#host.openUnit()?.unitId)?.title ?? '',
  ): Promise<void> {
    this.#deleting.set(unitId, title);
    const leaving = this.#host.courses().deleteUnit(this.#host.courseId, unitId);
    this.#leaving.add(leaving);
    try {
      await leaving;
    } finally {
      this.#leaving.delete(leaving);
      setTimeout(() => {
        this.#deleting.delete(unitId);
      }, 0);
    }
  }

  async createUnit(draft: UnitDraft): Promise<CourseUnitDetail | undefined> {
    let kept: CourseUnitDetail | undefined;
    try {
      return await this.#host.workspace.undo.perform(
        async () => {
          if (kept) {
            const again = kept;
            kept = undefined;
            return again;
          }
          const made = await this.#host.courses().createUnit(this.#host.courseId, draft);
          this.#host.takeUnit(made);
          return made;
        },
        async (made) => {
          if (await this.#grownSince(made.id)) {
            kept = made;
            return;
          }
          await unlessGone(() => this.#deleteUnit(made.id));
          this.forget([made.id], 'reader');
        },
        'createUnit',
      );
    } catch (error) {
      this.#host.workspace.report('save', error);
      return undefined;
    }
  }

  async #grownSince(unitId: string): Promise<boolean> {
    if (this.#host.units().some((unit) => unit.parentId === unitId)) return true;
    try {
      const { kind, body } = await this.#host.courses().unit(this.#host.courseId, unitId);
      return kind === 'unit' && body !== null && !isEmptyDoc(body);
    } catch (error) {
      if (isBackendError(error) && error.status === 404) return false;
      throw error;
    }
  }

  async deleteUnit(unitId: string): Promise<boolean> {
    try {
      await this.#host.workspace.undo.perform(
        () => this.#deleteShown(unitId),
        () => unlessGone(() => this.restoreUnit(unitId)),
        'deleteUnit',
      );
      return true;
    } catch (error) {
      this.#host.workspace.report('save', error);
      return false;
    }
  }

  async #deleteShown(unitId: string): Promise<void> {
    const before = this.#host.units();
    const gone = withDescendants(before, unitId);
    const title = this.#unit(this.#host.openUnit()?.unitId)?.title ?? '';
    const marks = [...gone].map((id) => [id, this.#forgottenAt.get(id)] as const);
    this.forget([unitId], 'held');
    try {
      await this.#deleteUnit(unitId, title);
    } catch (error) {
      if (!isBackendError(error) || error.status !== 404) {
        this.#host.setUnits(before);
        for (const [id, mark] of marks) {
          if (mark) this.#forgottenAt.set(id, mark);
          else this.#forgottenAt.delete(id);
        }
        throw error;
      }
    }
    const shown = this.#host.openUnit();
    if (shown && gone.has(shown.unitId)) shown.gone('reader', title);
  }

  async restoreUnit(unitId: string): Promise<void> {
    const back = await this.#host.courses().undeleteUnit(this.#host.courseId, unitId);
    this.#host.takeUnit(back);
    this.#host.reread();
  }

  moveUnit(unitId: string, to: PagePlace): Promise<boolean> {
    const turn =
      this.#queued === 0
        ? this.#moveUnit(unitId, to)
        : this.#queue.then(() => this.#moveUnit(unitId, to));
    this.#queued += 1;
    this.#queue = turn.finally(() => {
      this.#queued -= 1;
    });
    return turn;
  }

  async #moveUnit(unitId: string, to: PagePlace): Promise<boolean> {
    if (!movesAnywhere(this.#host.units(), unitId, to)) return true;
    const back = placeOf(this.#host.units(), unitId);
    if (!back) return false;
    let first = true;
    try {
      await this.#host.workspace.undo.perform(
        () => {
          if (!first) return unlessGone(() => this.#move(unitId, to));
          first = false;
          return this.#move(unitId, to);
        },
        () => unlessGone(() => this.#moveBack(unitId, back)),
        'moveUnit',
      );
      return true;
    } catch (error) {
      if (isBackendError(error) && error.status === 404) {
        this.forget([unitId], 'others');
        this.#host.workspace.report('moveUnit', error);
      } else this.#host.workspace.report('save', error);
      return false;
    }
  }

  readonly #moving = new Map<string, number>();
  readonly #pending = new PendingWrites<CourseUnit>();
  readonly #unmoved = new Map<string, CourseUnit>();
  #queue: Promise<unknown> = Promise.resolve();
  #queued = 0;

  receive(unit: CourseUnit): boolean {
    return this.#pending.receive(unit);
  }

  async #move(unitId: string, to: PagePlace): Promise<void> {
    const shown = this.#unit(unitId);
    if (shown && !this.#moving.has(unitId)) this.#unmoved.set(unitId, shown);
    this.#moving.set(unitId, (this.#moving.get(unitId) ?? 0) + 1);
    this.#pending.begin(unitId);
    this.#host.setUnits(moved(this.#host.units(), unitId, to));
    let answer: CourseUnit | undefined;
    try {
      answer = await this.#host.courses().moveUnit(this.#host.courseId, unitId, to);
    } finally {
      const latest = this.#pending.end(unitId, answer);
      const left = (this.#moving.get(unitId) ?? 1) - 1;
      if (left > 0) this.#moving.set(unitId, left);
      else {
        this.#moving.delete(unitId);
        const unmoved = this.#unmoved.get(unitId);
        this.#unmoved.delete(unitId);
        const show = latest ?? unmoved;
        if (show) this.#host.takeUnit(show);
        if (answer && !isPlaced(this.#host.units(), unitId, to)) this.#host.reread();
      }
    }
  }

  async #moveBack(unitId: string, back: PagePlace): Promise<void> {
    try {
      await this.#move(unitId, back);
    } catch (error) {
      if (!isBackendError(error) || error.status !== 400) throw error;
      const parentRefused = error.refusalAt('/parentId') !== undefined;
      if (parentRefused && error.reason === 'page_too_deep') throw error;
      await this.#move(unitId, { parentId: parentRefused ? '' : back.parentId });
    }
  }
}
