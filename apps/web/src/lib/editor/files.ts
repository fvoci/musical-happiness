/**
 * From the identifier of a file to the address it is fetched from.
 *
 * A document names an image by the identifier of an uploaded file and never
 * by an address. This is the one place that turns one into the other; the
 * app sets the resolver once it knows the board and the base path, and the
 * editor's image node asks here.
 *
 * What the address may assume about the server (its design, sections 5.1 and
 * 5.6; the server's side of it is not built yet, and the app sets a resolver
 * for the in-memory backend in the meantime, src/app/App.svelte):
 *
 * - The session is a cookie that scripts cannot read. The app never holds a
 *   token, so an address carries none, and nothing is fetched by script to be
 *   turned into a blob address: `<img src>` and plain links name the file
 *   directly and the browser sends the cookie. A shared, read-only board is
 *   the one case where the address carries something: the read token of that
 *   board, in the query.
 * - `GET` needs no extra request header, which is what lets a tag fetch a file.
 * - The API lives under `/api/v1`, below the base path the server is given.
 * - There is no single-user mode and no token of its own to append for it.
 * - A large file goes up in pieces (an upload session: create, send the
 *   numbered pieces, complete). The server names the size of a piece, 8 MiB
 *   unless configured otherwise; a file no larger than one piece may go up in
 *   a single request. A file has an identifier, and so an address, only once
 *   the upload is complete; until then the editor shows a placeholder and
 *   not an image node.
 */

export type FileUrlResolver = (fileId: string) => string | undefined;

let resolver: FileUrlResolver = () => undefined;

/** Sets how file identifiers become addresses. Returns the previous resolver. */
export function setFileUrlResolver(next: FileUrlResolver): FileUrlResolver {
  const previous = resolver;
  resolver = next;
  return previous;
}

/** The address of a file, or undefined when it cannot be shown. */
export function fileUrl(fileId: string): string | undefined {
  return resolver(fileId);
}
