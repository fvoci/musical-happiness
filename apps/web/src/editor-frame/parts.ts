/**
 * The parts of an HWPX package that a save leaves out, which rhwp's report of
 * the save does not name.
 *
 * rhwp writes the parts it reads: a part it has no use for (a history, a part
 * of another program's, a section past the count the header gives) is gone
 * from the saved file, with a report of no losses. So the frame compares the
 * package as it was opened with the saved one, and what is missing is told as
 * a loss, which the writer is asked about as about rhwp's own.
 *
 * A part under `BinData/` is found by what it holds (its CRC-32 and size, as
 * the archive's directory gives them): rhwp names the pictures and other data
 * it writes anew. Every other part is found by its name. This finds parts left
 * out, not parts kept under their name and written otherwise: what rhwp writes
 * of what it reads is its report's to say.
 */
import {
  LOSS_COUNT_LIMIT,
  LOSS_LIST_LIMIT,
  LOSS_PATH_LIMIT,
  type Loss,
} from '$lib/file-viewer/editor-protocol.ts';
import { zipEntries, type ZipEntry } from '$lib/file-viewer/zip-directory.ts';

const BINARY = 'BinData/';

/** What a package holds: its parts, folders left aside. Throws on bytes that are no archive. */
export function packageParts(bytes: Uint8Array): readonly ZipEntry[] {
  const entries = zipEntries(bytes);
  if (!entries) throw new Error('not an archive');
  return entries.filter((entry) => !entry.name.endsWith('/'));
}

const content = (part: ZipEntry): string => `${String(part.crc)}:${String(part.size)}`;

/** The parts of `given` that the saved package does not have, each as a loss. */
export function partsLeftOut(given: readonly ZipEntry[], saved: Uint8Array): Loss[] {
  const kept = packageParts(saved);
  const names = new Set(kept.map((part) => part.name));
  // How many of each content the saved package's data holds, taken as they are found again.
  const data = new Map<string, number>();
  for (const part of kept) {
    if (part.name.startsWith(BINARY)) data.set(content(part), (data.get(content(part)) ?? 0) + 1);
  }
  return given.flatMap((part): Loss[] => {
    if (part.name.startsWith(BINARY)) {
      const left = data.get(content(part)) ?? 0;
      if (left > 0) {
        data.set(content(part), left - 1);
        return [];
      }
    } else if (names.has(part.name)) {
      return [];
    }
    const path = part.name.slice(0, LOSS_PATH_LIMIT);
    return [{ code: 'partOmitted', subject: 'packagePart', reason: 'notKeptByEditor', path }];
  });
}

/** rhwp's report with the parts left out added: counted, and listed first as far as a message lists. */
export function withPartsLeftOut(
  report: { count: number; losses: Loss[] },
  left: Loss[],
): { count: number; losses: Loss[] } {
  return {
    count: Math.min(report.count + left.length, LOSS_COUNT_LIMIT),
    losses: [...left, ...report.losses].slice(0, LOSS_LIST_LIMIT),
  };
}
