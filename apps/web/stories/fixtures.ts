/** Public, deterministic examples. Never import the application's populated seed. */
import { createCard, type Block, type Card } from '$lib/logic/wire.ts';

export const STORY_NOW = Date.UTC(2026, 10, 13, 3);

export function exampleBlock(id: string, type: Block['type'] = 'card'): Block {
  return {
    id,
    type,
    boardId: 'example-board',
    parentId: 'example-board',
    createdBy: 'example-reader',
    modifiedBy: 'example-reader',
    schema: 1,
    title: '',
    fields: {},
    createAt: STORY_NOW,
    updateAt: STORY_NOW,
    deleteAt: 0,
  };
}

export function exampleCard(index: number): Card {
  return createCard({
    ...exampleBlock(`example-card-${index}`),
    title: `Example card ${String(index).padStart(3, '0')} · 예시 카드`,
  });
}
