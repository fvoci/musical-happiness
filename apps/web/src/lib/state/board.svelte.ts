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
  type BoardRole,
  type CardDraft,
  type CardPatch,
  type CardSummary,
  type Change,
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
import { restoreCard } from './trash.svelte.ts';
import { PendingWrites, put, reread, take } from './write.ts';

function roleOf(member: BoardMember | undefined): MemberRole {
  if (!member) return '';
  if (member.schemeAdmin) return 'admin';
  if (member.schemeEditor) return 'editor';
  if (member.schemeCommenter) return 'commenter';
  return member.schemeViewer ? 'viewer' : '';
}

/** The flags of a membership with exactly the one role. */
function roleFlags(
  role: BoardRole,
): Pick<BoardMember, 'schemeAdmin' | 'schemeEditor' | 'schemeCommenter' | 'schemeViewer'> {
  return {
    schemeAdmin: role === 'admin',
    schemeEditor: role === 'editor',
    schemeCommenter: role === 'commenter',
    schemeViewer: role === 'viewer',
  };
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
  /** The board is open and the reader no member of it: shown with a way to join (`join`). */
  joinable = $state(false);
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
  /** The real cards by identifier: a card a relation names is shown when it is here. */
  readonly cardsById: ReadonlyMap<string, CardSummary> = $derived(
    // eslint-disable-next-line svelte/prefer-svelte-reactivity -- made anew from the cards, never changed
    new Map(this.realCards.map((card) => [card.id, card])),
  );
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
  /** Whether the reader decides who is on the board and how it is shared. */
  readonly canShare = $derived(this.role === 'admin');
  readonly canComment = $derived(this.canEdit || this.role === 'commenter');

  /**
   * Whether the reader may delete a comment written by `createdBy`: an
   * editor their own, an admin anyone's. A commenter cannot delete even
   * their own (`deleteBlock` in `contracts/openapi.json`).
   */
  canDeleteComment(createdBy: string): boolean {
    if (this.role === 'admin') return true;
    return this.canEdit && createdBy === this.workspace.me?.id;
  }

  /**
   * Whether the reader may change the text of a comment written by
   * `createdBy`: their own, when they may edit the board. The server lets an
   * editor change anyone's comment and a commenter none (`patchBlock`); the
   * screen offers only one's own until the user's decision (B1 in
   * `docs/contract-assumptions.md`) is in the server.
   */
  canEditComment(createdBy: string): boolean {
    return this.canEdit && createdBy === this.workspace.me?.id;
  }

  readonly #cardWrites = new PendingWrites<CardSummary>();
  readonly #viewWrites = new PendingWrites<BoardView>();
  /** Readings again (`#refresh`) begun: the latest is the one that counts. */
  #refreshing = 0;
  /** Lists of members told by changes: a reading begun before the latest does not replace it. */
  #membersTold = 0;
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
    this.joinable = false;
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
      else if (isBackendError(error) && error.status === 403 && (await this.#openToJoin())) {
        this.joinable = true;
      } else this.workspace.report('load', error);
    }
  }

  /**
   * Whether the board refused is an open board the reader may join: one of
   * their team that they are no member of reads as itself, while its views,
   * cards and members are refused. Its title and description are then shown
   * with a way to join (the user's decision, H5).
   */
  async #openToJoin(): Promise<boolean> {
    try {
      const board = await this.#backend.board(this.boardId);
      if (board.type !== 'O' || board.isTemplate) return false;
      this.board = board;
      return true;
    } catch {
      return false;
    }
  }

  /** Joins the open board shown (`joinable`) and opens it. False (and told) when that failed. */
  async join(): Promise<boolean> {
    if (this.joining) return false;
    this.joining = true;
    try {
      await this.#backend.joinBoard(this.boardId);
    } catch (error) {
      // Made private or deleted meanwhile: there is nothing to join.
      if (isBackendError(error) && error.status === 404) {
        this.joinable = false;
        this.missing = true;
      } else this.workspace.report('save', error);
      return false;
    } finally {
      this.joining = false;
    }
    // Left meanwhile (the address changed): the board is not read for nobody.
    if (this.#closed) return false;
    // The sidebar hears of it from the server (a member message reads it again).
    await this.load();
    return this.status === 'ready';
  }

  /** The reader asked to join (`join`) and the answer has not come. */
  joining = $state(false);
  #closed = false;

  close(): void {
    this.#closed = true;
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
        cardTitle: (cardId) => this.cardsById.get(cardId)?.title,
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
        // Deleted and brought back: everything of it is read again.
        if (this.missing) {
          void this.load();
          break;
        }
        if (!this.board || change.board.updateAt >= this.board.updateAt) this.board = change.board;
        break;
      case 'boardDeleted':
        // An open board may only have been closed to the reader (taken off
        // it): read again, it shows as one to join, or as gone.
        if (this.board?.type === 'O' && !this.board.isTemplate && !this.#closed) {
          void this.load();
          break;
        }
        this.missing = true;
        this.status = 'failed';
        break;
      case 'members':
        this.#membersTold += 1;
        this.members = change.members;
        void this.workspace.learnUsers(change.members.map((member) => member.userId));
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
      case 'resync':
        void this.#refresh();
        break;
      default:
        break;
    }
  }

  /**
   * Reads the board again over what is shown, after changes may have been
   * missed (`resync`). Unlike `load`, the board stays on screen meanwhile;
   * what is read is merged with it (`reread`): a write under way, or a
   * change that arrived while reading, is not taken back. Of two readings
   * that overlap, the later one counts.
   */
  async #refresh(): Promise<void> {
    if (this.status !== 'ready') return;
    this.#refreshing += 1;
    const reading = this.#refreshing;
    const cardsBefore = this.cards.map((card) => card.id);
    const viewsBefore = this.views.map((view) => view.id);
    const membersTold = this.#membersTold;
    try {
      const [board, views, cards, members] = await Promise.all([
        this.#backend.board(this.boardId),
        this.#backend.views(this.boardId),
        this.#backend.cards(this.boardId),
        this.#backend.members(this.boardId),
      ]);
      // A newer reading has begun meanwhile: it is the one that counts.
      if (reading !== this.#refreshing) return;
      if (!this.board || board.updateAt >= this.board.updateAt) this.board = board;
      this.views = reread(this.views, views, viewsBefore, (view) => this.#viewWrites.receive(view));
      const shown = this.cards;
      this.cards = reread(shown, cards, cardsBefore, (card) => this.#cardWrites.receive(card));
      if (membersTold === this.#membersTold) {
        this.members = members;
        void this.workspace.learnUsers(members.map((member) => member.userId));
      }
      const open = this.openCard;
      if (open) {
        const card = this.card(open.cardId);
        if (card) open.cardChanged(card);
        else if (cardsBefore.includes(open.cardId)) open.forget([open.cardId]);
        void open.refresh();
      }
    } catch (error) {
      if (reading !== this.#refreshing) return;
      if (isBackendError(error) && error.status === 404) {
        this.missing = true;
        this.status = 'failed';
      } else {
        this.workspace.report('load', error);
      }
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
        await this.#deleteForUndo(card.id);
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

  /**
   * Deletes a block to take back its making: one that somebody else has
   * deleted meanwhile (404) is as good as deleted, so the undo goes on.
   */
  async #deleteForUndo(blockId: string): Promise<void> {
    try {
      await this.#backend.deleteBlock(this.boardId, blockId);
    } catch (error) {
      if (!isBackendError(error) || error.status !== 404) throw error;
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
        () => restoreCard(this, cardId),
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
        await this.#deleteForUndo(view.id);
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

  // ------------------------------------------------------------ sharing

  /** Whether anybody of the team may find and join the board. */
  async setOpen(open: boolean): Promise<void> {
    await this.changeBoard((board) => ({ ...board, type: open ? 'O' : 'P' }), 'changeBoardType');
  }

  /** The role somebody gets by joining the board. Members keep theirs. */
  async setMinimumRole(role: BoardRole): Promise<void> {
    await this.changeBoard((board) => ({ ...board, minimumRole: role }), 'changeMinimumRole');
  }

  /** Makes a person of the team a member. Somebody who is a member already keeps their role. */
  async addMember(userId: string, role: BoardRole): Promise<void> {
    try {
      const member = await this.#backend.addMember(this.boardId, userId, role);
      this.members = [...this.members.filter((one) => one.userId !== userId), member];
      void this.workspace.learnUsers([userId]);
    } catch (error) {
      this.workspace.report('save', error);
    }
  }

  async setMemberRole(userId: string, role: BoardRole): Promise<void> {
    await this.#memberWrite(
      this.members.map((member) =>
        member.userId === userId ? { ...member, ...roleFlags(role) } : member,
      ),
      () => this.#backend.setMemberRole(this.boardId, userId, role),
    );
  }

  async removeMember(userId: string): Promise<void> {
    await this.#memberWrite(
      this.members.filter((member) => member.userId !== userId),
      () => this.#backend.removeMember(this.boardId, userId),
    );
  }

  /**
   * Shows a change of the members at once. Refused, they go back and are read
   * again: what was put back may be older than what arrived meanwhile.
   */
  async #memberWrite(optimistic: BoardMember[], send: () => Promise<unknown>): Promise<void> {
    const before = this.members;
    this.members = optimistic;
    try {
      await send();
    } catch (error) {
      this.members = before;
      this.workspace.report('save', error);
      const told = this.#membersTold;
      try {
        const members = await this.#backend.members(this.boardId);
        // Members told meanwhile are newer than this reading.
        if (told === this.#membersTold) this.members = members;
      } catch {
        // The next message, or the next reading, brings them.
      }
    }
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
