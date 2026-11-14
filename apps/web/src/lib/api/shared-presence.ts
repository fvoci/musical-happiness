/** A public-link visit announces only its token and never subscribes to names or changes. */
import type { Presence, PresenceVisit } from '../state/presence.ts';
import { LivePresence } from './live-presence.ts';

export class SharedPresence implements Presence {
  readonly #url: string;
  readonly #boardId: string;
  readonly #live: LivePresence;
  #socket: WebSocket | undefined;
  #retry: ReturnType<typeof setTimeout> | undefined;
  #wait = 500;

  constructor(basePath: string, boardId: string, readToken: string) {
    const url = new URL(`${basePath}/ws`, location.href);
    url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:';
    this.#url = url.href;
    this.#boardId = boardId;
    this.#live = new LivePresence(
      (command) => {
        this.#socket?.send(JSON.stringify({ ...command, workspaceId: '', readToken }));
      },
      () => {
        if (!this.#live.wanted) this.#stop();
        else if (!this.#socket && this.#retry === undefined) this.#open();
      },
    );
  }

  setPresence(visit: PresenceVisit | null): void {
    this.#live.setPresence(visit?.boardId === this.#boardId ? visit : null);
  }

  watchPresence: Presence['watchPresence'] = () => () => undefined;

  #open(): void {
    if (!this.#live.wanted || this.#socket) return;
    const socket = new WebSocket(this.#url);
    this.#socket = socket;
    socket.addEventListener('open', () => {
      if (this.#socket !== socket) return;
      this.#live.up();
    });
    socket.addEventListener('close', () => {
      if (this.#socket !== socket) return;
      this.#socket = undefined;
      this.#live.down();
      if (!this.#live.wanted) return;
      this.#retry = setTimeout(() => {
        this.#retry = undefined;
        this.#open();
      }, this.#wait);
      this.#wait = Math.min(this.#wait * 2, 30_000);
    });
  }

  #stop(): void {
    clearTimeout(this.#retry);
    this.#retry = undefined;
    this.#wait = 500;
    const socket = this.#socket;
    this.#socket = undefined;
    this.#live.down();
    socket?.close();
  }
}
