/**
 * One open board: its definition, views, cards and members, kept up to date
 * while it is open, and every change a screen can make to them.
 *
 * A change shows at once, is sent, and is taken back if the backend refuses
 * it. Each one can be undone (`workspace.undoLast`); the changes of one
 * gesture (`inOneStep`) are undone together. What other people change
 * arrives through the backend's change messages and is applied the same way
 * as the answers to the reader's own writes.
 */
import type { BoardGroups, NoValueLabel } from '../logic/group.ts';
import type {
  BlockPatch,
  Container,
  ContainerMember,
  BoardView,
  Card,
  MemberRole,
  PropertyTemplate,
  PropertyValue,
} from '../logic/wire.ts';

import {
  isBackendError,
  isCardBlock,
  isViewBlock,
  type ContainerRole,
  type CardDraft,
  type CardSummary,
  type Change,
  type ViewDraft,
} from './backend.ts';
import { copyOfCard } from './card-copy.ts';
import { BoardWrites, cardWritesOf, viewWritesOf } from './board-writes.ts';
import { dropCardsOn } from './card-drop.ts';
import { CardStore } from './card.svelte.ts';
import { lastCommentsOf } from './last-comments.ts';
import { propertyMovesOf } from './property-moves.ts';
import { authorIds, memberId } from './user-names.ts';
import {
  handleBoardLoadFailure,
  handleBoardRefreshFailure,
  joinOpenBoard,
  readBoard,
} from './board-load.ts';
import {
  canChangeComment,
  cardsOfBoardView,
  groupingPropertyOf,
  groupsOfView,
  roleOf,
  withCardProperty,
  withVisibility,
  type CardDrop,
} from './board-parts.ts';
import { MemberWrites } from './member-writes.ts';
import { orderMembers, orderViews } from './order.ts';
import {
  containerPatches,
  cardPatches,
  isEmptyBlockPatch,
  isEmptyContainerPatch,
  isEmptyCardPatch,
  viewPatches,
} from './patches.ts';
import type { LoadStatus, Workspace } from './workspace.svelte.ts';
import { restoreCard } from './trash.svelte.ts';
import { reread, take } from './write.ts';

export type { CardDrop } from './board-parts.ts';

export class BoardStore {
  readonly workspace: Workspace;
  readonly boardId: string;
  /** The board as the dialogs of a board's or a wiki's people name it (`Shareable`). */
  get containerId(): string {
    return this.boardId;
  }
  get container(): Container | undefined {
    return this.board;
  }
  set container(board: Container | undefined) {
    this.board = board;
  }

  status = $state<LoadStatus>('idle');
  /** The board does not exist, or the reader may not see it: the two look the same. */
  missing = $state(false);
  /** The board is open and the reader no member of it: shown with a way to join (`join`). */
  joinable = $state(false);
  board = $state.raw<Container | undefined>(undefined);
  views = $state.raw<BoardView[]>([]);
  /** Every card of the board, card templates included, in no order. */
  cards = $state.raw<CardSummary[]>([]);
  members = $state.raw<ContainerMember[]>([]);
  /** The card that is open, if one is. */
  openCard = $state.raw<CardStore | undefined>(undefined);

  /** The views in the order of their list (`orderViews`). */
  readonly orderedViews = $derived.by(() => orderViews(this.views, this.workspace.locale));
  /** The cards that are cards, not templates for new ones. */
  readonly realCards = $derived(this.cards.filter((card) => !card.fields.isTemplate));
  /** The real cards by identifier: a card a relation names is shown when it is here. */
  readonly cardsById: ReadonlyMap<string, CardSummary> = $derived(
    // eslint-disable-next-line svelte/prefer-svelte-reactivity -- made anew from the cards, never changed
    new Map(this.realCards.map((card) => [card.id, card])),
  );
  readonly orderedMembers = $derived.by(() => {
    const nameOf = (id: string) => this.workspace.userName(id) ?? '';
    return orderMembers(this.members, nameOf, this.workspace.locale);
  });
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

  /** Whether the reader may delete or edit a comment written by `createdBy` (`canChangeComment`). */
  canDeleteComment(createdBy: string): boolean {
    return canChangeComment(this.role, createdBy, this.workspace.me?.id);
  }

  canEditComment(createdBy: string): boolean {
    return canChangeComment(this.role, createdBy, this.workspace.me?.id);
  }

  /**
   * Whether the reader may read what comments said before they were changed:
   * a member, whatever the role (`getCommentVersions`); not a reader by the
   * board's link.
   */
  readonly canReadCommentHistory = $derived(this.role !== '');

  /** Keeps the cards' `lastCommentAt` current between readings. */
  readonly #lastComments = lastCommentsOf(this);
  readonly #boardWrites = new BoardWrites(this);
  readonly #cardWrites = cardWritesOf(this);
  readonly #viewWrites = viewWritesOf(this);
  /** Readings again (`#refresh`) begun: the latest is the one that counts. */
  #refreshing = 0;
  /** Adding, changing and taking off members (`member-writes.ts`). */
  readonly #people = new MemberWrites(this);
  #stopWatching: (() => void) | undefined;

  constructor(workspace: Workspace, boardId: string) {
    this.workspace = workspace;
    this.boardId = boardId;
  }

  get #backend(): Workspace['backend'] {
    return this.workspace.backend;
  }

  /** Increments on every load or cache resume so `whileHere` detects board round-trips. */
  visit = 0;
  /** Set when cache invalidation detaches this store while active so leaving closes it instead of re-caching. */
  evicted = false;
  #evictionGen = 0;

  async load(): Promise<void> {
    this.visit += 1;
    [this.#parked, this.revalidating, this.evicted] = [false, false, false];
    [this.status, this.missing, this.joinable] = ['loading', false, false];
    try {
      // Listen first: a change made while the lists are being read is not lost.
      this.#listen();
      const [board, views, cards, members] = await readBoard(this.workspace, this.boardId);
      if (this.#isInactive()) return;
      [this.board, this.views, this.cards, this.members] = [board, views, cards, members];
      this.status = 'ready';
    } catch (error) {
      if (this.isClosed()) return;
      await handleBoardLoadFailure(this, error);
    }
  }

  markEvicted(): void {
    this.#evictionGen += 1;
    this.evicted = true;
  }

  #listen(): void {
    this.#stopWatching?.();
    this.#stopWatching = this.#backend.watchContainer(this.boardId, this.#apply.bind(this));
  }

  isClosed(): boolean {
    return this.#closed;
  }

  #isInactive(): boolean {
    return this.#closed || this.#parked;
  }

  /** Detaches live listeners, open card rooms and pending re-readings. */
  #detach(): void {
    this.#refreshing += 1;
    this.#lastComments.cancel();
    this.#stopWatching?.();
    this.#stopWatching = undefined;
    this.closeCard();
  }

  /** Joins the open board shown (`joinable`) and opens it. False (and told) when that failed. */
  join(): Promise<boolean> {
    return joinOpenBoard(this);
  }

  /** The reader asked to join (`join`) and the answer has not come. */
  joining = $state(false);
  /** True while a cached board is revalidating in the background after `resume()`. */
  revalidating = $state(false);
  #closed = false;
  #parked = false;

  /** Detaches listeners, open card rooms and timers while keeping the board cached. */
  park(): void {
    [this.#parked, this.revalidating] = [true, false];
    this.#detach();
  }

  /** Re-attaches live listening and refreshes cached data in the background. */
  async resume(): Promise<void> {
    if (this.#closed) return;
    [this.#parked, this.revalidating] = [false, true];
    this.visit += 1;
    const visit = this.visit;
    this.#listen();
    try {
      await this.#refresh();
    } finally {
      if (this.visit === visit) this.revalidating = false;
    }
  }

  close(): void {
    [this.#closed, this.#parked, this.revalidating] = [true, false, false];
    this.#detach();
  }

  view(viewId: string | undefined): BoardView | undefined {
    return this.views.find((view) => view.id === viewId);
  }

  card(cardId: string | undefined): CardSummary | undefined {
    return this.cards.find((card) => card.id === cardId);
  }

  /** The cards a view shows, in its order, for a search text. */
  cardsOf(view: BoardView, searchText = ''): Card[] {
    return cardsOfBoardView(this, view, searchText);
  }

  /** The property a view groups its cards by (`groupingPropertyOf`). */
  groupingOf(view: BoardView): PropertyTemplate | undefined {
    return groupingPropertyOf(this.board?.cardProperties ?? [], view);
  }

  /** The columns of a view, visible and hidden, for cards already filtered and sorted. */
  groupsOf(view: BoardView, cards: readonly Card[], noValueLabel: NoValueLabel): BoardGroups {
    return groupsOfView(this.board?.cardProperties ?? [], view, cards, noValueLabel);
  }

  // ------------------------------------------------------------ changes

  #apply(change: Change): void {
    switch (change.kind) {
      case 'container':
        // Deleted and brought back: everything of it is read again.
        if (this.missing) {
          void this.load();
          break;
        }
        if (this.#boardWrites.receive(change.container)) this.#takeBoard(change.container);
        break;
      case 'containerDeleted':
        this.workspace.evictContainer(this.boardId);
        // An open board may only have been closed to the reader (taken off
        // it): read again, it shows as one to join, or as gone.
        if (this.board?.visibility === 'open' && !this.board.isTemplate && !this.#closed) {
          void this.load();
          break;
        }
        this.missing = true;
        this.status = 'failed';
        break;
      case 'members':
        this.#people.told += 1;
        this.members = change.members;
        if (
          this.workspace.me &&
          !change.members.some((member) => member.userId === this.workspace.me?.id)
        ) {
          this.workspace.evictContainer(this.boardId);
        }
        void this.workspace.learnUsers(change.members.map(memberId), { members: true });
        break;
      case 'blocks':
        for (const block of change.blocks) {
          if (isCardBlock(block)) this.#receiveCard(block);
          else if (isViewBlock(block)) this.#receiveView(block);
          else {
            if (block.type === 'comment' && block.deletedAt === undefined) {
              this.#lastComments.heard(block);
            }
            this.openCard?.receive(block);
          }
        }
        break;
      case 'blocksDeleted':
        this.#lastComments.deleted(change.blockIds);
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
   * missed (`resync`, a card's room gone). Unlike `load`, the board stays;
   * what is read is merged with it (`reread`): a write under way, or a
   * change that arrived while reading, is not taken back. Of two readings
   * that overlap, the later one counts.
   */
  async #refresh(): Promise<void> {
    if (this.status !== 'ready' || this.#closed || this.#parked) return;
    this.#refreshing += 1;
    const heard = this.#lastComments.begin();
    const reading = this.#refreshing;
    const evictionGen = this.#evictionGen;
    const cardsBefore = this.cards.map((card) => card.id);
    const viewsBefore = this.views.map((view) => view.id);
    const membersTold = this.#people.told;
    try {
      const [board, views, cards, members] = await readBoard(this.workspace, this.boardId);
      // A newer reading has begun meanwhile: it is the one that counts.
      if (reading !== this.#refreshing || this.#isInactive()) return;
      if (this.#boardWrites.receive(board)) this.#takeBoard(board);
      this.views = reread(this.views, views, viewsBefore, (view) => this.#viewWrites.receive(view));
      const shown = this.cards;
      const read = cards.map((card) => this.#lastComments.settle(card, heard));
      this.cards = reread(shown, read, cardsBefore, (card) => this.#cardWrites.receive(card));
      if (membersTold === this.#people.told) {
        this.members = members;
        void this.workspace.learnUsers(members.map(memberId), { members: true });
      }
      if (this.#evictionGen === evictionGen) this.evicted = false;
      const open = this.openCard;
      if (open) {
        const card = this.card(open.cardId);
        if (card) open.cardChanged(card);
        else if (cardsBefore.includes(open.cardId)) open.forget([open.cardId]);
        void open.refresh();
      }
    } catch (error) {
      await handleBoardRefreshFailure(
        this,
        error,
        () => reading === this.#refreshing && !this.#isInactive(),
      );
    }
  }

  /** The reader is deleting a block of the board; a comment's card is `cardId`. */
  deleting(blockId: string, cardId?: string): void {
    this.#lastComments.deleting(blockId, cardId);
  }

  #receiveCard(incoming: CardSummary): void {
    const card = this.#lastComments.merge(incoming, this.card(incoming.id));
    if (!this.#cardWrites.receive(card)) return;
    void this.workspace.learnUsers(authorIds([card]));
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
    ready?: () => Promise<void> | undefined,
  ): Promise<T | undefined> {
    try {
      return await this.workspace.undo.perform(redo, undo, description, groupId, false, ready);
    } catch (error) {
      if (isBackendError(error) && (error.status === 403 || error.status === 404)) {
        this.workspace.evictContainer(this.boardId);
      }
      this.workspace.report('save', error);
      return undefined;
    }
  }

  /** Runs a gesture whose changes, made with the group it is given, are one step of undo. */
  inOneStep<T>(gesture: (groupId: string) => Promise<T>, placedAtOnce = false): Promise<T> {
    return this.workspace.undo.group(gesture, placedAtOnce);
  }

  // -------------------------------------------------------------- cards

  /**
   * Changes a card: `change` gets the card as it is and returns it as it
   * should be (a new object). Nothing is sent when the two say the same.
   * Undo and redo write what was said before, quietly: nobody the card is
   * given to again is told of it twice.
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
    let said = false;
    await this.#perform(
      async () => {
        await this.#cardWrites.write(cardId, forward, backward, said);
        said = true;
      },
      () => this.#cardWrites.write(cardId, backward, forward, true),
      description,
      groupId,
      this.#cardWrites.keep(cardId, backward),
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
      (card) => withCardProperty(card, propertyId, value),
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

  /**
   * Makes a card. Undo deletes it; redo makes it again (with a new
   * identifier), quietly: nobody it names or is given to is told twice, and
   * its writer watches it as before.
   */
  async createCard(draft: CardDraft, groupId?: string): Promise<CardSummary | undefined> {
    let made = false;
    return this.#perform(
      async () => {
        const card = await this.#backend.createCard(
          this.boardId,
          made ? { ...draft, quiet: true } : draft,
        );
        made = true;
        void this.workspace.learnUsers(authorIds([card]));
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
  duplicateCard(cardId: string, asTemplate?: boolean): Promise<CardSummary | undefined> {
    return copyOfCard(this, cardId, asTemplate);
  }

  /**
   * Deletes a block to take back its making: one that somebody else has
   * deleted meanwhile (404) is as good as deleted, so the undo goes on.
   */
  async #deleteForUndo(blockId: string): Promise<void> {
    this.deleting(blockId);
    try {
      await this.#backend.deleteBlock(this.boardId, blockId);
    } catch (error) {
      if (!isBackendError(error) || error.status !== 404) throw error;
    }
  }

  /** Deletes cards. Undo brings them back with their comments and attachments. */
  async deleteCards(cardIds: readonly string[]): Promise<void> {
    await this.inOneStep(async (groupId) => {
      for (const cardId of cardIds) {
        await this.#perform(
          async () => {
            this.deleting(cardId);
            await this.#backend.deleteBlock(this.boardId, cardId);
            this.cards = this.cards.filter((card) => card.id !== cardId);
            if (this.openCard?.cardId === cardId) this.closeCard();
          },
          () => restoreCard(this, cardId),
          'deleteCard',
          groupId,
        );
      }
    });
  }

  /**
   * Cards were dropped: into a column (their grouping property takes the
   * column's value) and, in a view without a sort, at a place (the view's
   * manual order changes). One undo takes all of it back.
   */
  async dropCards(drop: CardDrop): Promise<void> {
    await dropCardsOn(this, drop);
  }

  // -------------------------------------------------------------- views

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
      () => this.#viewWrites.write(viewId, forward, backward),
      () => this.#viewWrites.write(viewId, backward, forward),
      description,
      groupId,
      this.#viewWrites.keep(viewId, backward),
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

  /** Writes a view patch through this board's view writes (`view-moves.ts`). */
  writeView(viewId: string, forward: BlockPatch, backward: BlockPatch): Promise<void> {
    return this.#viewWrites.write(viewId, forward, backward);
  }

  /** Keeps an inverse of a view write (`view-moves.ts`). */
  keepViewInverse(viewId: string, backward: BlockPatch): () => Promise<void> | undefined {
    return this.#viewWrites.keep(viewId, backward);
  }

  /** Reads the board's views again, waiting for writes of `waitForId` to settle first when given. */
  async rereadViews(waitForId?: string): Promise<void> {
    if (waitForId !== undefined) await this.#viewWrites.settled(waitForId);
    const before = this.views.map((view) => view.id);
    const views = await this.#backend.views(this.boardId);
    if (this.#isInactive()) return;
    this.views = reread(this.views, views, before, (view) => this.#viewWrites.receive(view));
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

  async deleteView(viewId: string): Promise<void> {
    const gone = this.view(viewId);
    if (!gone) return;
    await this.#perform(
      async () => {
        this.deleting(viewId);
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

  /** As `take` (write.ts) does for cards and views. */
  #takeBoard(board: Container): void {
    if (!this.board || board.updatedAt >= this.board.updatedAt) this.board = board;
  }

  /**
   * Changes the board itself: its title, description, icon, or the
   * definitions of its card properties. `change` returns a new object.
   */
  async changeBoard(
    change: (board: Container) => Container,
    description = 'changeBoard',
    groupId?: string,
  ): Promise<void> {
    if (!this.board) return;
    const [forward, backward] = containerPatches(change(this.board), this.board);
    if (isEmptyContainerPatch(forward)) return;
    await this.#perform(
      () => this.#boardWrites.write(forward, backward),
      () => this.#boardWrites.write(backward, forward),
      description,
      groupId,
      this.#boardWrites.keep(backward),
    );
  }

  /**
   * Moves a card property a place up (`-1`) or down (`1`) among `among` (by default all): its
   * own request (`reorderCardProperties`), shown at once; undo moves it back.
   */
  async moveProperty(propertyId: string, step: -1 | 1, among?: readonly string[]): Promise<void> {
    // A move that found the property at an end moved nothing: its undo moves nothing back.
    let moved = false;
    await this.#perform(
      async () => {
        moved = await this.#propertyMoves.move(propertyId, step, among);
      },
      async () => {
        if (moved) await this.#propertyMoves.move(propertyId, step === 1 ? -1 : 1, among);
      },
      'moveProperty',
    );
  }

  readonly #propertyMoves = propertyMovesOf(this);
  // ------------------------------------------------------------ sharing

  /** Whether anybody of the workspace may find and join the board. */
  async setOpen(open: boolean): Promise<void> {
    await this.changeBoard((board) => withVisibility(board, open), 'changeVisibility');
  }

  /** The role somebody gets by joining the board. Members keep theirs. */
  async setMinimumRole(role: ContainerRole): Promise<void> {
    await this.changeBoard((board) => ({ ...board, minimumRole: role }), 'changeMinimumRole');
  }

  /** Makes a person of the workspace a member. Somebody who is a member already keeps their role. */
  addMember(userId: string, role: ContainerRole): Promise<void> {
    return this.#people.add(userId, role);
  }
  setMemberRole(userId: string, role: ContainerRole): Promise<void> {
    return this.#people.setRole(userId, role);
  }
  removeMember(userId: string): Promise<void> {
    return this.#people.remove(userId);
  }

  // --------------------------------------------------------- open card

  /** Opens a card: reads its body, comments and attachments. */
  async showCard(cardId: string): Promise<CardStore> {
    if (this.openCard?.cardId === cardId) return this.openCard;
    this.openCard?.close();
    const store = new CardStore(this, cardId, () => void this.#refresh());
    this.openCard = store;
    await store.load();
    return store;
  }

  closeCard(): void {
    this.openCard?.close();
    this.openCard = undefined;
  }
}
