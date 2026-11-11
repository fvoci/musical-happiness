/**
 * The shapes the server stores and sends, and everything that depends on them.
 *
 * The server's wire format is going to be redesigned. To keep that change in
 * one place, this is the only file of the logic that knows
 *
 * - what a block, a card, a view and a board look like,
 * - what is inside the untyped `fields` of a block,
 * - how a date value and the day of a date filter are written,
 * - how identifiers are made, and
 * - what a patch (the delta sent to the server) looks like.
 *
 * The other modules get at these through the functions exported here.
 * tests/logic/wire-boundary.test.ts fails when one of them reads `fields`
 * itself. What they do use directly are the members every block has at the
 * top level: `id`, `title`, `type`, `boardId`, `parentId`, `createdBy`,
 * `modifiedBy`, `createAt`, `updateAt`, `deleteAt` and `limited`.
 */
import { parseCalendarDate, type CalendarDate, type DatePoint, type DateValue } from './dates.ts';
import { diagnostics } from './diagnostics.ts';

// ------------------------------------------------------------ identifiers

/** z-base-32: no characters that are easily confused when read aloud. */
const ID_ALPHABET = 'ybndrfg8ejkmcpqxot1uwisza345h769';

export type IdKind =
  'none' | 'workspace' | 'board' | 'card' | 'view' | 'session' | 'user' | 'token' | 'block';

const ID_PREFIX: Record<IdKind, string> = {
  none: '7',
  workspace: 'w',
  board: 'b',
  card: 'c',
  view: 'v',
  session: 's',
  user: 'u',
  token: 'k',
  block: 'a',
};

/** Base32 with the alphabet above; `pad` fills the last group with `=`. */
export function encodeBase32(data: Uint8Array, pad = false): string {
  let bits = 0;
  let value = 0;
  let output = '';
  for (const byte of data) {
    value = (value << 8) | byte;
    bits += 8;
    while (bits >= 5) {
      output += ID_ALPHABET.charAt((value >>> (bits - 5)) & 31);
      bits -= 5;
    }
  }
  if (bits > 0) output += ID_ALPHABET.charAt((value << (5 - bits)) & 31);
  if (pad) while (output.length % 8 !== 0) output += '=';
  return output;
}

/** A new identifier: one letter for the kind and 26 characters of 128 random bits. */
export function createId(kind: IdKind): string {
  const bytes = new Uint8Array(16);
  crypto.getRandomValues(bytes);
  return ID_PREFIX[kind] + encodeBase32(bytes);
}

/** The identifier kind for a block of the given type. */
export function idKindOfBlockType(type: string | undefined): IdKind {
  switch (type) {
    case 'workspace':
    case 'board':
    case 'card':
    case 'view':
      return type;
    default:
      return 'none';
  }
}

// ----------------------------------------------------------------- blocks

export const CONTENT_BLOCK_TYPES = [
  'text',
  'image',
  'divider',
  'checkbox',
  'h1',
  'h2',
  'h3',
  'list-item',
  'attachment',
  'quote',
  'video',
] as const;

export const BLOCK_TYPES = [
  ...CONTENT_BLOCK_TYPES,
  'board',
  'view',
  'card',
  'comment',
  'unknown',
] as const;

export type ContentBlockType = (typeof CONTENT_BLOCK_TYPES)[number];
export type BlockType = (typeof BLOCK_TYPES)[number];

export interface Block {
  id: string;
  boardId: string;
  parentId: string;
  createdBy: string;
  modifiedBy: string;
  schema: number;
  type: BlockType;
  title: string;
  fields: Record<string, unknown>;
  createAt: number;
  updateAt: number;
  deleteAt: number;
  /** Set by the server on a card it withholds the content of. */
  limited?: boolean;
}

export interface BlockPatch {
  parentId?: string;
  schema?: number;
  type?: BlockType;
  title?: string;
  updatedFields?: Record<string, unknown>;
  deletedFields?: string[];
  deleteAt?: number;
}

/**
 * A block with every member present: a copy of `source` with the gaps
 * filled, or a new empty block. The copy shares the values inside `fields`.
 */
export function createBlock(source?: Block): Block {
  const now = Date.now();
  return {
    id: source?.id || createId(idKindOfBlockType(source?.type)),
    schema: 1,
    boardId: source?.boardId || '',
    parentId: source?.parentId || '',
    createdBy: source?.createdBy || '',
    modifiedBy: source?.modifiedBy || '',
    type: source?.type || 'unknown',
    fields: source?.fields ? { ...source.fields } : {},
    title: source?.title || '',
    createAt: source?.createAt || now,
    updateAt: source?.updateAt || now,
    deleteAt: source?.deleteAt || 0,
    limited: Boolean(source?.limited),
  };
}

// ------------------------------------------------------------------ cards

/**
 * A property value of a card: text, a list of option or user identifiers, or
 * (for a date property only) a date value.
 */
export type PropertyValue = string | string[] | StoredDateValue;

/** Content blocks in display order; an inner list is one row of blocks side by side. */
export type ContentOrder = (string | string[])[];

export type CardFields = {
  icon?: string;
  isTemplate?: boolean;
  properties: Record<string, PropertyValue>;
  contentOrder: ContentOrder;
};

export type Card = Block & { fields: CardFields };

function readContentOrder(value: unknown): ContentOrder {
  if (!Array.isArray(value)) return [];
  const order: ContentOrder = [];
  for (const entry of value as unknown[]) {
    if (typeof entry === 'string') order.push(entry);
    else if (Array.isArray(entry)) order.push([...(entry as string[])]);
  }
  return order;
}

export function createCard(source?: Block): Card {
  const fields = source?.fields ?? {};
  return {
    ...createBlock(source),
    type: 'card',
    fields: {
      icon: (fields.icon as string | undefined) || '',
      properties: { ...(fields.properties as Record<string, PropertyValue> | undefined) },
      contentOrder: readContentOrder(fields.contentOrder),
      isTemplate: Boolean(fields.isTemplate),
    },
  };
}

/** The value of a property as it is stored on the card, if any. */
export function cardProperty(card: Card, propertyId: string): PropertyValue | undefined {
  return card.fields.properties[propertyId];
}

/** Every stored property value of the card, by property identifier. */
export function cardProperties(card: Card): Readonly<Record<string, PropertyValue>> {
  return card.fields.properties;
}

/** Sets a property value on the card object itself. */
export function setCardProperty(card: Card, propertyId: string, value: PropertyValue): void {
  card.fields.properties[propertyId] = value;
}

export function cardContentOrder(card: Card): ContentOrder {
  return card.fields.contentOrder;
}

export function cardIcon(card: Card): string {
  return card.fields.icon ?? '';
}

/** Appends a content block to the end of the card's content. */
export function appendCardContent(card: Card, blockId: string): void {
  card.fields.contentOrder.push(blockId);
}

// ---------------------------------------------------------------- filters

export type FilterCondition =
  | 'includes'
  | 'notIncludes'
  | 'isEmpty'
  | 'isNotEmpty'
  | 'isSet'
  | 'isNotSet'
  | 'is'
  | 'contains'
  | 'notContains'
  | 'startsWith'
  | 'notStartsWith'
  | 'endsWith'
  | 'notEndsWith'
  | 'isBefore'
  | 'isAfter';

export type FilterClause = {
  propertyId: string;
  condition: FilterCondition;
  values: string[];
};

export type FilterGroupOperation = 'and' | 'or';

/** Either all of its filters have to hold (`and`) or one of them (`or`). */
export type FilterGroup = {
  operation: FilterGroupOperation;
  filters: (FilterClause | FilterGroup)[];
};

export function isFilterGroup(filter: FilterClause | FilterGroup): filter is FilterGroup {
  return 'operation' in filter && 'filters' in filter;
}

/** A clause with every member present; a stored clause can lack some. */
export function createFilterClause(source?: Partial<FilterClause>): FilterClause {
  return {
    propertyId: source?.propertyId || '',
    condition: source?.condition || 'includes',
    values: Array.isArray(source?.values) ? source.values.slice() : [],
  };
}

export function createFilterGroup(source?: FilterGroup): FilterGroup {
  return {
    operation: source?.operation || 'and',
    filters: (source?.filters ?? []).map((filter) =>
      isFilterGroup(filter) ? createFilterGroup(filter) : createFilterClause(filter),
    ),
  };
}

// ------------------------------------------------------------------ views

export type ViewType = 'board' | 'table' | 'gallery' | 'calendar';

/** How far in a Gantt view is zoomed: what one mark of its axis is. */
export type GanttZoom = 'day' | 'week' | 'month';

/** The pseudo property that stands for the title column in sort options and calculations. */
export const TITLE_COLUMN_ID = '__title';

export type SortOption = { propertyId: string; reversed: boolean };

export type KanbanCalculation = { calculation: string; propertyId: string };

export type BoardViewFields = {
  viewType: ViewType;
  groupById?: string;
  dateDisplayPropertyId?: string;
  /**
   * A Gantt view: the card relation property whose value on a card names the
   * cards that come before it. Missing or empty: no card comes after another.
   */
  dependencyPropertyId?: string;
  /** A Gantt view: its zoom. Missing, or a word this version does not know: `week`. */
  ganttZoom?: GanttZoom;
  sortOptions: SortOption[];
  visiblePropertyIds: string[];
  visibleOptionIds: string[];
  hiddenOptionIds: string[];
  collapsedOptionIds: string[];
  filter: FilterGroup;
  cardOrder: string[];
  columnWidths: Record<string, number>;
  columnCalculations: Record<string, string>;
  kanbanCalculations: Record<string, KanbanCalculation>;
  defaultTemplateId: string;
};

export type BoardView = Block & { fields: BoardViewFields };

const copyList = (value: unknown): string[] =>
  Array.isArray(value) ? [...(value as string[])] : [];

export function createBoardView(source?: Block): BoardView {
  const fields = (source?.fields ?? {}) as Partial<BoardViewFields>;
  return {
    ...createBlock(source),
    type: 'view',
    fields: {
      viewType: fields.viewType || 'board',
      groupById: fields.groupById,
      dateDisplayPropertyId: fields.dateDisplayPropertyId,
      dependencyPropertyId: fields.dependencyPropertyId,
      ganttZoom: fields.ganttZoom,
      sortOptions: (fields.sortOptions ?? []).map((option) => ({ ...option })),
      visiblePropertyIds: copyList(fields.visiblePropertyIds),
      visibleOptionIds: copyList(fields.visibleOptionIds),
      hiddenOptionIds: copyList(fields.hiddenOptionIds),
      collapsedOptionIds: copyList(fields.collapsedOptionIds),
      filter: createFilterGroup(fields.filter),
      cardOrder: copyList(fields.cardOrder),
      columnWidths: { ...fields.columnWidths },
      columnCalculations: { ...fields.columnCalculations },
      kanbanCalculations: { ...fields.kanbanCalculations },
      defaultTemplateId: fields.defaultTemplateId || '',
    },
  };
}

/** What a view says about which cards to show and in what order. */
export interface ViewQuery {
  viewType: ViewType;
  filter: FilterGroup;
  sortOptions: readonly SortOption[];
  /** The order the cards were dragged into; used when there is no sort option. */
  cardOrder: readonly string[];
  visiblePropertyIds: readonly string[];
  dateDisplayPropertyId: string | undefined;
}

export function viewQuery(view: BoardView): ViewQuery {
  const { fields } = view;
  return {
    viewType: fields.viewType,
    filter: fields.filter,
    sortOptions: fields.sortOptions,
    cardOrder: fields.cardOrder,
    visiblePropertyIds: fields.visiblePropertyIds,
    dateDisplayPropertyId: fields.dateDisplayPropertyId,
  };
}

/** What a view says about its columns: which property groups them and which groups show. */
export interface ViewGrouping {
  groupById: string | undefined;
  visibleOptionIds: readonly string[];
  hiddenOptionIds: readonly string[];
  collapsedOptionIds: readonly string[];
}

export function viewGrouping(view: BoardView): ViewGrouping {
  const { fields } = view;
  return {
    groupById: fields.groupById,
    visibleOptionIds: fields.visibleOptionIds,
    hiddenOptionIds: fields.hiddenOptionIds,
    collapsedOptionIds: fields.collapsedOptionIds,
  };
}

// ----------------------------------------------------- other block kinds

export type CommentBlock = Block & { type: 'comment' };
export type TextBlock = Block & { type: 'text' };
export type HeadingBlock = Block & { type: 'h1' | 'h2' | 'h3' };
export type DividerBlock = Block & { type: 'divider' };
export type CheckboxBlock = Block & { type: 'checkbox' };
export type ImageBlock = Block & { type: 'image'; fields: { fileId: string } };
export type AttachmentBlock = Block & { type: 'attachment'; fields: { fileId: string } };

function createTyped<T extends BlockType>(type: T, source?: Block): Block & { type: T } {
  return { ...createBlock(source), type };
}

export const createCommentBlock = (source?: Block): CommentBlock => createTyped('comment', source);
export const createTextBlock = (source?: Block): TextBlock => createTyped('text', source);
export const createDividerBlock = (source?: Block): DividerBlock => createTyped('divider', source);
export const createCheckboxBlock = (source?: Block): CheckboxBlock =>
  createTyped('checkbox', source);

export function createHeadingBlock(level: 1 | 2 | 3, source?: Block): HeadingBlock {
  return createTyped(`h${level}`, source);
}

export function createImageBlock(source?: Block): ImageBlock {
  return {
    ...createTyped('image', source),
    fields: { fileId: (source?.fields.fileId as string | undefined) || '' },
  };
}

export function createAttachmentBlock(source?: Block): AttachmentBlock {
  const fields = source?.fields ?? {};
  return {
    ...createTyped('attachment', source),
    // Archives made by old versions name the file `attachmentId`.
    fields: {
      fileId:
        (fields.attachmentId as string | undefined) || (fields.fileId as string | undefined) || '',
    },
  };
}

/** The identifier of the file an image or attachment block shows. */
export function blockFileId(block: ImageBlock | AttachmentBlock): string {
  return block.fields.fileId;
}

/**
 * The block as the kind its `type` says, with the members of that kind
 * filled in. A block of a type this app does not know comes back as a plain
 * block, and is reported.
 */
export function normalizeBlock(block: Block): Block {
  switch (block.type) {
    case 'view':
      return createBoardView(block);
    case 'card':
      return createCard(block);
    case 'text':
      return createTextBlock(block);
    case 'h1':
      return createHeadingBlock(1, block);
    case 'h2':
      return createHeadingBlock(2, block);
    case 'h3':
      return createHeadingBlock(3, block);
    case 'image':
      return createImageBlock(block);
    case 'divider':
      return createDividerBlock(block);
    case 'comment':
      return createCommentBlock(block);
    case 'checkbox':
      return createCheckboxBlock(block);
    case 'attachment':
      return createAttachmentBlock(block);
    default:
      diagnostics.report(`cannot normalize a block of unknown type: ${block.type}`);
      return createBlock(block);
  }
}

export function normalizeBlocks(blocks: readonly Block[]): Block[] {
  return blocks.map(normalizeBlock);
}

/**
 * Rewrites the identifiers a block keeps of other blocks: the manual card
 * order of a view and the content order of a card. Used after copying a tree
 * of blocks under new identifiers.
 */
export function remapBlockReferences(block: Block, idMap: Readonly<Record<string, string>>): void {
  // An identifier outside the copied tree stays as it is.
  const mapped = (id: string): string => idMap[id] ?? id;
  if (block.type === 'view') {
    const view = block as BoardView;
    view.fields.cardOrder = view.fields.cardOrder.map(mapped);
  }
  if (block.type === 'card') {
    const card = block as Card;
    card.fields.contentOrder = card.fields.contentOrder.map((entry) =>
      Array.isArray(entry) ? entry.map(mapped) : mapped(entry),
    );
  }
}

// ----------------------------------------------------------------- boards

export const BOARD_TYPE_OPEN = 'O';
export const BOARD_TYPE_PRIVATE = 'P';
export type BoardType = typeof BOARD_TYPE_OPEN | typeof BOARD_TYPE_PRIVATE;

export type MemberRole = 'viewer' | 'commenter' | 'editor' | 'admin' | '';

export type PropertyType =
  | 'text'
  | 'number'
  | 'select'
  | 'multiSelect'
  | 'date'
  | 'person'
  | 'multiPerson'
  | 'file'
  | 'checkbox'
  | 'url'
  | 'email'
  | 'phone'
  | 'createdTime'
  | 'createdBy'
  | 'updatedTime'
  | 'updatedBy'
  | 'relation'
  | 'unknown';

export interface PropertyOption {
  id: string;
  value: string;
  color: string;
}

/** The definition of one card property of a board. */
export interface PropertyTemplate {
  id: string;
  name: string;
  type: PropertyType;
  options: PropertyOption[];
}

export interface Board {
  id: string;
  teamId: string;
  channelId?: string;
  createdBy: string;
  modifiedBy: string;
  type: BoardType;
  minimumRole: MemberRole;
  title: string;
  description: string;
  icon?: string;
  showDescription: boolean;
  isTemplate: boolean;
  templateVersion: number;
  properties: Record<string, string | string[]>;
  cardProperties: PropertyTemplate[];
  createAt: number;
  updateAt: number;
  deleteAt: number;
}

export interface BoardPatch {
  type?: BoardType;
  minimumRole?: MemberRole;
  title?: string;
  description?: string;
  icon?: string;
  showDescription?: boolean;
  updatedProperties?: Record<string, unknown>;
  deletedProperties?: string[];
  updatedCardProperties?: PropertyTemplate[];
  deletedCardProperties?: string[];
}

export interface BoardMember {
  boardId: string;
  userId: string;
  roles?: string;
  minimumRole: MemberRole;
  schemeAdmin: boolean;
  schemeEditor: boolean;
  schemeCommenter: boolean;
  schemeViewer: boolean;
  synthetic: boolean;
}

export interface BoardsAndBlocks {
  boards: Board[];
  blocks: Block[];
}

export interface BoardsAndBlocksPatch {
  boardIDs: string[];
  boardPatches: BoardPatch[];
  blockIDs: string[];
  blockPatches: BlockPatch[];
}

/**
 * A board with every member present: a copy of `source` with the gaps filled
 * (its card properties and their options copied too), or a new board.
 *
 * A new board starts with one select property. `statusName` is its name; the
 * caller passes the translated word.
 */
export function createBoard(source?: Board, statusName = 'Status'): Board {
  const now = Date.now();
  const cardProperties: PropertyTemplate[] = source?.cardProperties
    ? source.cardProperties.map((template) => ({
        id: template.id,
        name: template.name,
        type: template.type,
        options: (template.options as PropertyOption[] | undefined)?.map((o) => ({ ...o })) ?? [],
      }))
    : [{ id: createId('block'), name: statusName, type: 'select', options: [] }];

  return {
    id: source?.id || createId('board'),
    teamId: source?.teamId || '',
    channelId: source?.channelId || '',
    createdBy: source?.createdBy || '',
    modifiedBy: source?.modifiedBy || '',
    type: source?.type || BOARD_TYPE_PRIVATE,
    minimumRole: source?.minimumRole || '',
    title: source?.title || '',
    description: source?.description || '',
    icon: source?.icon || '',
    showDescription: source?.showDescription || false,
    isTemplate: source?.isTemplate || false,
    templateVersion: source?.templateVersion || 0,
    properties: source?.properties ?? {},
    cardProperties,
    createAt: source?.createAt || now,
    updateAt: source?.updateAt || now,
    deleteAt: source?.deleteAt || 0,
  };
}

// ------------------------------------------------------------ date values

/**
 * How a date property is written on the wire: an object, not text. Each end
 * says by its JSON type what it is. A calendar day is the text `YYYY-MM-DD`,
 * the same day for every reader; a moment is an integer, the milliseconds
 * since 1970-01-01 00:00 UTC, like `createAt` and `updateAt` of a block.
 *
 * `from` is always there; `to`, when there, is of the same kind as `from`,
 * later than it, and itself part of the range. Nothing else is in the object:
 * whether a value carries a time is its type, and the time zone belongs to
 * the reader, not to the value.
 */
export interface StoredDateValue {
  from: string | number;
  to?: string | number;
}

/** The last moment the wire can carry: the end of the year 9999. */
const LAST_INSTANT = Date.UTC(9999, 11, 31, 23, 59, 59, 999);

function readPoint(stored: unknown): DatePoint | undefined {
  if (typeof stored === 'string') {
    const date = parseCalendarDate(stored);
    return date === undefined ? undefined : { date };
  }
  if (typeof stored !== 'number' || !Number.isInteger(stored)) return undefined;
  return stored >= 0 && stored <= LAST_INSTANT ? { instant: stored } : undefined;
}

const writePoint = (point: DatePoint): string | number => point.instant ?? point.date;

const samePoint = (first: DatePoint, second: DatePoint): boolean =>
  first.date === second.date && first.instant === second.instant;

/**
 * Reads the stored value of a date property. Whatever is not a well-formed
 * date value (text left behind by another property type, ends of two kinds,
 * a range that runs backwards, a day that does not exist) reads as an empty
 * value: the server refuses to store such a thing, so it is not guessed at.
 */
export function parseDateValue(stored: unknown): DateValue {
  if (typeof stored !== 'object' || stored === null || Array.isArray(stored)) return {};
  const { from: storedFrom, to: storedTo } = stored as { from?: unknown; to?: unknown };
  const from = readPoint(storedFrom);
  if (!from) return {};
  if (storedTo === undefined) return { from };
  const to = readPoint(storedTo);
  if (!to) return {};
  if (from.date !== undefined && to.date !== undefined) {
    if (to.date < from.date) return {};
  } else if (from.instant !== undefined && to.instant !== undefined) {
    if (to.instant < from.instant) return {};
  } else {
    return {};
  }
  return samePoint(from, to) ? { from } : { from, to };
}

/**
 * The value to store for a date value; undefined when the value is empty
 * (the property is then cleared). A range whose two ends are the same is
 * written as that one end, and one written backwards is turned around.
 */
export function formatDateValue(value: DateValue): StoredDateValue | undefined {
  const { from, to } = value;
  if (!from) return undefined;
  if (!to || samePoint(from, to)) return { from: writePoint(from) };
  const first = writePoint(from);
  const last = writePoint(to);
  if (typeof first !== typeof last) return { from: first };
  return first <= last ? { from: first, to: last } : { from: last, to: first };
}

/**
 * A stored value as text or a list of texts, which is what every kind of
 * property but the date kind holds. A date value has no text of its own: it
 * becomes a key, its ends joined by a slash, that is the same for the same
 * value. The key is for counting and comparing; what is shown for a date is
 * made from `parseDateValue`. A malformed date value has no text at all.
 */
export function plainPropertyValue(
  value: PropertyValue | undefined,
): string | string[] | undefined {
  if (value === undefined || typeof value === 'string' || Array.isArray(value)) return value;
  const stored = formatDateValue(parseDateValue(value));
  if (!stored) return undefined;
  return stored.to === undefined ? String(stored.from) : `${stored.from}/${stored.to}`;
}

/** The most cards one relation value names. */
export const RELATION_LIMIT = 50;

/**
 * The cards a relation property names: a list of card identifiers, each
 * once, in the order they were picked. Whatever else is stored (text left by
 * another kind, a date) names no card. Identifiers of deleted cards are kept
 * here; what is shown leaves them out. Written as such a list too.
 */
export function parseRelationValue(stored: PropertyValue | undefined): string[] {
  if (!Array.isArray(stored)) return [];
  return [...new Set(stored.filter((id) => typeof id === 'string' && id !== ''))];
}

/** The day a date filter compares with: a filter stores it as `YYYY-MM-DD`. */
export function parseFilterDate(text: string | undefined): CalendarDate | undefined {
  return text === undefined ? undefined : parseCalendarDate(text);
}

/** The text to store in a date filter for a day. */
export function formatFilterDate(date: CalendarDate): string {
  return date;
}

// ---------------------------------------------------------------- patches

/** A stored object can be missing altogether; it then counts as empty. */
type Members = object | null | undefined;

function keysOnlyIn(first: Members, second: Members): string[] {
  const other = new Set(Object.keys(second ?? {}));
  return Object.keys(first ?? {}).filter((key) => !other.has(key));
}

/** The members of `from` whose value differs from the one in `to`, except `skip`. */
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

/**
 * The patch that turns `oldBlock` into `newBlock`, and the one that undoes
 * it. Values are compared by identity, so a field that was replaced by an
 * equal copy counts as changed.
 */
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

/** The templates of `templates` that are new or different compared with `others`. */
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

/** The patch that turns the old card properties into the new ones, and the one that undoes it. */
export function createCardPropertiesPatches(
  newCardProperties: readonly PropertyTemplate[],
  oldCardProperties: readonly PropertyTemplate[],
): [BoardPatch, BoardPatch] {
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

/** The patch that turns `oldBoard` into `newBoard`, and the one that undoes it. */
export function createPatchesFromBoards(
  newBoard: Board,
  oldBoard: Board,
): [BoardPatch, BoardPatch] {
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

/**
 * One patch for a board together with some of its blocks, and the one that
 * undoes it. `updatedBlocks[i]` and `oldBlocks[i]` are the two states of the
 * block `updatedBlockIds[i]`.
 */
export function createPatchesFromBoardsAndBlocks(
  updatedBoard: Board,
  oldBoard: Board,
  updatedBlockIds: string[],
  updatedBlocks: readonly Block[],
  oldBlocks: readonly Block[],
): [BoardsAndBlocksPatch, BoardsAndBlocksPatch] {
  const blockPatches: BlockPatch[] = [];
  const blockUndoPatches: BlockPatch[] = [];
  updatedBlocks.forEach((newBlock, index) => {
    const oldBlock = oldBlocks[index];
    if (!oldBlock) throw new Error(`no old state for block ${newBlock.id}`);
    const [patch, undoPatch] = createPatchesFromBlocks(newBlock, oldBlock);
    blockPatches.push(patch);
    blockUndoPatches.push(undoPatch);
  });
  const [boardPatch, boardUndoPatch] = createPatchesFromBoards(updatedBoard, oldBoard);
  return [
    {
      blockIDs: updatedBlockIds,
      blockPatches,
      boardIDs: [updatedBoard.id],
      boardPatches: [boardPatch],
    },
    {
      blockIDs: updatedBlockIds,
      blockPatches: blockUndoPatches,
      boardIDs: [updatedBoard.id],
      boardPatches: [boardUndoPatch],
    },
  ];
}
