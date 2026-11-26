/**
 * Store for the unified account-wide `'Today'` hub (`/today`): reads
 * `backend.accountToday()` across open workspaces and, when the personal space
 * is open and has a diary, computes the current streak locally via
 * `backend.diary.runs(diaryId)` without asking the server to pick a diary entry.
 */
import { dateOfInstant } from '../logic/dates.ts';
import { streakOf } from '../logic/diary-streak.ts';

import { isOwnSpace, type AccountToday } from './backend.ts';
import { isOpen } from './workspace-entry.ts';
import type { LoadStatus, Workspace } from './workspace.svelte.ts';

export class TodayHubStore {
  readonly workspace: Workspace;
  status = $state<LoadStatus>('idle');
  data = $state.raw<AccountToday | undefined>(undefined);
  diaryStreak = $state<number | undefined>(undefined);
  personalDiary = $state.raw<{ workspaceId: string; diaryId: string } | undefined>(undefined);

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

  #isClosed(): boolean {
    return this.#closed;
  }

  async load(): Promise<void> {
    if (this.#isClosed()) return;
    this.#readings += 1;
    const reading = this.#readings;
    if (!this.data) this.status = 'loading';
    try {
      const [todayData, diaryInfo] = await Promise.all([
        this.workspace.backend.accountToday(),
        this.#readPersonalDiaryStreak(),
      ]);
      if (this.#isClosed() || reading !== this.#readings) return;
      this.data = todayData;
      this.diaryStreak = diaryInfo.streak;
      this.personalDiary = diaryInfo.place;
      this.status = 'ready';
    } catch (error) {
      if (this.#isClosed() || reading !== this.#readings) return;
      this.status = 'failed';
      this.workspace.report('load', error);
    }
  }

  async #readPersonalDiaryStreak(): Promise<{
    streak: number | undefined;
    place: { workspaceId: string; diaryId: string } | undefined;
  }> {
    const diariesApi = this.workspace.backend.diary;
    const own = this.workspace.workspaces.find((ws) => isOwnSpace(ws) && isOpen(ws));
    if (!diariesApi || !own) return { streak: undefined, place: undefined };
    try {
      const existing =
        this.workspace.info?.id === own.id
          ? this.workspace.containers.find((c) => c.kind === 'diary')
          : (await diariesApi.diaries(own.id))[0];
      if (!existing) return { streak: undefined, place: undefined };
      const runs = await diariesApi.runs(existing.id);
      const deviceToday = dateOfInstant(Date.now(), this.workspace.regional.timeZone);
      const streak = streakOf(runs, deviceToday).current;
      return { streak, place: { workspaceId: own.id, diaryId: existing.id } };
    } catch {
      return { streak: undefined, place: undefined };
    }
  }

  close(): void {
    this.#closed = true;
    this.#readings += 1;
    this.#stop();
  }
}
