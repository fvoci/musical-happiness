/**
 * The member names of a ZIP archive, read from its central directory without
 * inflating anything: enough to tell an Office document from another archive.
 *
 * Only what the viewer opens is handled: no ZIP64 (a file of at most
 * `VIEW_LIMIT_BYTES` does not need it) and no archive split over several
 * files. Anything else, and anything malformed, gives undefined: the bytes are
 * then not a kind the viewer draws.
 */

const END_SIGNATURE = 0x06054b50;
const ENTRY_SIGNATURE = 0x02014b50;
/** The end record is 22 bytes, followed by a comment of at most 65535. */
const END_SIZE = 22;
const END_SEARCH = END_SIZE + 0xffff;
const ENTRY_SIZE = 46;
/** More names than any Office document has; past this the archive is not one the viewer reads. */
const NAME_LIMIT = 10_000;

const decoder = new TextDecoder('utf-8', { fatal: false });

function endRecord(view: DataView): number | undefined {
  const lowest = Math.max(0, view.byteLength - END_SEARCH);
  for (let at = view.byteLength - END_SIZE; at >= lowest; at -= 1) {
    if (view.getUint32(at, true) === END_SIGNATURE) return at;
  }
  return undefined;
}

/** The names in the archive's central directory, or undefined when the bytes are no archive this reads. */
export function zipMemberNames(bytes: Uint8Array): string[] | undefined {
  if (bytes.byteLength < END_SIZE) return undefined;
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const end = endRecord(view);
  if (end === undefined) return undefined;
  const disk = view.getUint16(end + 4, true);
  const count = view.getUint16(end + 10, true);
  const size = view.getUint32(end + 12, true);
  let at = view.getUint32(end + 16, true);
  if (disk !== 0 || count > NAME_LIMIT || at + size > end) return undefined;

  const names: string[] = [];
  for (let index = 0; index < count; index += 1) {
    if (at + ENTRY_SIZE > end || view.getUint32(at, true) !== ENTRY_SIGNATURE) return undefined;
    const nameLength = view.getUint16(at + 28, true);
    const extraLength = view.getUint16(at + 30, true);
    const commentLength = view.getUint16(at + 32, true);
    const nameStart = at + ENTRY_SIZE;
    if (nameStart + nameLength > end) return undefined;
    names.push(decoder.decode(bytes.subarray(nameStart, nameStart + nameLength)));
    at = nameStart + nameLength + extraLength + commentLength;
  }
  return names;
}
