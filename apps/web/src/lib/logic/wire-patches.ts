/**
 * Wire patch builders for blocks, card property definitions, and containers,
 * re-exported through `wire.ts`.
 */
import type { Block, BlockPatch } from './wire-blocks.ts';
import type {
  Container,
  ContainerPatch,
  ContainersAndBlocksPatch,
  PropertyTemplate,
} from './wire.ts';

type Members = object | null | undefined;

function keysOnlyIn(first: Members, second: Members): string[] {
  const other = new Set(Object.keys(second ?? {}));
  return Object.keys(first ?? {}).filter((key) => !other.has(key));
}

function changedMembers(
  from: Members,
  to: Members,
  skip: readonly string[],
): Record<string, unknown> {
  const fromRecord = (from ?? {}) as Record<string, unknown>;
  const toRecord = (to ?? {}) as Record<string, unknown>;
  const changed: Record<string, unknown> = {};
  for (const key of Object.keys(fromRecord)) {
    if (!skip.includes(key) && fromRecord[key] !== toRecord[key]) changed[key] = fromRecord[key];
  }
  return changed;
}

export function createPatchesFromBlocks(
  newBlock: Block,
  oldBlock: Block,
): [BlockPatch, BlockPatch] {
  return [
    {
      ...changedMembers(newBlock, oldBlock, ['fields']),
      updatedFields: changedMembers(newBlock.fields, oldBlock.fields, []),
      deletedFields: keysOnlyIn(oldBlock.fields, newBlock.fields),
    },
    {
      ...changedMembers(oldBlock, newBlock, ['fields']),
      updatedFields: changedMembers(oldBlock.fields, newBlock.fields, []),
      deletedFields: keysOnlyIn(newBlock.fields, oldBlock.fields),
    },
  ];
}

function templatesEqual(first: PropertyTemplate, second: PropertyTemplate): boolean {
  if (Object.keys(changedMembers(first, second, ['options'])).length > 0) return false;
  if (first.options.length !== second.options.length) return false;
  return first.options.every((option) => {
    const other = second.options.find((candidate) => candidate.id === option.id);
    return other !== undefined && Object.keys(changedMembers(option, other, [])).length === 0;
  });
}

function changedTemplates(
  templates: readonly PropertyTemplate[],
  others: readonly PropertyTemplate[],
): PropertyTemplate[] {
  return templates.filter((template) => {
    const other = others.find((candidate) => candidate.id === template.id);
    return !other || !templatesEqual(template, other);
  });
}

function missingTemplateIds(
  templates: readonly PropertyTemplate[],
  others: readonly PropertyTemplate[],
): string[] {
  return templates
    .filter((template) => !others.some((candidate) => candidate.id === template.id))
    .map((template) => template.id);
}

export function createCardPropertiesPatches(
  newCardProperties: readonly PropertyTemplate[],
  oldCardProperties: readonly PropertyTemplate[],
): [ContainerPatch, ContainerPatch] {
  return [
    {
      updatedCardProperties: changedTemplates(newCardProperties, oldCardProperties),
      deletedCardProperties: missingTemplateIds(oldCardProperties, newCardProperties),
    },
    {
      updatedCardProperties: changedTemplates(oldCardProperties, newCardProperties),
      deletedCardProperties: missingTemplateIds(newCardProperties, oldCardProperties),
    },
  ];
}

const BOARD_MEMBERS_WITH_OWN_PATCH = ['properties', 'cardProperties'];

export function createPatchesFromContainers(
  newBoard: Container,
  oldBoard: Container,
): [ContainerPatch, ContainerPatch] {
  const [cardPropertiesPatch, cardPropertiesUndoPatch] = createCardPropertiesPatches(
    newBoard.cardProperties,
    oldBoard.cardProperties,
  );
  return [
    {
      ...changedMembers(newBoard, oldBoard, BOARD_MEMBERS_WITH_OWN_PATCH),
      ...cardPropertiesPatch,
      updatedProperties: changedMembers(newBoard.properties, oldBoard.properties, []),
      deletedProperties: keysOnlyIn(oldBoard.properties, newBoard.properties),
    },
    {
      ...changedMembers(oldBoard, newBoard, BOARD_MEMBERS_WITH_OWN_PATCH),
      ...cardPropertiesUndoPatch,
      updatedProperties: changedMembers(oldBoard.properties, newBoard.properties, []),
      deletedProperties: keysOnlyIn(newBoard.properties, oldBoard.properties),
    },
  ];
}

export function createPatchesFromContainersAndBlocks(
  updatedBoard: Container,
  oldBoard: Container,
  updatedBlockIds: string[],
  updatedBlocks: readonly Block[],
  oldBlocks: readonly Block[],
): [ContainersAndBlocksPatch, ContainersAndBlocksPatch] {
  const blockPatches: BlockPatch[] = [];
  const blockUndoPatches: BlockPatch[] = [];
  updatedBlocks.forEach((newBlock, index) => {
    const oldBlock = oldBlocks[index];
    if (!oldBlock) throw new Error(`no old state for block ${newBlock.id}`);
    const [patch, undoPatch] = createPatchesFromBlocks(newBlock, oldBlock);
    blockPatches.push(patch);
    blockUndoPatches.push(undoPatch);
  });
  const [boardPatch, boardUndoPatch] = createPatchesFromContainers(updatedBoard, oldBoard);
  return [
    {
      blockIDs: updatedBlockIds,
      blockPatches,
      containerIDs: [updatedBoard.id],
      containerPatches: [boardPatch],
    },
    {
      blockIDs: updatedBlockIds,
      blockPatches: blockUndoPatches,
      containerIDs: [updatedBoard.id],
      containerPatches: [boardUndoPatch],
    },
  ];
}
