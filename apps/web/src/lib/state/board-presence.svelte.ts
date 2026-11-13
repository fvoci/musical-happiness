/** The visit lasts only while its board is on screen; hidden tabs leave after a minute. */
import { EMPTY_PRESENCE, type PresenceSnapshot } from './presence.ts';
import type { BoardStore } from './board.svelte.ts';

export class BoardPresence {
  readonly #board: BoardStore;
  snapshot = $state.raw<PresenceSnapshot>(EMPTY_PRESENCE);
  readonly others = $derived(
    this.snapshot.people.filter((person) => person.userId !== this.#board.workspace.me?.id),
  );
  readonly count = $derived(
    this.others.length + this.snapshot.anonymousCount + this.snapshot.hiddenCount,
  );
  #active = false;
  #hidden = false;
  #cardId: string | undefined;

  constructor(board: BoardStore) {
    this.#board = board;
  }

  setCard(cardId: string | undefined): void {
    this.#cardId = cardId;
    this.#send();
  }

  start(): () => void {
    const presence = this.#board.workspace.backend.presence;
    this.#active = true;
    this.#hidden = false;
    const stop = presence.watchPresence(this.#board.boardId, (snapshot) => {
      this.snapshot = snapshot;
      void this.#board.workspace.learnUsers(snapshot.people.map((person) => person.userId));
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
    this.#board.workspace.backend.presence.setPresence(
      this.#hidden
        ? null
        : {
            workspaceId: this.#board.workspace.info?.id ?? '',
            boardId: this.#board.boardId,
            cardId: this.#cardId,
          },
    );
  }
}
