/**
 * The members of a board or a wiki as its admins change them: who is added,
 * whose role changes, who is taken off. Each change shows at once; refused, it
 * goes back, is told (`workspace.report`), and the members are read again,
 * unless a list told meanwhile is newer than that reading.
 */
import type { Board, BoardMember } from '../logic/wire.ts';

import type { BoardRole } from './backend.ts';
import { roleFlags } from './board-parts.ts';
import type { Workspace } from './workspace.svelte.ts';

/** What holds the members of one board or wiki. */
export interface MembersOf {
  readonly workspace: Workspace;
  readonly boardId: string;
  members: BoardMember[];
}

/**
 * A board or a wiki as the dialogs of its people read and change it
 * (`ShareDialog`, `BoardTeams`): its openness, the role of who joins it, and
 * its members.
 */
export interface Shareable extends MembersOf {
  readonly board: Board | undefined;
  readonly orderedMembers: BoardMember[];
  setOpen(open: boolean): Promise<void>;
  setMinimumRole(role: BoardRole): Promise<void>;
  addMember(userId: string, role: BoardRole): Promise<void>;
  setMemberRole(userId: string, role: BoardRole): Promise<void>;
  removeMember(userId: string): Promise<void>;
}

export class MemberWrites {
  readonly #of: MembersOf;
  /** Lists of members told by changes: a reading begun before the latest does not replace it. */
  told = 0;

  constructor(of: MembersOf) {
    this.#of = of;
  }

  /** Makes a person of the workspace a member. Somebody who is a member already keeps their role. */
  async add(userId: string, role: BoardRole): Promise<void> {
    const of = this.#of;
    try {
      const member = await of.workspace.backend.addMember(of.boardId, userId, role);
      of.members = [...of.members.filter((one) => one.userId !== userId), member];
      void of.workspace.learnUsers([userId], { members: true });
    } catch (error) {
      of.workspace.report('save', error);
    }
  }

  async setRole(userId: string, role: BoardRole): Promise<void> {
    const of = this.#of;
    await this.#write(
      of.members.map((member) =>
        member.userId === userId ? { ...member, ...roleFlags(role) } : member,
      ),
      () => of.workspace.backend.setMemberRole(of.boardId, userId, role),
    );
  }

  async remove(userId: string): Promise<void> {
    const of = this.#of;
    await this.#write(
      of.members.filter((member) => member.userId !== userId),
      () => of.workspace.backend.removeMember(of.boardId, userId),
    );
  }

  /**
   * Shows a change of the members at once. Refused, they go back and are read
   * again: what was put back may be older than what arrived meanwhile.
   */
  async #write(optimistic: BoardMember[], send: () => Promise<unknown>): Promise<void> {
    const of = this.#of;
    const before = of.members;
    of.members = optimistic;
    try {
      await send();
    } catch (error) {
      of.members = before;
      of.workspace.report('save', error);
      const told = this.told;
      try {
        const members = await of.workspace.backend.members(of.boardId);
        // Members told meanwhile are newer than this reading.
        if (told === this.told) of.members = members;
      } catch {
        // The next message, or the next reading, brings them.
      }
    }
  }
}
