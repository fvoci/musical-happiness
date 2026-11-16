/**
 * The visit lasts only while its board is on screen; hidden tabs leave after a minute.
 * A wiki is visited as a board is, its page as a card.
 */
import { EMPTY_PRESENCE, type PresenceSnapshot, type PresentPerson } from './presence.ts';
import type { BoardStore } from './board.svelte.ts';
import type { Workspace } from './workspace.svelte.ts';

/** Where the reader is: a board, or a wiki. */
export interface PresencePlace {
  readonly workspace: Workspace;
  readonly containerId: string;
}

export class ContainerPresence {
  readonly #place: PresencePlace;
  snapshot = $state.raw<PresenceSnapshot>(EMPTY_PRESENCE);
  #active = false;
  #hidden = false;
  #cardId: string | undefined;

  constructor(board: PresencePlace) {
    this.#place = board;
  }

  /**
   * Everybody on the board but the reader. A getter, not a `$derived`: the presence
   * can be made first where a component derives it (`boardPresence`).
   */
  get others(): readonly PresentPerson[] {
    return this.snapshot.people.filter((person) => person.userId !== this.#place.workspace.me?.id);
  }

  /** How many others are on the board, named or not. */
  get count(): number {
    return this.others.length + this.snapshot.anonymousCount + this.snapshot.hiddenCount;
  }

  setCard(cardId: string | undefined): void {
    this.#cardId = cardId;
    this.#send();
  }

  start(): () => void {
    const presence = this.#place.workspace.backend.presence;
    this.#active = true;
    this.#hidden = false;
    const stop = presence.watchPresence(this.#place.containerId, (snapshot) => {
      this.snapshot = snapshot;
      void this.#place.workspace.learnUsers(snapshot.people.map((person) => person.userId));
    });
    let hiding: ReturnType<typeof setTimeout> | undefined;
    const visibility = (): void => {
      clearTimeout(hiding);
      if (document.hidden) {
        hiding = setTimeout(() => {
          this.#hidden = true;
          this.#send();
        }, 60_000);
      } else {
        this.#hidden = false;
        this.#send();
      }
    };
    const leaving = (): void => {
      this.#hidden = true;
      this.#send();
    };
    document.addEventListener('visibilitychange', visibility);
    window.addEventListener('pagehide', leaving);
    window.addEventListener('pageshow', visibility);
    visibility();
    this.#send();
    return () => {
      this.#active = false;
      clearTimeout(hiding);
      document.removeEventListener('visibilitychange', visibility);
      window.removeEventListener('pagehide', leaving);
      window.removeEventListener('pageshow', visibility);
      presence.setPresence(null);
      stop();
      this.snapshot = EMPTY_PRESENCE;
    };
  }

  #send(): void {
    if (!this.#active) return;
    this.#place.workspace.backend.presence.setPresence(
      this.#hidden
        ? null
        : {
            workspaceId: this.#place.workspace.info?.id ?? '',
            containerId: this.#place.containerId,
            cardId: this.#cardId,
          },
    );
  }
}

const presences = new WeakMap<BoardStore, ContainerPresence>();

/** Who is on `board`, kept beside the board's store for as long as it is. */
export function boardPresence(board: BoardStore): ContainerPresence {
  let presence = presences.get(board);
  if (!presence) {
    presence = new ContainerPresence(board);
    presences.set(board, presence);
  }
  return presence;
}
