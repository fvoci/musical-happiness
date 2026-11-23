/**
 * One open unit of a course: its body, edited together in its room.
 *
 * The title is not kept here: it lives in the course's outline (`CourseStore.units`),
 * so the outline and the unit view never disagree, and is changed through
 * `CourseStore.changeUnitTitle`. Comments and body versions belong to t5.
 */
import type { DocNode } from '../editor/schema.ts';

import {
  isBackendError,
  isOwnSpace,
  type BodyDoc,
  type BodyDocState,
  type CourseUnit,
} from './backend.ts';
import { BodyUploads } from './body-uploads.svelte.ts';
import { REJOIN_SPACING, afterPutOut } from './card-parts.ts';
import type { CourseStore } from './course.svelte.ts';
import { UnitComments } from './unit-comments.svelte.ts';
import { mentionedIn } from './user-names.ts';
import type { LoadStatus } from './workspace.svelte.ts';

const sameDoc = (first: DocNode | null, second: DocNode | null): boolean =>
  JSON.stringify(first) === JSON.stringify(second);

export class UnitStore {
  readonly course: CourseStore;
  readonly unitId: string;

  status = $state<LoadStatus>('idle');
  /** The unit is gone (deleted here or by somebody else), is a chapter, or never was. */
  missing = $state(false);
  /** The unit was shown and removed since (`others` to announce, `reader` for own deletion). */
  removed = $state.raw<{ by: 'others' | 'reader'; title: string } | undefined>(undefined);
  /** The body as stored, as far as this store knows. */
  body = $state.raw<DocNode | null>(null);
  /** The body in its room, while open (`openBody`). */
  room = $state.raw<BodyDocState | undefined>(undefined);
  /** Reasons changes were dropped, each once, until dismissed. */
  lost = $state.raw<readonly string[]>([]);
  /** Whether the reader is watching this unit for comment notifications (shared workspaces only). */
  watching = $state<boolean | undefined>(undefined);
  watchingBusy = $state(false);

  #readAt = 0;
  #bodyDoc: BodyDoc | undefined;
  #closed = false;
  #rejoinedAt = -Infinity;
  #news: CourseUnit | undefined;
  #readings = 0;
  #watchGen = 0;
  #shown = false;

  readonly comments: UnitComments;
  readonly #bodyUploads: BodyUploads;

  constructor(course: CourseStore, unitId: string) {
    this.course = course;
    this.unitId = unitId;
    this.comments = new UnitComments(course, unitId, () => {
      if (this.canWatch) {
        this.#watchGen += 1;
        this.watching = true;
      }
    });
    this.#bodyUploads = new BodyUploads(() => this.course.workspace);
  }

  get canWatch(): boolean {
    const { info, me } = this.course.workspace;
    return Boolean(info?.id && me?.id && !isOwnSpace(info));
  }

  get #courseId(): string {
    return this.course.courseId;
  }

  #read(): Promise<{ kind: 'unit' | 'chapter'; body: DocNode | null; updateAt: number }> {
    const courses = this.course.workspace.backend.course;
    if (!courses) return Promise.reject(new Error('this backend has no courses'));
    return courses.unit(this.#courseId, this.unitId);
  }

  async load(): Promise<void> {
    this.status = 'loading';
    await Promise.all([this.#readBody(true), this.comments.read(true), this.#readWatch()]);
  }

  /** Reads the body and comments again after changes may have been missed (`resync`). */
  async refresh(): Promise<void> {
    if (this.status !== 'ready') return;
    await Promise.all([
      this.#busy() ? undefined : this.#readBody(false),
      this.comments.read(false),
    ]);
  }

  async #readWatch(): Promise<void> {
    if (!this.canWatch) return;
    const courses = this.course.workspace.backend.course;
    const workspaceId = this.course.workspace.info?.id;
    const meId = this.course.workspace.me?.id;
    if (!courses || !workspaceId || !meId) return;
    this.#watchGen += 1;
    const gen = this.#watchGen;
    try {
      const ids = await courses.watching(workspaceId, meId);
      if (!this.#closed && gen === this.#watchGen) this.watching = ids.includes(this.unitId);
    } catch {
      // Non-critical: the unit opens even if subscriptions could not be read.
    }
  }

  async setWatching(next: boolean): Promise<void> {
    if (!this.canWatch || this.watchingBusy) return;
    const courses = this.course.workspace.backend.course;
    const workspaceId = this.course.workspace.info?.id;
    const meId = this.course.workspace.me?.id;
    if (!courses || !workspaceId || !meId) return;
    const before = this.watching;
    this.#watchGen += 1;
    this.watching = next;
    this.watchingBusy = true;
    try {
      if (next) await courses.watchUnit(workspaceId, this.unitId, meId);
      else await courses.unwatchUnit(workspaceId, this.unitId, meId);
    } catch (error) {
      if (this.#closed) return;
      if (!next && isBackendError(error) && error.status === 404) {
        this.watching = false;
      } else {
        this.watching = before;
        this.course.workspace.report('save', error);
      }
    } finally {
      this.watchingBusy = false;
    }
  }

  async #readBody(first: boolean): Promise<void> {
    this.#readings += 1;
    const reading = this.#readings;
    try {
      const detail = await this.#read();
      if (this.#closed || reading !== this.#readings) return;
      // A chapter has no body or room: it is not opened as a unit.
      if (detail.kind === 'chapter') {
        this.gone();
        return;
      }
      if (!first && detail.updateAt <= this.#readAt) return;
      this.#readAt = detail.updateAt;
      if (!sameDoc(detail.body, this.body)) this.body = detail.body;
      void this.course.workspace.learnUsers(mentionedIn(detail.body));
      [this.status, this.missing] = ['ready', false];
      this.#shown = true;
    } catch (error) {
      if (this.#closed || reading !== this.#readings) return;
      if (isBackendError(error) && error.status === 401) {
        if (first) this.status = 'failed';
        this.course.workspace.report('load', error);
      } else if (isBackendError(error) && (error.status === 404 || error.status === 403)) {
        this.gone();
      } else if (first) {
        this.status = 'failed';
        this.course.workspace.report('load', error);
      }
    }
  }

  close(): void {
    this.#closed = true;
    this.comments.close();
    this.closeBody();
    this.#bodyUploads.giveUp();
  }

  gone(by?: 'others' | 'reader', title = ''): void {
    if (by && this.#shown && !this.removed) this.removed = { by, title };
    this.missing = true;
    this.status = 'failed';
    this.closeBody();
  }

  /** The unit changed in the outline: re-read its body unless the room holds it. */
  unitChanged(unit: CourseUnit): void {
    if (unit.id !== this.unitId || unit.updateAt <= this.#readAt || this.missing) return;
    if (this.#busy()) {
      this.#news = unit;
      return;
    }
    void this.#readBody(false);
  }

  #busy(): boolean {
    return this.#closed || this.room?.doc !== undefined;
  }

  #catchUp(): void {
    const news = this.#news;
    if (!news || this.#busy()) return;
    this.#news = undefined;
    this.unitChanged(news);
  }

  openBody(): void {
    const backend = this.course.workspace.backend;
    const workspaceId = this.course.workspace.info?.id;
    if (this.#bodyDoc || this.#closed || this.missing || !backend.openDoc || !workspaceId) return;
    const opened = backend.openDoc(workspaceId, this.#courseId, this.unitId, {
      state: (state) => {
        if (this.#bodyDoc !== opened) return;
        const shut = state.status === 'out' && state.reason === 'not_found';
        if (shut && this.room?.reason !== 'not_found') {
          void this.#stillThere(opened, this.room?.doc !== undefined);
        }
        this.room = state;
        if (!state.doc) this.#catchUp();
      },
      discarded: (why) => {
        if (this.#bodyDoc === opened) this.sayLost(why);
      },
    });
    this.#bodyDoc = opened;
    this.room = opened.state;
  }

  async #stillThere(opened: BodyDoc, heldDoc: boolean): Promise<void> {
    const read = await afterPutOut(() => this.#read());
    if (this.#bodyDoc !== opened) return;
    if (read.found === 'there' && heldDoc && Date.now() - this.#rejoinedAt >= REJOIN_SPACING) {
      this.#rejoinedAt = Date.now();
      opened.retry();
    }
    if (read.found === 'ended') this.course.workspace.report('load', read.error);
    if (read.found === 'gone') this.gone();
  }

  closeBody(): void {
    const opened = this.#bodyDoc;
    this.#bodyDoc = undefined;
    this.room = undefined;
    opened?.close();
  }

  retryBody(): void {
    this.#bodyDoc?.retry();
  }

  settleBody(): Promise<void> {
    return this.#bodyDoc?.settle() ?? Promise.resolve();
  }

  sayLost(why: string): void {
    if (!this.lost.includes(why)) this.lost = [...this.lost, why];
  }

  dismissLost(): void {
    this.lost = [];
  }

  uploadFile(file: File): Promise<string> {
    return this.#bodyUploads.run(file, (options) =>
      this.course.workspace.backend.upload(this.#courseId, file, options),
    );
  }
}
