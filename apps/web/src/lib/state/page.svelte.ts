/**
 * One open page of a wiki: its body, edited together in its room.
 *
 * The title and the icon are not here: they are in the wiki's list of pages
 * (`page`), so that the tree and the page never disagree, and are changed
 * there (`WikiStore.changePage`).
 *
 * The body is the card's way (`card.svelte.ts`): the page's room
 * (`Backend.openDoc` with the page in place of the card) holds the document
 * the reader sees and writes, and the server writes the page's body from it.
 * `body` is the body as the page was last read, shown until the room's
 * document has arrived, and read again when the page changes while no room's
 * document is held. The comments on the page are `comments`
 * (`page-comments.svelte.ts`), read with it.
 */
import type { DocNode } from '../editor/schema.ts';
import { isBackendError, type BodyDoc, type BodyDocState, type WikiPage } from './backend.ts';
import { REJOIN_SPACING, afterPutOut } from './card-parts.ts';
import { PageComments } from './page-comments.svelte.ts';
import type { WikiStore } from './wiki.svelte.ts';
import type { LoadStatus } from './workspace.svelte.ts';

const sameDoc = (first: DocNode | null, second: DocNode | null): boolean =>
  JSON.stringify(first) === JSON.stringify(second);

export class PageStore {
  readonly wiki: WikiStore;
  readonly pageId: string;

  status = $state<LoadStatus>('idle');
  /** The page is gone (deleted here or by somebody else), or never was. */
  missing = $state(false);
  /**
   * The page was shown and is gone since: by somebody else (`others`, to be
   * told), or by the reader (deleted, or its making undone). Its title as it was.
   */
  removed = $state.raw<{ by: 'others' | 'reader'; title: string } | undefined>(undefined);
  /** The body as it is stored, as far as this store knows. */
  body = $state.raw<DocNode | null>(null);
  /** The body in its room, while it is open there (`openBody`). */
  room = $state.raw<BodyDocState | undefined>(undefined);
  /** Why things of the reader's were dropped, each once, until dismissed (`CardStore.lost`). */
  lost = $state.raw<readonly string[]>([]);

  /** The version of the page the body was read at. */
  #readAt = 0;
  #bodyDoc: BodyDoc | undefined;
  #closed = false;
  #rejoinedAt = -Infinity;
  /** News of the page heard while the body was the room's: read once it is not. */
  #news: WikiPage | undefined;
  /** Readings of the body begun: the latest is the one that counts. */
  #readings = 0;
  /** The page was shown (read once): its going is told, whoever finds out first. */
  #shown = false;

  /** The comments on the page. */
  readonly comments: PageComments;

  constructor(wiki: WikiStore, pageId: string) {
    this.wiki = wiki;
    this.pageId = pageId;
    this.comments = new PageComments(wiki, pageId);
  }

  get #wikiId(): string {
    return this.wiki.wikiId;
  }

  #read(): Promise<{ body: DocNode | null; updateAt: number }> {
    const wikis = this.wiki.workspace.backend.wiki;
    if (!wikis) return Promise.reject(new Error('this backend has no wikis'));
    return wikis.page(this.#wikiId, this.pageId);
  }

  async load(): Promise<void> {
    this.status = 'loading';
    await Promise.all([this.#readBody(true), this.comments.read(true)]);
  }

  /** Reads the body and the comments again after changes may have been missed (the wiki's `resync`). */
  async refresh(): Promise<void> {
    if (this.status !== 'ready') return;
    await Promise.all([
      this.#busy() ? undefined : this.#readBody(false),
      this.comments.read(false),
    ]);
  }

  async #readBody(first: boolean): Promise<void> {
    this.#readings += 1;
    const reading = this.#readings;
    try {
      const detail = await this.#read();
      if (this.#closed || reading !== this.#readings) return;
      if (!first && detail.updateAt <= this.#readAt) return;
      this.#readAt = detail.updateAt;
      if (!sameDoc(detail.body, this.body)) this.body = detail.body;
      [this.status, this.missing] = ['ready', false];
      this.#shown = true;
    } catch (error) {
      if (this.#closed || reading !== this.#readings) return;
      if (isBackendError(error) && (error.status === 404 || error.status === 403)) this.gone();
      else if (first) {
        this.status = 'failed';
        this.wiki.workspace.report('load', error);
      }
    }
  }

  close(): void {
    this.#closed = true;
    this.comments.close();
    this.closeBody();
  }

  /**
   * The page was deleted (with one above it, perhaps): nothing more is read or
   * written of it. `by` says who, when it was shown; `title` is its title then.
   * Who is known from the wiki's list only: the room or a reading may find the
   * page gone first (no `by`), and the list then says who.
   */
  gone(by?: 'others' | 'reader', title = ''): void {
    if (by && this.#shown && !this.removed) this.removed = { by, title };
    this.missing = true;
    this.status = 'failed';
    this.closeBody();
  }

  /** The page changed in the wiki's list: its body is read again unless the room's is shown. */
  pageChanged(page: WikiPage): void {
    if (page.id !== this.pageId || page.updateAt <= this.#readAt || this.missing) return;
    if (this.#busy()) {
      this.#news = page;
      return;
    }
    void this.#readBody(false);
  }

  /** The body shown is the room's document, or the page is closed. */
  #busy(): boolean {
    return this.#closed || this.room?.doc !== undefined;
  }

  #catchUp(): void {
    const news = this.#news;
    if (!news || this.#busy()) return;
    this.#news = undefined;
    this.pageChanged(news);
  }

  // --------------------------------------------------------------- body

  /** Opens the body in its room, unless it is open there, gone, or the backend has no rooms. */
  openBody(): void {
    const backend = this.wiki.workspace.backend;
    const workspaceId = this.wiki.workspace.info?.id;
    if (this.#bodyDoc || this.#closed || this.missing || !backend.openDoc || !workspaceId) return;
    const opened = backend.openDoc(workspaceId, this.#wikiId, this.pageId, {
      state: (state) => {
        if (this.#bodyDoc !== opened) return;
        const shut = state.status === 'out' && state.reason === 'not_found';
        if (shut && this.room?.reason !== 'not_found') {
          void this.#stillThere(opened, this.room?.doc !== undefined);
        }
        this.room = state;
        if (!state.doc) this.#catchUp();
      },
      discarded: (why) => {
        if (this.#bodyDoc === opened) this.sayLost(why);
      },
    });
    this.#bodyDoc = opened;
    this.room = opened.state;
  }

  /**
   * The room let the reader go as for a page that is not there: read again,
   * gone it is missing; still there, a room that gave the reader its
   * document is joined again, not sooner than `REJOIN_SPACING` after the last.
   */
  async #stillThere(opened: BodyDoc, heldDoc: boolean): Promise<void> {
    const read = await afterPutOut(() => this.#read());
    if (this.#bodyDoc !== opened) return;
    if (read.found === 'there' && heldDoc && Date.now() - this.#rejoinedAt >= REJOIN_SPACING) {
      this.#rejoinedAt = Date.now();
      opened.retry();
    }
    if (read.found === 'ended') this.wiki.workspace.report('load', read.error);
    if (read.found === 'gone') this.gone();
  }

  /** Leaves the body's room; what waits is sent first. */
  closeBody(): void {
    const opened = this.#bodyDoc;
    this.#bodyDoc = undefined;
    this.room = undefined;
    opened?.close();
  }

  retryBody(): void {
    this.#bodyDoc?.retry();
  }

  /** Sends what the reader wrote at once; settles when the server has it, or cannot now. */
  settleBody(): Promise<void> {
    return this.#bodyDoc?.settle() ?? Promise.resolve();
  }

  sayLost(why: string): void {
    if (!this.lost.includes(why)) this.lost = [...this.lost, why];
  }

  dismissLost(): void {
    this.lost = [];
  }

  /** Uploads an image for the body, a file of the wiki, and gives the identifier its node names. */
  async uploadImage(file: File): Promise<string> {
    try {
      return (await this.wiki.workspace.backend.upload(this.#wikiId, file)).fileId;
    } catch (error) {
      this.wiki.workspace.report('upload', error);
      throw error;
    }
  }
}
