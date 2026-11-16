/**
 * The versions of the bodies of cards, and of the pages of wikis, as the
 * server keeps them (`getCardBodyVersions`, `getPageBodyVersions`), for the
 * backend in memory:
 *
 * - the body a card has before somebody writes in its room is kept first
 *   (by nobody, as of the card's last change, when nobody is known to have
 *   written it), and what is written in the room is kept when those who
 *   wrote it have left it (whoever stays and wrote nothing), and when the
 *   room writes the card after the body has been written on for ten minutes;
 * - a body written whole is kept, by its writer, after the body it was
 *   written over; a version restored is such a write;
 * - a body that is the newest version already is not kept again, no body is
 *   no first version, and the newest fifty are kept.
 *
 * Unlike the server, two bodies are the same when they are written the
 * same (their members in the same order), and the versions of a card that
 * is deleted for good stay for the life of the world.
 */
import type { DocNode } from '../../editor/schema.ts';
import type { BodyVersion, BodyVersionContent, BodyVersions } from '../body-versions.ts';

import { BackendError } from '../problems.ts';

import { memberRole } from './board-access.ts';
import { checkedBody } from './bodies.ts';
import { fail } from './refuse.ts';
import { copy, pageOf, wireOf } from './shapes.ts';
import type { StoredCard, World } from './world.ts';

/** How many versions of a body are kept (the server's `collab_body_versions`). */
export const VERSIONS_KEPT = 50;
/** How long a body is written on before a version of it is kept (`collab_version_minutes`). */
export const VERSION_EVERY = 10 * 60_000;

interface Kept {
  version: number;
  keptAt: number;
  editors: string[];
  body: DocNode | null;
}

/** What is kept of one card's body. */
export interface KeptOfCard {
  /** The oldest first. */
  versions: Kept[];
  /** Who wrote the body since its newest version. */
  unkept: Set<string>;
  /** When what is written on is kept next; undefined while nobody wrote since the newest version. */
  due: number | undefined;
}

function keptOf(world: World, cardId: string): KeptOfCard {
  let kept = world.bodyVersions.get(cardId);
  if (!kept) {
    kept = { versions: [], unkept: new Set(), due: undefined };
    world.bodyVersions.set(cardId, kept);
  }
  return kept;
}

function same(one: DocNode | null, other: DocNode | null): boolean {
  return JSON.stringify(one) === JSON.stringify(other);
}

/**
 * Keeps a card's body as its newest version, by those who wrote since the
 * one before and `writtenBy`; not a body that is the newest version
 * already, and not no body as the first.
 */
function keep(world: World, card: StoredCard, writtenBy?: string): void {
  const kept = keptOf(world, card.id);
  const body = card.body ?? null;
  const newest = kept.versions.at(-1);
  const editors = new Set(kept.unkept);
  kept.unkept.clear();
  kept.due = undefined;
  if (newest ? same(newest.body, body) : body === null) return;
  if (writtenBy !== undefined) editors.add(writtenBy);
  kept.versions.push({
    version: (newest?.version ?? 0) + 1,
    // Nobody is known to have written it: it is as old as the card's last change at the latest.
    keptAt: editors.size === 0 ? card.updateAt : world.now(),
    editors: [...editors].sort(),
    body: copy(body),
  });
  kept.versions.splice(0, Math.max(0, kept.versions.length - VERSIONS_KEPT));
}

/** A change of `userId` is taken into a card's room: the body before those who write now began is kept first. */
export function typedBy(world: World, card: StoredCard, userId: string): void {
  const kept = keptOf(world, card.id);
  if (kept.due === undefined) {
    keep(world, card);
    kept.due = Date.now() + VERSION_EVERY;
  }
  kept.unkept.add(userId);
}

/** A card's room wrote its body: kept, once it has been written on for long enough. */
export function roomWrote(world: World, card: StoredCard): void {
  const due = keptOf(world, card.id).due;
  if (due !== undefined && Date.now() >= due) keep(world, card);
}

/** Whether somebody wrote a card's body since its newest version, and none of `staying`, the users still in its room to write. */
export function leftByWriters(world: World, cardId: string, staying: string[]): boolean {
  const unkept = keptOf(world, cardId).unkept;
  return unkept.size > 0 && !staying.some((userId) => unkept.has(userId));
}

/** Those who wrote have left a card's room, and its body is written: what they wrote is kept. */
export function writersLeft(world: World, card: StoredCard): void {
  if (keptOf(world, card.id).unkept.size > 0) keep(world, card);
}

/**
 * Writes a card's body whole (`write` does, to `body`), as `userId`: the
 * body before is kept, then the body written, as the writer's. Nothing is
 * kept around a write of the body the card has.
 */
export function writtenWhole(
  world: World,
  card: StoredCard,
  body: DocNode | null,
  userId: string,
  write: () => void,
): void {
  const changes = !same(card.body ?? null, body);
  if (changes) keep(world, card);
  write();
  if (changes) keep(world, card, userId);
}

/**
 * The body of a version as it is written to its card again: held to what a
 * body a client sends is held to, and refused as a conflict (409) when it
 * may not be stored any more or shows an image whose file is gone.
 */
function storable(world: World, containerId: string, body: DocNode | null): DocNode | null {
  try {
    return checkedBody(world, containerId, body);
  } catch (error) {
    if (!(error instanceof BackendError)) throw error;
    fail(409, `the version cannot be restored: ${error.message}`);
  }
}

/**
 * The versions of a world's cards, or of its wikis' pages (`of`), read and
 * restored as one of its users: the board's or the wiki's members read them,
 * its editors restore them.
 */
export class MemoryBodyVersions implements BodyVersions {
  readonly #world: World;
  readonly #userId: string;
  /** The wait every call of the backend starts with, and the failure a test asked for. */
  readonly #enter: () => Promise<void>;
  readonly #of: 'card' | 'page';

  constructor(
    world: World,
    userId: string,
    enter: () => Promise<void>,
    of: 'card' | 'page' = 'card',
  ) {
    this.#world = world;
    this.#userId = userId;
    this.#enter = enter;
    this.#of = of;
  }

  get #kind(): 'board' | 'wiki' {
    return this.#of === 'page' ? 'wiki' : 'board';
  }

  async list(containerId: string, cardId: string): Promise<BodyVersion[]> {
    await this.#enter();
    return this.#described(this.#card(containerId, cardId)).map(
      ({ body: _, ...version }) => version,
    );
  }

  async read(containerId: string, cardId: string, version: number): Promise<BodyVersionContent> {
    await this.#enter();
    const kept = this.#described(this.#card(containerId, cardId));
    return kept.find((one) => one.version === version) ?? fail(404, 'no such version');
  }

  async restore(containerId: string, cardId: string, version: number): Promise<void> {
    await this.#enter();
    const world = this.#world;
    const role = memberRole(world, this.#userId, containerId, this.#kind);
    const refuse = (): void => {
      if (role !== 'admin' && role !== 'editor') {
        fail(403, `access denied to restore a version of the ${this.#of}'s body`);
      }
    };
    // As the server: a card is looked for first, a page's wiki asked first.
    if (this.#of === 'page') refuse();
    const card = this.#card(containerId, cardId);
    refuse();
    const kept = keptOf(world, cardId).versions.find((one) => one.version === version);
    if (!kept) fail(404, 'no such version');
    // What the room's editors typed is in the card first, and is kept with the body before.
    world.bodyFlushed?.(cardId);
    if (same(card.body ?? null, kept.body)) return;
    const body = storable(world, containerId, kept.body);
    writtenWhole(world, card, body, this.#userId, () => {
      card.body = body;
      card.updateAt = world.now();
      card.modifiedBy = this.#userId;
    });
    world.bodyWritten?.(cardId);
    if (this.#of === 'page') {
      world.tellContainer(containerId, { kind: 'pages', containerId, pages: [pageOf(card)] });
    } else
      world.tellContainer(containerId, {
        kind: 'blocks',
        containerId,
        blocks: [wireOf(card)],
      });
  }

  /** A card of the board (a page of the wiki), for a member of it: the versions are theirs to read. */
  #card(containerId: string, cardId: string): StoredCard {
    memberRole(this.#world, this.#userId, containerId, this.#kind);
    const block = this.#world.blocks.get(cardId);
    if (block?.type !== this.#of || block.containerId !== containerId)
      fail(404, `no such ${this.#of}`);
    return block as StoredCard;
  }

  /**
   * The versions of a card, the newest first, the newest with the card's
   * body marked: none while the card's room holds what the card does not yet.
   */
  #described(card: StoredCard): BodyVersionContent[] {
    const kept = [...keptOf(this.#world, card.id).versions].reverse();
    const current = this.#world.bodyUnwritten?.(card.id)
      ? undefined
      : kept.find((one) => same(one.body, card.body ?? null))?.version;
    return kept.map((one) => ({
      ...copy(one),
      isCurrent: one.version === current,
    }));
  }
}
