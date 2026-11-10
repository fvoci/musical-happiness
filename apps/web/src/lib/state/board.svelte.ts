/**
 * One open board: its definition, views, cards and members, kept up to date
 * while it is open, and every change a screen can make to them.
 *
 * A change shows at once, is sent, and is taken back if the backend refuses
 * it. Each one can be undone (`workspace.undoLast`); changes made with one
 * gesture share a group and are undone together. What other people change
 * arrives through the backend's change messages and is applied the same way
 * as the answers to the reader's own writes.
 */
import { cardsOfView } from '../logic/card-list.ts';
import { getVisibleAndHiddenGroups, type BoardGroups, type NoValueLabel } from '../logic/group.ts';
import { propertyBehavior } from '../logic/properties.ts';
import { manualComparator } from '../logic/sort.ts';
import {
  createBoardView,
  viewGrouping,
  viewQuery,
  type BlockPatch,
  type Board,
  type BoardMember,
  type BoardPatch,
  type BoardView,
  type Card,
  type MemberRole,
  type PropertyTemplate,
  type PropertyValue,
} from '../logic/wire.ts';

import {
  isBackendError,
  isCardBlock,
  isViewBlock,
  type Attachment,
  type CardDraft,
  type CardPatch,
  type CardSummary,
  type Change,
  type Comment,
  type ViewDraft,
} from './backend.ts';
import { droppedCardOrder, placeAmongAll } from './card-order.ts';
import { CardStore } from './card.svelte.ts';
import { orderMembers, orderViews } from './order.ts';
import {
  applyBoardPatch,
  applyCardPatch,
  applyViewPatch,
  boardPatches,
  cardPatches,
  isEmptyBlockPatch,
  isEmptyCardPatch,
  viewPatches,
} from './patches.ts';
import type { LoadStatus, Workspace } from './workspace.svelte.ts';
import { PendingWrites, put, take } from './write.ts';

function roleOf(member: BoardMember | undefined): MemberRole {
  if (!member) return '';
  if (member.schemeAdmin) return 'admin';
  if (member.schemeEditor) return 'editor';
  if (member.schemeCommenter) return 'commenter';
  return member.schemeViewer ? 'viewer' : '';
}

let groupCounter = 0;
/** A name for the changes of one gesture, so that undo takes them back together. */
export function newUndoGroup(): string {
  groupCounter += 1;
  return `group-${groupCounter}`;
}

/** Where dropped cards go: a column (by the value of the grouping property) and a place in it. */
export interface CardDrop {
  viewId: string;
  cardIds: readonly string[];
  /**
   * The option of the column they were dropped in; `''` is the column of
   * cards without a value. Undefined: the view is not grouped, or the column
   * is not to change.
   */
  optionId?: string;
  /**
   * The place among the cards of that column, counted without the dropped
   * ones. Undefined: the view is sorted and places itself.
   */
  index?: number;
  /**
   * The cards of that column the page shows, in shown order, when a filter,
   * a search or a folded group may hide some of them: `index` is then a
   * place among these, and the store finds the place among all of the
   * column (`placeAmongAll`). Left out: `index` counts every card.
   */
  shown?: readonly string[];
}

export class BoardStore {
  readonly workspace: Workspace;
  readonly boardId: string;

  status = $state<LoadStatus>('idle');
  /** The board does not exist, or the reader may not see it: the two look the same. */
  missing = $state(false);
  board = $state.raw<Board | undefined>(undefined);
  views = $state.raw<BoardView[]>([]);
  /** Every card of the board, card templates included, in no order. */
  cards = $state.raw<CardSummary[]>([]);
  members = $state.raw<BoardMember[]>([]);
  /** The card that is open, if one is. */
  openCard = $state.raw<CardStore | undefined>(undefined);

  /** The views in the order of their tabs. */
  readonly orderedViews = $derived.by(() => orderViews(this.views, this.workspace.locale));
  /** The cards that are cards, not templates for new ones. */
  readonly realCards = $derived(this.cards.filter((card) => !card.fields.isTemplate));
  readonly orderedMembers = $derived.by(() =>
    orderMembers(
      this.members,
      (userId) => this.workspace.userName(userId) ?? '',
      this.workspace.locale,
    ),
  );
  /**
   * The people a person property of this board can be set to, and a filter
   * can ask for: its members, in their order. Whoever is named on a card
   * without being a member any more is still shown by name
   * (`workspace.users`), but is not offered.
   */
  readonly people = $derived(
    this.orderedMembers.flatMap((member) => this.workspace.user(member.userId) ?? []),
  );

  /** What the reader is on this board. */
  readonly role = $derived<MemberRole>(
    roleOf(this.members.find((member) => member.userId === this.workspace.me?.id)),
  );
  readonly canEdit = $derived(this.role === 'admin' || this.role === 'editor');
  readonly canComment = $derived(this.canEdit || this.role === 'commenter');

  readonly #cardWrites = new PendingWrites<CardSummary>();
  readonly #viewWrites = new PendingWrites<BoardView>();
  #stopWatching: (() => void) | undefined;

  constructor(workspace: Workspace, boardId: string) {
    this.workspace = workspace;
    this.boardId = boardId;
  }

  get #backend(): Workspace['backend'] {
    return this.workspace.backend;
  }

  async load(): Promise<void> {
    this.status = 'loading';
    this.missing = false;
    try {
      // Listen first: a change made while the lists are being read is not lost.
      this.#stopWatching?.();
      this.#stopWatching = this.#backend.watchBoard(this.boardId, (change) => {
        this.#apply(change);
      });
      const [board, views, cards, members] = await Promise.all([
        this.#backend.board(this.boardId),
        this.#backend.views(this.boardId),
        this.#backend.cards(this.boardId),
        this.#backend.members(this.boardId),
      ]);
      this.board = board;
      this.views = views;
      this.cards = cards;
      this.members = members;
      this.status = 'ready';
      void this.workspace.learnUsers(members.map((member) => member.userId));
    } catch (error) {
      this.status = 'failed';
      if (isBackendError(error) && error.status === 404) this.missing = true;
      else this.workspace.report('load', error);
    }
  }

  close(): void {
    this.#stopWatching?.();
    this.#stopWatching = undefined;
    this.openCard?.close();
    this.openCard = undefined;
  }

  view(viewId: string | undefined): BoardView | undefined {
    return this.views.find((view) => view.id === viewId);
  }

  card(cardId: string | undefined): CardSummary | undefined {
    return this.cards.find((card) => card.id === cardId);
  }

  /** The cards a view shows, in its order, for a search text. */
  cardsOf(view: BoardView, searchText = ''): Card[] {
    return cardsOfView(
      this.realCards,
      this.board?.cardProperties ?? [],
      viewQuery(view),
      searchText,
      {
        usernameOf: (userId) => this.workspace.userName(userId),
        locale: this.workspace.locale,
        timeZone: this.workspace.regional.timeZone,
      },
    );
  }

  /**
   * The property a view groups its cards by: the one it names. A board view
   * that names none, or one that cannot group, is grouped by the first
   * property that can, as the original drew such a view.
   */
  groupingOf(view: BoardView): PropertyTemplate | undefined {
    const properties = this.board?.cardProperties ?? [];
    const named = properties.find((template) => template.id === view.fields.groupById);
    if (view.fields.viewType !== 'board') return named;
    if (named && propertyBehavior(named.type).canGroup) return named;
    return properties.find((template) => propertyBehavior(template.type).canGroup);
  }

  /** The columns of a view, visible and hidden, for cards already filtered and sorted. */
  groupsOf(view: BoardView, cards: readonly Card[], noValueLabel: NoValueLabel): BoardGroups {
    const grouping = viewGrouping(view);
    const groupBy = this.groupingOf(view);
    return getVisibleAndHiddenGroups(
      cards,
      grouping.visibleOptionIds,
      grouping.hiddenOptionIds,
      groupBy,
      noValueLabel,
    );
  }

  // ------------------------------------------------------------ changes

  #apply(change: Change): void {
    switch (change.kind) {
      case 'board':
        if (!this.board || change.board.updateAt >= this.board.updateAt) this.board = change.board;
        break;
      case 'boardDeleted':
        this.missing = true;
        this.status = 'failed';
        break;
      case 'members':
        this.members = change.members;
        break;
      case 'blocks':
        for (const block of change.blocks) {
          if (isCardBlock(block)) this.#receiveCard(block);
          else if (isViewBlock(block)) this.#receiveView(block);
          else this.openCard?.receive(block);
        }
        break;
      case 'blocksDeleted':
        this.cards = this.cards.filter((card) => !change.blockIds.includes(card.id));
        this.views = this.views.filter((view) => !change.blockIds.includes(view.id));
        this.openCard?.forget(change.blockIds);
        break;
      default:
        break;
    }
  }

  #receiveCard(card: CardSummary): void {
    if (!this.#cardWrites.receive(card)) return;
    this.cards = take(this.cards, card);
    this.openCard?.cardChanged(card);
  }

  #receiveView(view: BoardView): void {
    if (this.#viewWrites.receive(view)) this.views = take(this.views, view);
  }

  /** A card came back from the backend, outside of the card list's own writes. */
  receiveCard(card: CardSummary): void {
    this.#receiveCard(card);
  }

  /** A comment or attachment of the open card changed. */
  receiveChild(block: Comment | Attachment): void {
    this.openCard?.receive(block);
  }

  async #perform<T>(
    redo: () => Promise<T>,
    undo: (value: T) => Promise<void>,
    description: string,
    groupId?: string,
  ): Promise<T | undefined> {
    try {
      return await this.workspace.undo.perform(redo, undo, description, groupId);
    } catch (error) {
      this.workspace.report('save', error);
      return undefined;
    }
  }

  // -------------------------------------------------------------- cards

  /** Shows `patch` on the card and sends it; on refusal shows `reverse` instead. */
  async #sendCardPatch(cardId: string, patch: CardPatch, reverse: CardPatch): Promise<void> {
    const current = this.card(cardId);
    if (!current) return;
    this.cards = put(this.cards, applyCardPatch(current, patch));
    this.#cardWrites.begin(cardId);
    try {
      const answer = await this.#backend.patchCard(this.boardId, cardId, patch);
      const latest = this.#cardWrites.end(cardId, answer);
      if (latest) {
        this.cards = take(this.cards, latest);
        this.openCard?.cardChanged(latest);
      }
    } catch (error) {
      const latest = this.#cardWrites.end(cardId);
      const shown = latest ?? this.card(cardId);
      if (shown) this.cards = put(this.cards, latest ?? applyCardPatch(shown, reverse));
      throw error;
    }
  }

  /**
   * Changes a card: `change` gets the card as it is and returns it as it
   * should be (a new object). Nothing is sent when the two say the same.
   */
  async changeCard(
    cardId: string,
    change: (card: CardSummary) => Card,
    description = 'changeCard',
    groupId?: string,
  ): Promise<void> {
    const current = this.card(cardId);
    if (!current) return;
    const [forward, backward] = cardPatches(change(current), current);
    if (isEmptyCardPatch(forward)) return;
    await this.#perform(
      () => this.#sendCardPatch(cardId, forward, backward),
      () => this.#sendCardPatch(cardId, backward, forward),
      description,
      groupId,
    );
  }

  /** Sets the value of a property on a card; `undefined` clears it. */
  async setCardProperty(
    cardId: string,
    propertyId: string,
    value: PropertyValue | undefined,
    groupId?: string,
  ): Promise<void> {
    await this.changeCard(
      cardId,
      (card) => {
        const properties = { ...card.fields.properties };
        const empty =
          value === undefined || value === '' || (Array.isArray(value) && !value.length);
        // eslint-disable-next-line @typescript-eslint/no-dynamic-delete -- the value of a property is removed by its id
        if (empty) delete properties[propertyId];
        else properties[propertyId] = value;
        return { ...card, fields: { ...card.fields, properties } };
      },
      'changeProperty',
      groupId,
    );
  }

  async setCardTitle(cardId: string, title: string): Promise<void> {
    await this.changeCard(cardId, (card) => ({ ...card, title }), 'changeTitle');
  }

  async setCardIcon(cardId: string, icon: string): Promise<void> {
    await this.changeCard(
      cardId,
      (card) => ({ ...card, fields: { ...card.fields, icon } }),
      'changeIcon',
    );
  }

  /** Makes a card. Undo deletes it; redo makes it again (with a new identifier). */
  async createCard(draft: CardDraft, groupId?: string): Promise<CardSummary | undefined> {
    return this.#perform(
      async () => {
        const card = await this.#backend.createCard(this.boardId, draft);
        this.cards = take(this.cards, card);
        return card;
      },
      async (card) => {
        await this.#backend.deleteBlock(this.boardId, card.id);
        this.cards = this.cards.filter((existing) => existing.id !== card.id);
      },
      'createCard',
      groupId,
    );
  }

  /** A copy of a card: title, icon, properties and body. Comments and attachments are not copied. */
  async duplicateCard(cardId: string, asTemplate?: boolean): Promise<CardSummary | undefined> {
    const source = this.card(cardId);
    if (!source) return undefined;
    try {
      const detail = await this.#backend.card(this.boardId, cardId);
      return await this.createCard({
        title: detail.title,
        icon: detail.fields.icon,
        properties: detail.fields.properties,
        isTemplate: asTemplate ?? detail.fields.isTemplate,
        body: detail.body,
      });
    } catch (error) {
      this.workspace.report('save', error);
      return undefined;
    }
  }

  /** Deletes cards. Undo brings them back with their comments and attachments. */
  async deleteCards(cardIds: readonly string[]): Promise<void> {
    const groupId = newUndoGroup();
    for (const cardId of cardIds) {
      await this.#perform(
        async () => {
          await this.#backend.deleteBlock(this.boardId, cardId);
          this.cards = this.cards.filter((card) => card.id !== cardId);
          if (this.openCard?.cardId === cardId) this.closeCard();
        },
        async () => {
          await this.#backend.restoreBlock(this.boardId, cardId);
          this.cards = take(this.cards, await this.#backend.card(this.boardId, cardId));
        },
        'deleteCard',
        groupId,
      );
    }
  }

  /**
   * Cards were dropped: into a column (their grouping property takes the
   * column's value) and, in a view without a sort, at a place (the view's
   * manual order changes). One undo takes all of it back.
   */
  async dropCards(drop: CardDrop): Promise<void> {
    const view = this.view(drop.viewId);
    if (!view) return;
    const groupId = newUndoGroup();
    const groupById = this.groupingOf(view)?.id;
    const optionId = drop.optionId;
    const valueOf = (card: Card): string => {
      const value = groupById ? card.fields.properties[groupById] : undefined;
      return typeof value === 'string' ? value : '';
    };

    if (drop.index !== undefined) {
      // The cards as they stand in the manual order, the dropped ones taken out.
      const shown = [...this.realCards].sort(manualComparator(view.fields.cardOrder));
      const moved = drop.cardIds;
      const column = shown
        .filter((card) => !moved.includes(card.id))
        .filter((card) => optionId === undefined || valueOf(card) === optionId)
        .map((card) => card.id);
      const cardOrder = droppedCardOrder(
        shown.map((card) => card.id),
        column,
        // The dropped cards keep the order they had.
        shown.filter((card) => moved.includes(card.id)).map((card) => card.id),
        drop.shown
          ? placeAmongAll(
              column,
              drop.shown.filter((id) => !moved.includes(id)),
              drop.index,
            )
          : drop.index,
      );
      await this.changeView(
        view.id,
        (current) => ({ ...current, fields: { ...current.fields, cardOrder } }),
        'moveCard',
        groupId,
      );
    }

    if (groupById && optionId !== undefined) {
      for (const cardId of drop.cardIds) {
        const card = this.card(cardId);
        if (!card || valueOf(card) === optionId) continue;
        await this.setCardProperty(
          cardId,
          groupById,
          optionId === '' ? undefined : optionId,
          groupId,
        );
      }
    }
  }

  // -------------------------------------------------------------- views

  async #sendViewPatch(viewId: string, patch: BlockPatch, reverse: BlockPatch): Promise<void> {
    const current = this.view(viewId);
    if (!current) return;
    this.views = put(this.views, applyViewPatch(current, patch));
    this.#viewWrites.begin(viewId);
    try {
      const answer = await this.#backend.patchView(this.boardId, viewId, patch);
      const latest = this.#viewWrites.end(viewId, answer);
      if (latest) this.views = take(this.views, latest);
    } catch (error) {
      const latest = this.#viewWrites.end(viewId);
      const shown = latest ?? this.view(viewId);
      if (shown) this.views = put(this.views, latest ?? applyViewPatch(shown, reverse));
      throw error;
    }
  }

  /** Changes a view: `change` gets the view as it is and returns it as it should be. */
  async changeView(
    viewId: string,
    change: (view: BoardView) => BoardView,
    description = 'changeView',
    groupId?: string,
  ): Promise<void> {
    const current = this.view(viewId);
    if (!current) return;
    const [forward, backward] = viewPatches(change(current), current);
    if (isEmptyBlockPatch(forward)) return;
    await this.#perform(
      () => this.#sendViewPatch(viewId, forward, backward),
      () => this.#sendViewPatch(viewId, backward, forward),
      description,
      groupId,
    );
  }

  /** Changes the members of a view's `fields`, such as its filter or its sort options. */
  async setViewFields(
    viewId: string,
    fields: Partial<BoardView['fields']>,
    description = 'changeView',
    groupId?: string,
  ): Promise<void> {
    await this.changeView(
      viewId,
      (view) => ({ ...view, fields: { ...view.fields, ...fields } }),
      description,
      groupId,
    );
  }

  async createView(draft: ViewDraft): Promise<BoardView | undefined> {
    return this.#perform(
      async () => {
        const view = await this.#backend.createView(this.boardId, draft);
        this.views = take(this.views, view);
        return view;
      },
      async (view) => {
        await this.#backend.deleteBlock(this.boardId, view.id);
        this.views = this.views.filter((existing) => existing.id !== view.id);
      },
      'createView',
    );
  }

  /** A copy of a view under another title. */
  async duplicateView(viewId: string, title: string): Promise<BoardView | undefined> {
    const source = this.view(viewId);
    if (!source) return undefined;
    return this.createView({ title, fields: createBoardView(source).fields });
  }

  async deleteView(viewId: string): Promise<void> {
    const gone = this.view(viewId);
    if (!gone) return;
    await this.#perform(
      async () => {
        await this.#backend.deleteBlock(this.boardId, viewId);
        this.views = this.views.filter((view) => view.id !== viewId);
      },
      async () => {
        await this.#backend.restoreBlock(this.boardId, viewId);
        this.views = take(this.views, gone);
      },
      'deleteView',
    );
  }

  // -------------------------------------------------------------- board

  async #sendBoardPatch(patch: BoardPatch, reverse: BoardPatch): Promise<void> {
    if (!this.board) return;
    this.board = applyBoardPatch(this.board, patch);
    try {
      const answer = await this.#backend.patchBoard(this.boardId, patch);
      if (answer.updateAt >= this.board.updateAt) this.board = answer;
    } catch (error) {
      this.board = applyBoardPatch(this.board, reverse);
      throw error;
    }
  }

  /**
   * Changes the board itself: its title, description, icon, or the
   * definitions of its card properties. `change` returns a new object.
   */
  async changeBoard(
    change: (board: Board) => Board,
    description = 'changeBoard',
    groupId?: string,
  ): Promise<void> {
    if (!this.board) return;
    const [forward, backward] = boardPatches(change(this.board), this.board);
    const empty = Object.keys(forward).every((key) => {
      const value = (forward as Record<string, unknown>)[key];
      if (Array.isArray(value)) return value.length === 0;
      if (typeof value === 'object' && value !== null) return Object.keys(value).length === 0;
      return value === undefined;
    });
    if (empty) return;
    await this.#perform(
      () => this.#sendBoardPatch(forward, backward),
      () => this.#sendBoardPatch(backward, forward),
      description,
      groupId,
    );
  }

  // --------------------------------------------------------- open card

  /** Opens a card: reads its body, comments and attachments. */
  async showCard(cardId: string): Promise<CardStore> {
    if (this.openCard?.cardId === cardId) return this.openCard;
    this.openCard?.close();
    const store = new CardStore(this, cardId);
    this.openCard = store;
    await store.load();
    return store;
  }

  closeCard(): void {
    this.openCard?.close();
    this.openCard = undefined;
  }
}
