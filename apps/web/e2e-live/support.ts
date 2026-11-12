/**
 * What the tests against the real server share: accounts, and requests to
 * the API made beside the page (to set up what a test needs, or to change
 * something as another user).
 *
 * The first account of a server needs no invitation and becomes its
 * administrator; every account after it needs the workspace's invitation, which
 * only an administrator can read. `invitation()` makes that first account
 * once per run and reads the invitation with it.
 */
import { randomBytes } from 'node:crypto';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { DatabaseSync } from 'node:sqlite';

import { expect, request, type APIRequestContext, type Page } from '@playwright/test';

import { addressOf, placeOf } from '../e2e/address.ts';

import type { components } from '../src/lib/api/contract.ts';

export type Schemas = components['schemas'];

export interface Person {
  username: string;
  email: string;
  password: string;
}

const PASSWORD = 'correct horse battery';

let made = 0;
/** A name nobody has used on this server: the tests share it. */
export function uniqueName(prefix: string): string {
  made += 1;
  return `${prefix}-${Date.now().toString(36)}-${made}`;
}

export function person(prefix: string): Person {
  const username = uniqueName(prefix);
  return { username, email: `${username}@example.test`, password: PASSWORD };
}

/** The address of a board made for a test, in its workspace, a view of it, a card on that view. */
export function boardPath(
  board: { workspaceId: string; boardId: string },
  viewId?: string,
  cardId?: string,
): string {
  return addressOf({ workspaceId: board.workspaceId, boardId: board.boardId, viewId, cardId });
}

export { placeOf };

/** A client of the API, signed in as nobody yet. */
export async function apiClient(baseURL: string): Promise<APIRequestContext> {
  return request.newContext({
    baseURL,
    extraHTTPHeaders: { 'X-Requested-With': 'XMLHttpRequest' },
  });
}

async function ok(response: Awaited<ReturnType<APIRequestContext['get']>>, what: string) {
  if (!response.ok()) {
    throw new Error(`${what}: ${response.status()} ${await response.text()}`);
  }
}

export const ADMIN: Person = { username: 'admin', email: 'admin@example.test', password: PASSWORD };
let token: string | undefined;

/** The workspace's invitation. Makes the administrator first if the server has nobody yet. */
export async function invitation(baseURL: string): Promise<string> {
  if (token !== undefined) return token;
  const api = await apiClient(baseURL);
  // Once the server has an account, a registration without the invitation
  // is refused (403) before anything else is looked at: the administrator
  // is there already (made by an earlier worker of this run), and signs in.
  const registered = await api.post('/api/v1/register', { data: ADMIN });
  if (!registered.ok() && registered.status() !== 403) await ok(registered, 'register admin');
  await ok(await api.post('/api/v1/login', { data: ADMIN }), 'login admin');
  const [workspace] = (await (
    await api.get('/api/v1/users/me/workspaces')
  ).json()) as Schemas['UserWorkspace'][];
  const found = workspace
    ? ((await (await api.get(`/api/v1/w/${workspace.id}`)).json()) as Schemas['Workspace'])
    : undefined;
  token = found?.signupToken;
  await api.dispose();
  if (!token) throw new Error('the administrator was given no invitation');
  return token;
}

/** Makes an account through the API. */
export async function register(baseURL: string, who: Person): Promise<void> {
  const api = await apiClient(baseURL);
  const answer = await api.post('/api/v1/register', {
    data: { ...who, token: await invitation(baseURL) },
  });
  await ok(answer, `register ${who.username}`);
  await api.dispose();
}

/** A client of the API signed in as `who`. */
export async function signedIn(baseURL: string, who: Person): Promise<APIRequestContext> {
  const api = await apiClient(baseURL);
  await ok(
    await api.post('/api/v1/login', { data: { username: who.username, password: who.password } }),
    `login ${who.username}`,
  );
  return api;
}

/** Signs in on the sign-in page, which the app shows to whoever is signed out. */
export async function signInOnPage(page: Page, who: Person): Promise<void> {
  const form = page.getByTestId('sign-in-form');
  await expect(form).toBeVisible();
  await form.getByLabel(/user name or email/i).fill(who.username);
  await form.getByLabel(/^password$/i).fill(who.password);
  await form.getByRole('button', { name: /^sign in$/i }).click();
  await expect(page.getByTestId('app-shell')).toBeVisible();
}

/** Fails the test on anything the page throws. Call before the page loads. */
export function watchErrors(page: Page): string[] {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.stack ?? error.message));
  return errors;
}

/**
 * What a test board is made of: a select property "Status", a text property
 * "Note", a date property "Due", and one kanban view grouped by "Status".
 */
export interface BoardPlan {
  title: string;
  /** `O`: open to the workspace, anyone may join. `P` (the default): members only. */
  type?: 'O' | 'P';
  description?: string;
  /** The options of "Status", in order. */
  statuses?: string[];
  cards?: { title: string; status?: string; note?: string }[];
  /** The workspace to make it in; the first of the maker's when none is named. */
  workspaceId?: string;
  /** Other people to add, and the role each gets. */
  members?: { who: Person; role: 'viewer' | 'commenter' | 'editor' }[];
}

export interface MadeBoard {
  boardId: string;
  viewId: string;
  workspaceId: string;
  /** Option identifiers of "Status" by name. */
  options: Record<string, string>;
  /** Card identifiers by title. */
  cards: Record<string, string>;
}

async function json<T>(answer: Awaited<ReturnType<APIRequestContext['get']>>, what: string) {
  await ok(answer, what);
  return (await answer.json()) as T;
}

/** Makes a board through the API as the person `api` is signed in as, who becomes its admin. */
export async function makeBoard(api: APIRequestContext, plan: BoardPlan): Promise<MadeBoard> {
  const workspaces = await json<Schemas['UserWorkspace'][]>(
    await api.get('/api/v1/users/me/workspaces'),
    'workspaces',
  );
  const workspaceId = plan.workspaceId ?? workspaces[0]?.id ?? '';
  const statuses = plan.statuses ?? ['To do', 'Doing', 'Done'];
  const options = Object.fromEntries(statuses.map((name, at) => [name, `opt-${at}`]));
  const board = await json<Schemas['Board']>(
    await api.post('/api/v1/boards', {
      data: {
        workspaceId,
        type: plan.type ?? 'P',
        title: plan.title,
        ...(plan.description ? { description: plan.description } : {}),
        cardProperties: [
          {
            id: 'status',
            name: 'Status',
            type: 'select',
            options: statuses.map((name) => ({
              id: options[name],
              value: name,
              color: 'propColorBlue',
            })),
          },
          { id: 'note', name: 'Note', type: 'text', options: [] },
          { id: 'due', name: 'Due', type: 'date', options: [] },
        ],
      },
    }),
    'create board',
  );
  const [view] = await json<Schemas['Block'][]>(
    await api.post(`/api/v1/boards/${board.id}/blocks`, {
      data: [
        {
          boardId: board.id,
          type: 'view',
          title: 'By status',
          createAt: 1,
          updateAt: 1,
          fields: {
            viewType: 'board',
            groupById: 'status',
            visibleOptionIds: [...Object.values(options), ''],
            hiddenOptionIds: [],
            collapsedOptionIds: [],
            sortOptions: [],
            visiblePropertyIds: [],
            cardOrder: [],
            filter: { operation: 'and', filters: [] },
          },
        },
      ],
    }),
    'create view',
  );
  const cards: Record<string, string> = {};
  for (const card of plan.cards ?? []) {
    const status = card.status === undefined ? undefined : options[card.status];
    const properties = {
      ...(status ? { status } : {}),
      ...(card.note === undefined ? {} : { note: card.note }),
    };
    const made = await json<Schemas['Card']>(
      await api.post(`/api/v1/boards/${board.id}/cards`, {
        data: { title: card.title, properties },
      }),
      `create card ${card.title}`,
    );
    cards[card.title] = made.id;
  }
  for (const { who, role } of plan.members ?? []) {
    const found = await json<Schemas['User'][]>(
      await api.get(`/api/v1/w/${workspaceId}/users`, { params: { search: who.username } }),
      `find ${who.username}`,
    );
    const user = found.find((candidate) => candidate.username === who.username);
    if (!user) throw new Error(`no user ${who.username}`);
    await ok(
      await api.post(`/api/v1/boards/${board.id}/members`, {
        data: {
          userId: user.id,
          schemeViewer: true,
          schemeCommenter: role !== 'viewer',
          schemeEditor: role === 'editor',
        },
      }),
      `add ${who.username}`,
    );
  }
  return { boardId: board.id, viewId: view?.id ?? '', workspaceId, options, cards };
}

/** An identifier as the server makes them (UUID version 7). */
function newId(): string {
  const hex = Date.now().toString(16).padStart(12, '0') + randomBytes(10).toString('hex');
  const variant = ((parseInt(hex.slice(16, 17), 16) & 0x3) | 0x8).toString(16);
  return [
    hex.slice(0, 8),
    hex.slice(8, 12),
    `7${hex.slice(13, 16)}`,
    `${variant}${hex.slice(17, 20)}`,
    hex.slice(20, 32),
  ].join('-');
}

/** The database of the live server on the port of `baseURL` (`tools/live-server.ts`). */
function serverDatabase(baseURL: string): DatabaseSync {
  const port = new URL(baseURL).port;
  const pointer = path.resolve(import.meta.dirname, `../../../target/web-live/server-${port}.json`);
  const { data } = JSON.parse(readFileSync(pointer, 'utf8')) as { data: string };
  return new DatabaseSync(path.join(data, 'fvoci.db'), { timeout: 10_000 });
}

/**
 * Makes `who` a member of a new workspace of their organization, as its
 * administrator, and returns its identifier. No request of the server makes
 * a workspace yet, so it is written into the server's database
 * (`tools/live-server.ts` says where that is), as the server would.
 */
export function addWorkspace(baseURL: string, who: Person, name: string): string {
  const database = serverDatabase(baseURL);
  try {
    const user = database.prepare('SELECT id FROM users WHERE username = ?').get(who.username) as
      { id: string } | undefined;
    if (!user) throw new Error(`no user ${who.username}`);
    const home = database
      .prepare(
        'SELECT w.org_id, w.settings FROM workspaces AS w JOIN workspace_members AS m ' +
          'ON m.workspace_id = w.id WHERE m.user_id = ? AND NOT w.is_system LIMIT 1',
      )
      .get(user.id) as { org_id: string; settings: string } | undefined;
    if (!home) throw new Error(`${who.username} works in no workspace`);
    const id = newId();
    const now = Date.now();
    database
      .prepare(
        'INSERT INTO workspaces (id, signup_token, settings, modified_by, create_at, update_at, ' +
          'org_id, name) VALUES (?, ?, ?, ?, ?, ?, ?, ?)',
      )
      .run(
        id,
        randomBytes(16).toString('hex'),
        home.settings,
        user.id,
        now,
        now,
        home.org_id,
        name,
      );
    database
      .prepare(
        'INSERT INTO workspace_members (workspace_id, user_id, role, create_at) VALUES (?, ?, ?, ?)',
      )
      .run(id, user.id, 'admin', now);
    return id;
  } finally {
    database.close();
  }
}

/**
 * Makes `who` a member of the workspace `workspaceId` too, written into the
 * server's database as `addWorkspace` does.
 */
export function joinWorkspace(baseURL: string, who: Person, workspaceId: string): void {
  const database = serverDatabase(baseURL);
  try {
    database
      .prepare(
        'INSERT INTO workspace_members (workspace_id, user_id, role, create_at) ' +
          'SELECT ?, id, ?, ? FROM users WHERE username = ?',
      )
      .run(workspaceId, 'member', Date.now(), who.username);
  } finally {
    database.close();
  }
}
