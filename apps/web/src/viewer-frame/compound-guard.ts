/**
 * What the frame does with a compound file (HWP) before its renderer reads
 * it, in the renderer's worker: holds it to the limit on what its streams say
 * they hold, checks that each stream has the sectors it says and that no
 * sector is read as two streams, and reads its `FileHeader` for a password.
 *
 * The streams of an HWP document are deflated inside (the records of its
 * text, its pictures). The renderer unpacks them itself and stops past a
 * budget of its own (about 256 MB in all, as rhwp 0.8.7 does), so what
 * they unpack to is not counted here.
 */
import {
  compoundFile,
  readStream,
  rootStream,
  verifyChains,
} from '$lib/file-viewer/compound-file.ts';
import { holdsTooMuch, HWP_SIGNATURE } from '$lib/file-viewer/formats.ts';

import { LockedFile, UnreadableFile } from './renderer.ts';

/** The signature (32 bytes), the version (4) and the properties (4) of `FileHeader`. */
const HEAD_SIZE = 40;
/** The property that says the document is locked with a password. */
const PASSWORD = 1 << 1;

const ascii = new TextDecoder('ascii');

/** The file as it came, once checked; throws `UnreadableFile` or `LockedFile` otherwise. */
export function checkCompound(buffer: ArrayBuffer): ArrayBuffer {
  const file = compoundFile(new Uint8Array(buffer));
  if (!file || holdsTooMuch(file) || !verifyChains(file)) throw new UnreadableFile();
  const header = rootStream(file, 'FileHeader');
  const head = header && readStream(file, header, HEAD_SIZE);
  if (head?.length !== HEAD_SIZE) throw new UnreadableFile();
  if (ascii.decode(head.subarray(0, HWP_SIGNATURE.length)) !== HWP_SIGNATURE) {
    throw new UnreadableFile();
  }
  const view = new DataView(head.buffer, head.byteOffset, head.byteLength);
  if (view.getUint32(36, true) & PASSWORD) throw new LockedFile();
  return buffer;
}
