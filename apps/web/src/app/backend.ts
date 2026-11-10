/**
 * Which backend the app runs on. This is the one file that picks an
 * implementation of the data boundary (`lib/state/backend.ts`); everything
 * else sees the interface only, and a lint rule keeps it so.
 *
 * Today that is the one in memory, seeded with the built-in templates. It is
 * fetched as a chunk of its own: the app that talks to the server will not
 * carry it. When the server's contract is published, the implementation that
 * calls the server is chosen here instead.
 */
import type { Backend } from '$lib/state/index.ts';

export interface StartedBackend {
  backend: Backend;
  /**
   * The same data seen by somebody else, when the backend can do that: the
   * in-memory one can. The development panel uses it to let "another person"
   * change a card while the reader has it open.
   */
  asSomeoneElse?: () => Backend;
}

export async function startBackend(): Promise<StartedBackend> {
  const [{ MemoryBackend }, { loadTemplates, seedWorld }] = await Promise.all([
    import('$lib/state/memory/backend.ts'),
    import('$lib/state/memory/seed.ts'),
  ]);
  const { world, readerId } = seedWorld(await loadTemplates());
  const backend = new MemoryBackend(world, readerId);
  // Long enough to see that a change shows before it is answered.
  backend.latency = 40;
  const otherId = [...world.users.keys()].find((id) => id !== readerId);
  const asSomeoneElse = otherId === undefined ? undefined : () => backend.as(otherId);
  // A handle for the browser tests and for trying things by hand in the
  // console: the same data as another person, and the next call failing.
  (window as unknown as { fvociDev?: unknown }).fvociDev = {
    backend,
    someoneElse: asSomeoneElse,
  };
  return { backend, asSomeoneElse };
}
