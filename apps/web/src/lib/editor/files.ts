/**
 * From the identifier of a file to the address it is fetched from.
 *
 * A document names an image by the identifier of an uploaded file and never
 * by an address. This is the one place that turns one into the other; the
 * app sets the resolver once it knows the board and the base path, and the
 * editor's image node asks here.
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
