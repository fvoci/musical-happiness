/**
 * What the frame does with a compound file (HWP) before its renderer reads
 * it, in the renderer's worker: checks that each stream has the sectors it
 * says and that no sector is read as two streams, reads its `FileHeader` for
 * a password, and holds what its streams unpack to, together, to the limit.
 *
 * The streams of an HWP document are deflated inside (the records of its
 * text, its pictures). rhwp (0.8.7) unpacks them itself: the text (DocInfo,
 * BodyText, ViewText) within a budget of its own (256 MiB a stream, 512 MiB
 * in all), but the pictures (BinData) with none, and a file it reads leniently
 * (one its compound file reader refuses) the same. So every stream is
 * unpacked here first, as rhwp unpacks it (raw deflate, then zlib, and as it
 * came when neither reads it), counting the bytes and keeping none. What
 * comes out before a stream fails to unpack counts as well: rhwp may have
 * taken it, and the memory it took stays.
 */
import {
  COMPOUND_TYPES,
  compoundFile,
  readStream,
  rootStream,
  verifyChains,
  type CompoundFile,
} from '$lib/file-viewer/compound-file.ts';
import { HWP_SIGNATURE, UNPACKED_LIMIT_BYTES } from '$lib/file-viewer/formats.ts';

import { LockedFile, TooLargeFile, UnreadableFile } from './renderer.ts';

/** The signature (32 bytes), the version (4) and the properties (4) of `FileHeader`. */
const HEAD_SIZE = 40;
/** The property that says the document is locked with a password. */
const PASSWORD = 1 << 1;
/** How rhwp unpacks a stream, in the order it tries. */
const DEFLATES = ['deflate-raw', 'deflate'] as const;

const ascii = new TextDecoder('ascii');

/** How much a stream unpacks to as `format`, counted as far as `room`; and whether it all did. */
async function inflated(
  data: Uint8Array<ArrayBuffer>,
  format: (typeof DEFLATES)[number],
  room: number,
): Promise<{ size: number; whole: boolean }> {
  const reader = new Blob([data]).stream().pipeThrough(new DecompressionStream(format)).getReader();
  let size = 0;
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) return { size, whole: true };
      size += value.byteLength;
      if (size > room) break;
    }
  } catch {
    return { size, whole: false };
  }
  await reader.cancel().catch(() => undefined);
  return { size, whole: false };
}

/** The most a stream may unpack to as rhwp unpacks it, counted as far as `room`. */
async function unpackedSize(data: Uint8Array<ArrayBuffer>, room: number): Promise<number> {
  let most = data.byteLength;
  for (const format of DEFLATES) {
    const { size, whole } = await inflated(data, format, room);
    most = Math.max(most, size);
    if (whole || most > room) break;
  }
  return most;
}

/** Throws `TooLargeFile` when the streams unpack to more than `limit` together. */
async function holdUnpacked(file: CompoundFile, limit: number): Promise<void> {
  let total = 0;
  for (const entry of file.entries) {
    if (entry.type !== COMPOUND_TYPES.STREAM) continue;
    const data = readStream(file, entry, entry.size);
    if (!data) throw new UnreadableFile();
    total += await unpackedSize(data, limit - total);
    if (total > limit) throw new TooLargeFile();
  }
}

/**
 * The file as it came, once checked; rejects with `UnreadableFile`,
 * `LockedFile` or `TooLargeFile` otherwise. `limit` is what the streams may unpack to.
 */
export async function checkCompound(
  buffer: ArrayBuffer,
  limit = UNPACKED_LIMIT_BYTES,
): Promise<ArrayBuffer> {
  const file = compoundFile(new Uint8Array(buffer));
  if (!file || !verifyChains(file)) throw new UnreadableFile();
  const header = rootStream(file, 'FileHeader');
  const head = header && readStream(file, header, HEAD_SIZE);
  if (head?.length !== HEAD_SIZE) throw new UnreadableFile();
  if (ascii.decode(head.subarray(0, HWP_SIGNATURE.length)) !== HWP_SIGNATURE) {
    throw new UnreadableFile();
  }
  const view = new DataView(head.buffer, head.byteOffset, head.byteLength);
  if (view.getUint32(36, true) & PASSWORD) throw new LockedFile();
  await holdUnpacked(file, limit);
  return buffer;
}
