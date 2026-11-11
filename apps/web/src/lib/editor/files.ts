/**
 * From the identifier of a file to the address it is fetched from.
 *
 * A document names an image by the identifier of an uploaded file and never
 * by an address. This is the one place that turns one into the other; the
 * app sets the resolver once it knows the board and the base path, and the
 * editor's image node asks here.
 *
 * What the address assumes about the server (`getFile` in the contract;
 * the app sets the resolver, src/app/App.svelte):
 *
 * - The session is a cookie that scripts cannot read. The app never holds a
 *   token, so an address carries none: `<img src>` names the file directly
 *   and the browser sends the cookie. (Saving an attachment under its own
 *   name reads its bytes by script, features/card/download.ts: the server
 *   names the download after the file's identifier.)
 * - `GET` needs no extra request header, which is what lets a tag fetch a file.
 * - The API lives under `/api/v1`, below the base path the server is given.
 * - A file goes up in parts through an upload session (`api/upload.ts`). It
 *   has an identifier, and so an address, only once the upload is complete;
 *   until then the editor shows a placeholder and not an image node.
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
