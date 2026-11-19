/**
 * The built-in templates over HTTP. The server has them in several languages
 * and goes by the account's settings; where those leave the language to the
 * device, by the `language` these requests carry: the one the screen is in.
 */
import type { Container } from '../logic/wire.ts';

import { containerOf, type Schemas } from './convert.ts';
import type { Http } from './http.ts';
import { underWorkspace } from './requests.ts';

/** `getTemplates`: the workspace's own templates, then the built-in ones in one language. */
export async function templatesOf(
  http: Http,
  workspaceId: string,
  language: string,
): Promise<Container[]> {
  const boards = await http.request<Schemas['Container'][]>(
    underWorkspace('/templates'),
    { workspaceId },
    { query: { language: language || undefined } },
  );
  return boards.map(containerOf);
}

/** `onboard`: a copy of the built-in welcome board, as a board of the user's own. Answers its identifier. */
export async function makeWelcomeBoard(
  http: Http,
  workspaceId: string,
  language: string,
): Promise<string> {
  const made = await http.request<Schemas['WelcomeBoard']>(
    underWorkspace('/onboard'),
    { workspaceId },
    { method: 'POST', query: { language: language || undefined } },
  );
  return made.boardId;
}
