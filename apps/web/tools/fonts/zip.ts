/** Reads one member of a zip archive. Enough for the Unihan archive; no zip64, no encryption. */
import { inflateRawSync } from 'node:zlib';

export function readZipMember(archive: Buffer, memberName: string): Buffer {
  const endSignature = archive.lastIndexOf(Buffer.from([0x50, 0x4b, 0x05, 0x06]));
  if (endSignature === -1) throw new Error('not a zip archive');
  const entries = archive.readUInt16LE(endSignature + 10);
  let at = archive.readUInt32LE(endSignature + 16);
  for (let index = 0; index < entries; index += 1) {
    if (archive.readUInt32LE(at) !== 0x02014b50) throw new Error('corrupt zip directory');
    const method = archive.readUInt16LE(at + 10);
    const compressedSize = archive.readUInt32LE(at + 20);
    const nameLength = archive.readUInt16LE(at + 28);
    const extraLength = archive.readUInt16LE(at + 30);
    const commentLength = archive.readUInt16LE(at + 32);
    const localHeader = archive.readUInt32LE(at + 42);
    const name = archive.toString('utf8', at + 46, at + 46 + nameLength);
    if (name === memberName) {
      const dataStart =
        localHeader +
        30 +
        archive.readUInt16LE(localHeader + 26) +
        archive.readUInt16LE(localHeader + 28);
      const data = archive.subarray(dataStart, dataStart + compressedSize);
      if (method === 0) return Buffer.from(data);
      if (method === 8) return inflateRawSync(data);
      throw new Error(`${memberName}: unsupported zip compression method ${method}`);
    }
    at += 46 + nameLength + extraLength + commentLength;
  }
  throw new Error(`${memberName} is not in the archive`);
}
