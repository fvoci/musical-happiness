/**
 * What the tests against the real server share: accounts, and requests to
 * the API made beside the page (to set up what a test needs, or to change
 * something as another user).
 *
 * The first account of a server needs no invitation and becomes its
 * administrator; every account after it needs the team's invitation, which
 * only an administrator can read. `invitation()` makes that first account
 * once per run and reads the invitation with it.
 */
import { expect, request, type APIRequestContext, type Page } from '@playwright/test';

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

const ADMIN: Person = { username: 'admin', email: 'admin@example.test', password: PASSWORD };
let token: string | undefined;

/** The team's invitation. Makes the administrator first if the server has nobody yet. */
export async function invitation(baseURL: string): Promise<string> {
  if (token !== undefined) return token;
  const api = await apiClient(baseURL);
  // Once the server has an account, a registration without the invitation
  // is refused (403) before anything else is looked at: the administrator
  // is there already (made by an earlier worker of this run), and signs in.
  const registered = await api.post('/api/v1/register', { data: ADMIN });
  if (!registered.ok() && registered.status() !== 403) await ok(registered, 'register admin');
  await ok(await api.post('/api/v1/login', { data: ADMIN }), 'login admin');
  const teams = (await (await api.get('/api/v1/teams')).json()) as Schemas['Team'][];
  token = teams[0]?.signupToken;
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
  /** The options of "Status", in order. */
  statuses?: string[];
  cards?: { title: string; status?: string; note?: string }[];
  /** Other people to add, and the role each gets. */
  members?: { who: Person; role: 'viewer' | 'commenter' | 'editor' }[];
}

export interface MadeBoard {
  boardId: string;
  viewId: string;
  teamId: string;
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
  const teams = await json<Schemas['Team'][]>(await api.get('/api/v1/teams'), 'teams');
  const teamId = teams[0]?.id ?? '';
  const statuses = plan.statuses ?? ['To do', 'Doing', 'Done'];
  const options = Object.fromEntries(statuses.map((name, at) => [name, `opt-${at}`]));
  const board = await json<Schemas['Board']>(
    await api.post('/api/v1/boards', {
      data: {
        teamId,
        type: 'P',
        title: plan.title,
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
      await api.get(`/api/v1/teams/${teamId}/users`, { params: { search: who.username } }),
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
  return { boardId: board.id, viewId: view?.id ?? '', teamId, options, cards };
}
