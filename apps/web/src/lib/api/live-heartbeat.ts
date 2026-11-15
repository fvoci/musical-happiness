/**
 * Whether a WebSocket still carries (`contracts/websocket.md` 3): a page
 * cannot see the websocket's own Ping and Pong, so it sends a `PING` of
 * its own a while after it last heard the server, and takes the connection
 * for dead when nothing at all comes back in time. A connection that died
 * without a word (the network changed, the lid was closed) says nothing for
 * minutes otherwise, and the server's changes stop meanwhile.
 *
 * A page in the background has its timers slowed (Chrome lets them run
 * once a minute at most) or stopped. A wait that ends much later than it
 * was set for says the page slept, not that the server is gone: the `PING`
 * is sent again, and only a second wait without an answer ends the
 * connection.
 */

/** How long after the connection opened, or the last answer, the next `PING` goes, in milliseconds. */
export const PING_EVERY = 20_000;
/** How long a `PING` waits for anything from the server, in milliseconds. */
export const PONG_WAIT = 15_000;
/** A wait that ends more than this late was a page asleep, in milliseconds. */
const LATE = 5000;

export const PING = JSON.stringify({ action: 'PING' });
/** The server's answer, as it sends it. */
export const PONG = JSON.stringify({ action: 'PONG' });

export class Heartbeat {
  readonly #ping: () => void;
  readonly #dead: () => void;
  #timer: ReturnType<typeof setTimeout> | undefined;
  /** When the `PING` that waits for an answer went; `undefined` when none waits. */
  #sentAt: number | undefined;
  /** The `PING` that waits was sent again after a wait that ended late. */
  #again = false;
  #stopped = false;

  /** `ping` sends a `PING`; `dead` is told once, when nothing answered in time. */
  constructor(ping: () => void, dead: () => void) {
    this.#ping = ping;
    this.#dead = dead;
    this.#next();
  }

  /** Something came from the server: the connection carries. */
  heard(): void {
    if (this.#stopped || this.#sentAt === undefined) return;
    this.#sentAt = undefined;
    this.#again = false;
    this.#next();
  }

  /** The connection is let go: nothing more is sent or told. */
  stop(): void {
    this.#stopped = true;
    clearTimeout(this.#timer);
    this.#timer = undefined;
  }

  #next(): void {
    clearTimeout(this.#timer);
    this.#timer = setTimeout(() => {
      this.#send();
    }, PING_EVERY);
  }

  #send(): void {
    this.#sentAt = Date.now();
    this.#ping();
    this.#timer = setTimeout(() => {
      this.#waited();
    }, PONG_WAIT);
  }

  #waited(): void {
    const sentAt = this.#sentAt;
    if (this.#stopped || sentAt === undefined) return;
    const late = Date.now() - sentAt - PONG_WAIT > LATE;
    if (late && !this.#again) {
      this.#again = true;
      this.#send();
      return;
    }
    this.stop();
    this.#dead();
  }
}
