/** Ephemeral visits: a connection views one board and at most one card. */
export interface PresenceVisit {
  workspaceId: string;
  boardId: string;
  cardId?: string;
}

export interface PresentPerson {
  userId: string;
  /** The union of cards opened in this person's tabs. */
  cardIds: readonly string[];
}

export interface PresenceSnapshot {
  people: readonly PresentPerson[];
  anonymousCount: number;
  hiddenCount: number;
}

export interface Presence {
  setPresence(visit: PresenceVisit | null): void;
  watchPresence(boardId: string, hear: (snapshot: PresenceSnapshot) => void): () => void;
}

/** Where a backend keeps who is on its boards. */
export interface PresenceOfBoards {
  readonly presence: Presence;
}

export const EMPTY_PRESENCE: PresenceSnapshot = {
  people: [],
  anonymousCount: 0,
  hiddenCount: 0,
};
