/**
 * The account's hub of today (`/today`): what `backend.accountToday()` reads
 * across the open spaces, and a line for each diary of the reader's own
 * space, its days in a row reckoned here from the days it has written
 * (`backend.diary.runs`): the server never picks the day. Done is given to a
 * card from here by its board's rule (`doneWhen`), one chosen here where the
 * board has none, and a unit due is put off by days.
 */
import { addDays, dateOfInstant, type CalendarDate } from '../logic/dates.ts';
import { streakOf } from '../logic/diary-streak.ts';
import { doneValue } from '../logic/done-rule.ts';
import type { Container, DoneRule } from '../logic/wire.ts';

import { isOwnSpace, type AccountToday } from './backend.ts';
import type { TodayCardItem, TodayUnitEntry } from './authority-types.ts';
import type { DiaryRun } from './diary-types.ts';
import { isOpen } from './workspace-entry.ts';
import type { LoadStatus, Workspace } from './workspace.svelte.ts';

/** A diary of the reader's own space, as today finds it. */
export interface TodayDiary {
  workspaceId: string;
  diaryId: string;
  title: string;
  /** Days written in a row up to today, or up to yesterday while today is not written yet. */
  streak: number;
  writtenToday: boolean;
}

/** A board of the cards due, read for its rule of done, with what the reader may do on it. */
export interface TodayBoard {
  board: Container;
  /** A card of it may be made done. */
  canEdit: boolean;
  /** Its rule of done may be chosen. */
  canChooseDone: boolean;
}

/** Whether `runs` have `today` among their days. */
function writtenOn(runs: readonly DiaryRun[], today: CalendarDate): boolean {
  return runs.some((run) => run.from <= today && today <= run.to);
}

export class TodayHubStore {
  readonly workspace: Workspace;
  status = $state<LoadStatus>('idle');
  data = $state.raw<AccountToday | undefined>(undefined);
  diaries = $state.raw<readonly TodayDiary[]>([]);
  /** The boards of the cards due, as read for their rules of done. */
  boards = $state.raw<readonly TodayBoard[]>([]);

  #readings = 0;
  #closed = false;
  readonly #stop: () => void;

  constructor(workspace: Workspace) {
    this.workspace = workspace;
    this.#stop = workspace.backend.watchSelf((change) => {
      if (this.#closed) return;
      if (change.kind === 'resync' || change.kind === 'notifications') {
        void this.load();
      }
    });
  }

  /** Read after an answer comes: the store may have closed meanwhile. */
  #isClosed(): boolean {
    return this.#closed;
  }

  async load(): Promise<void> {
    if (this.#isClosed()) return;
    this.#readings += 1;
    const reading = this.#readings;
    if (!this.data) this.status = 'loading';
    try {
      const [today, diaries] = await Promise.all([
        this.workspace.backend.accountToday(),
        this.#readDiaries(),
      ]);
      if (this.#isClosed() || reading !== this.#readings) return;
      this.data = today;
      this.diaries = diaries;
      this.status = 'ready';
      void this.#readBoards(today.cards, reading);
    } catch (error) {
      if (this.#isClosed() || reading !== this.#readings) return;
      this.status = 'failed';
      this.workspace.report('load', error);
    }
  }

  /** The board of `boardId`, once read. */
  board(boardId: string): TodayBoard | undefined {
    return this.boards.find((one) => one.board.id === boardId);
  }

  /** Gives `card` the value its board's rule calls done, and leaves it off the list. */
  async markDone(card: TodayCardItem): Promise<void> {
    const board = this.board(card.boardId)?.board;
    const rule = board?.doneWhen;
    if (!board || !rule) return;
    const backend = this.workspace.backend;
    const property = board.cardProperties.find((prop) => prop.id === rule.propertyId);
    this.#withoutCard(card.cardId);
    try {
      const had =
        property?.type === 'multiSelect'
          ? (await backend.card(card.boardId, card.cardId)).fields.properties[rule.propertyId]
          : undefined;
      const value = doneValue(rule, property, had);
      if (value === undefined) return;
      await backend.patchCard(card.boardId, card.cardId, {
        updatedProperties: { [rule.propertyId]: value },
      });
    } catch (error) {
      this.workspace.report('save', error);
      void this.load();
    }
  }

  /** Makes `rule` what calls a card of `boardId` done; the cards it calls done already leave. */
  async chooseDone(boardId: string, rule: DoneRule): Promise<void> {
    try {
      const changed = await this.workspace.backend.patchContainer(boardId, { doneWhen: rule });
      this.boards = this.boards.map((one) =>
        one.board.id === boardId ? { ...one, board: changed } : one,
      );
      this.workspace.receiveContainer(changed);
      void this.load();
    } catch (error) {
      this.workspace.report('save', error);
    }
  }

  /** Puts `unit` off to `days` after today, on the reader's own plan of its course. */
  async putOff(unit: TodayUnitEntry, days: number): Promise<void> {
    const course = this.workspace.backend.course;
    const today = this.data?.today;
    if (!course || !today) return;
    const plannedDate = addDays(today as CalendarDate, days);
    this.#without((data) => ({
      ...data,
      units: data.units.filter((one) => one.unitId !== unit.unitId),
    }));
    try {
      await course.putUnitSchedule(
        unit.courseId,
        unit.unitId,
        { plannedDate },
        unit.workspace.workspaceId,
      );
    } catch (error) {
      this.workspace.report('save', error);
      void this.load();
    }
  }

  close(): void {
    this.#closed = true;
    this.#readings += 1;
    this.#stop();
  }

  #withoutCard(cardId: string): void {
    this.#without((data) => ({
      ...data,
      cards: data.cards.filter((one) => one.cardId !== cardId),
    }));
  }

  #without(change: (data: AccountToday) => AccountToday): void {
    if (this.data) this.data = change(this.data);
  }

  /** The boards of `cards` not read yet; one that cannot be read is left without a way to done. */
  async #readBoards(cards: readonly TodayCardItem[], reading: number): Promise<void> {
    const ids = cards
      .map((card) => card.boardId)
      .filter((id, at, all) => all.indexOf(id) === at && !this.board(id));
    const backend = this.workspace.backend;
    const read = await Promise.all(
      ids.map((id) =>
        Promise.all([backend.container(id), backend.myPermissions(id)]).then(
          ([board, may]): TodayBoard => ({
            board,
            canEdit: may.canEdit,
            canChooseDone: may.canManageProperties,
          }),
          () => undefined,
        ),
      ),
    );
    if (this.#isClosed() || reading !== this.#readings) return;
    const fresh = read.filter((one) => one !== undefined);
    if (fresh.length > 0) this.boards = [...this.boards, ...fresh];
  }

  /** The diaries of the reader's own space, each with its days in a row; none where it is shut. */
  async #readDiaries(): Promise<TodayDiary[]> {
    const diariesApi = this.workspace.backend.diary;
    const own = this.workspace.workspaces.find((ws) => isOwnSpace(ws) && isOpen(ws));
    if (!diariesApi || !own) return [];
    try {
      const volumes =
        this.workspace.info?.id === own.id
          ? this.workspace.containers.filter((container) => container.kind === 'diary')
          : await diariesApi.diaries(own.id);
      const today = dateOfInstant(Date.now(), this.workspace.regional.timeZone);
      return await Promise.all(
        volumes.map(async (volume) => {
          const runs = await diariesApi.runs(volume.id);
          return {
            workspaceId: own.id,
            diaryId: volume.id,
            title: volume.title,
            streak: streakOf(runs, today).current,
            writtenToday: writtenOn(runs, today),
          };
        }),
      );
    } catch {
      return [];
    }
  }
}
