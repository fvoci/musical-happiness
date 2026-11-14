/**
 * The versions of a card's body on the server (`getCardBodyVersions`,
 * `getCardBodyVersion`, `restoreCardBodyVersion`). The card restored comes
 * to the board as any change does, through the socket: the answer's copy of
 * it is not used.
 */
import type { BodyVersion, BodyVersionContent, BodyVersions } from '../state/body-versions.ts';

import { bodyOf, type Schemas } from './convert.ts';
import type { RequestOptions } from './http.ts';
import type { WorkspacePath } from './requests.ts';

/** A request under the workspace of the board that `params.boardId` names. */
type Ask = <T>(
  path: WorkspacePath,
  params: Record<string, string>,
  options?: RequestOptions,
) => Promise<T>;

export class ServerBodyVersions implements BodyVersions {
  readonly #ask: Ask;

  constructor(ask: Ask) {
    this.#ask = ask;
  }

  list(boardId: string, cardId: string): Promise<BodyVersion[]> {
    return this.#ask<Schemas['BodyVersion'][]>('/cards/{cardId}/body-versions', {
      cardId,
      boardId,
    });
  }

  async read(boardId: string, cardId: string, version: number): Promise<BodyVersionContent> {
    const kept = await this.#ask<Schemas['BodyVersionContent']>(
      '/cards/{cardId}/body-versions/{version}',
      { cardId, boardId, version: String(version) },
    );
    return { ...kept, body: bodyOf(kept.body) };
  }

  async restore(boardId: string, cardId: string, version: number): Promise<void> {
    await this.#ask<Schemas['Card']>(
      '/cards/{cardId}/body-versions/{version}/restore',
      { cardId, boardId, version: String(version) },
      { method: 'POST' },
    );
  }
}
