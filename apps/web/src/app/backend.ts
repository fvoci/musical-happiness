/**
 * Which backend the app runs on. This is the one file that picks an
 * implementation of the data boundary (`lib/state/backend.ts`); everything
 * else sees the interface only, and a lint rule keeps it so.
 *
 * Two are there: the server's API (`lib/api/`), and one in memory, seeded
 * with the built-in templates, for looking at the screens without a server.
 * The page says which (`chosenBackend`). Each is fetched as a chunk of its
 * own, so the app the server serves does not carry the one in memory.
 */
import type { Account, Backend, People } from '$lib/state/index.ts';
import type { PasswordRecovery } from '$lib/state/password-recovery.ts';
import type { Organizations } from '$lib/state/organization.ts';

import { readBasePath } from './router.svelte.ts';
import { display } from '$lib/display.svelte.ts';
import { DEFAULT_ACCOUNT_SETTINGS } from '$lib/logic/account-settings.ts';
import { readDisplaySettings } from '$lib/logic/display.ts';
import { readLanguageSettings } from '$lib/logic/language.ts';
import { readRegionalSettings } from '$lib/logic/regional.ts';

export interface StartedBackend {
  backend: Backend;
  /** Signing in and out; only a backend with accounts has it. */
  account?: Account;
  /** Managing who is in a workspace; only a backend with accounts has it. */
  people?: People;
  /** The organization's administration and the workspaces' records; only the server has them. */
  organizations?: Organizations;
}

export type BackendName = 'server' | 'memory';

/**
 * The backend the page asks for in `<meta name="fvoci:backend">`. The
 * development aids that serve the app without a server (tools/preview.ts,
 * tools/serve.ts) write `memory` there; the server writes nothing, and a
 * built app then calls it. The development server of Vite serves the
 * source, without the tag, and runs in memory.
 */
export function chosenBackend(document: Document): BackendName {
  const asked = document.querySelector<HTMLMetaElement>('meta[name="fvoci:backend"]')?.content;
  if (asked === 'memory' || asked === 'server') return asked;
  return import.meta.env.DEV ? 'memory' : 'server';
}

/** Where the in-memory backend keeps the reader's settings between visits. */
const SETTINGS_KEY = 'fvoci:memory-settings';

/**
 * A world in memory starts from its seed on every visit, and with it the
 * reader's account settings. So that a theme or a time zone chosen in
 * the settings is still there after a reload, as it would be with a server,
 * the settings are kept on the device and given back to the new world.
 */
async function keepSettings(backend: Backend): Promise<void> {
  try {
    const saved: unknown = JSON.parse(localStorage.getItem(SETTINGS_KEY) ?? 'null');
    // Only the settings this version knows, each read as it would be from
    // the server: a name it no longer has (one saved by an older version) is
    // left out, not refused with all the others.
    if (saved)
      await backend.patchSettings({
        ...readRegionalSettings(saved),
        ...readDisplaySettings(saved),
        ...readLanguageSettings(saved),
      });
  } catch {
    // Nothing kept, or something the backend refuses now: the defaults then.
  }
  backend.watchSelf((change) => {
    if (change.kind !== 'settings') return;
    try {
      localStorage.setItem(SETTINGS_KEY, JSON.stringify(change.settings));
    } catch {
      // Without storage the settings last for the visit.
    }
  });
}

async function startServer(): Promise<StartedBackend> {
  const [{ ServerBackend }, { Http }, { ServerOrganizations }] = await Promise.all([
    import('$lib/api/backend.ts'),
    import('$lib/api/http.ts'),
    import('$lib/api/organizations.ts'),
  ]);
  const base = readBasePath(document);
  const path = base === '' ? '' : `/${base}`;
  const backend = new ServerBackend(path);
  return {
    backend,
    account: backend,
    people: backend.people,
    organizations: new ServerOrganizations(new Http(path)),
  };
}

/**
 * The backend of a reader by a board's public link (`/shared/…`): the
 * server's, which reads the board by the link's read token alone, and the
 * place it shows the board in.
 */
export async function startShared(
  boardId: string,
  readToken: string,
): Promise<{ backend: Backend; place: string }> {
  const { SharedBackend, SHARED_PLACE } = await import('$lib/api/shared-backend.ts');
  const base = readBasePath(document);
  // The device's theme and density stay as they are: the reader has no account's.
  const settings = { ...DEFAULT_ACCOUNT_SETTINGS, ...display.settings };
  const backend = new SharedBackend(base === '' ? '' : `/${base}`, boardId, readToken, settings);
  return { backend, place: SHARED_PLACE };
}

export async function startBackend(): Promise<StartedBackend> {
  return chosenBackend(document) === 'server' ? startServer() : startMemory();
}

async function startMemory(): Promise<StartedBackend> {
  const [{ MemoryBackend }, { loadTemplates, seedWorld }] = await Promise.all([
    import('$lib/state/memory/backend.ts'),
    import('$lib/state/memory/seed.ts'),
  ]);
  const { world, readerId } = seedWorld(await loadTemplates());
  const backend = new MemoryBackend(world, readerId);
  // Long enough to see that a change shows before it is answered.
  backend.latency = 40;
  await keepSettings(backend);
  const otherId = [...world.users.keys()].find((id) => id !== readerId);
  const asSomeoneElse = otherId === undefined ? undefined : () => backend.as(otherId);
  // A handle for the browser tests and for trying things by hand in the
  // console: the same data as another person, and the next call failing.
  (window as unknown as { fvociDev?: unknown }).fvociDev = {
    backend,
    someoneElse: asSomeoneElse,
  };
  return { backend };
}

/** Recovery is available before there is an account or a workspace to load. */
export async function startPasswordRecovery(): Promise<PasswordRecovery> {
  const [{ ServerPasswordRecovery }, { Http }] = await Promise.all([
    import('$lib/api/password-recovery.ts'),
    import('$lib/api/http.ts'),
  ]);
  const base = readBasePath(document);
  return new ServerPasswordRecovery(new Http(base === '' ? '' : `/${base}`));
}
