/**
 * The versions of the files of the backend in memory, as the server keeps
 * them: a file that was never written anew has its upload as its only
 * version; from the second on each is kept, the newest `LIMITS.fileVersions`
 * of them. Read by who reads the board or wiki, written by who edits it.
 */
import type { UploadedFile } from '../backend.ts';
import { FILE_VERSION_STALE, type FileVersion, type FileVersions } from '../file-versions.ts';

import { memberRole } from './container-access.ts';
import { readableFile } from './files.ts';
import { fail } from './refuse.ts';
import { LIMITS } from './shapes.ts';
import type { KeptFileVersion, StoredFile, World } from './world.ts';

const extensionOf = (name: string): string => /\.[^./]+$/.exec(name)?.[0]?.toLowerCase() ?? '';

/** The versions kept of a file, the oldest first: its upload alone until it is written anew. */
function keptOf(file: StoredFile): KeptFileVersion[] {
  return (
    file.versions ?? [
      {
        version: 1,
        createAt: file.createAt ?? 0,
        createdBy: file.createdBy ?? '',
        size: file.size,
        restoredFrom: null,
        ...(file.blob ? { blob: file.blob } : {}),
      },
    ]
  );
}

const told = (kept: KeptFileVersion, newest: number): FileVersion => ({
  version: kept.version,
  createAt: kept.createAt,
  createdBy: kept.createdBy,
  size: kept.size,
  isCurrent: kept.version === newest,
  restoredFrom: kept.restoredFrom,
});

export class MemoryFileVersions implements FileVersions {
  readonly #backend: { readonly world: World; readonly userId: string };
  readonly #enter: () => Promise<void>;

  /** `backend`: whose world and reader, read at each call. */
  constructor(
    backend: { readonly world: World; readonly userId: string },
    enter: () => Promise<void>,
  ) {
    this.#backend = backend;
    this.#enter = enter;
  }

  get #world(): World {
    return this.#backend.world;
  }

  get #userId(): string {
    return this.#backend.userId;
  }

  async list(containerId: string, fileId: string): Promise<FileVersion[]> {
    await this.#enter();
    const kept = keptOf(readableFile(this.#world, this.#userId, containerId, fileId));
    const newest = kept.at(-1)?.version ?? 1;
    return kept.map((one) => told(one, newest)).reverse();
  }

  async bytes(containerId: string, fileId: string, version: number): Promise<Blob> {
    await this.#enter();
    const kept = keptOf(readableFile(this.#world, this.#userId, containerId, fileId));
    const blob = kept.find((one) => one.version === version)?.blob;
    if (!blob) fail(404, 'no such version');
    return blob;
  }

  async add(
    containerId: string,
    fileId: string,
    baseVersion: number,
    upload: UploadedFile,
  ): Promise<FileVersion> {
    await this.#enter();
    const file = this.#held(containerId, fileId, baseVersion);
    const source = this.#world.files.get(upload.fileId);
    const shown = [...this.#world.blocks.values()].some(
      (block) => block.file?.fileId === upload.fileId,
    );
    if (
      source?.containerId !== containerId ||
      source === file ||
      source.versions !== undefined ||
      shown ||
      extensionOf(source.name) !== extensionOf(file.name)
    ) {
      fail(400, 'the uploaded file cannot become a version', { field: 'fileId' });
    }
    this.#world.files.delete(source.fileId);
    file.contentType = source.contentType;
    return this.#write(file, source.size, source.blob, null);
  }

  async restore(
    containerId: string,
    fileId: string,
    version: number,
    baseVersion: number,
  ): Promise<FileVersion> {
    await this.#enter();
    const file = this.#held(containerId, fileId, baseVersion);
    const kept = keptOf(file);
    const source = kept.find((one) => one.version === version);
    if (!source) fail(404, 'no such version');
    const newest = kept.at(-1)?.version ?? 1;
    if (source.version === newest) return told(source, newest);
    return this.#write(file, source.size, source.blob, source.version);
  }

  /** The file, for who may write its versions from the version `baseVersion`. */
  #held(containerId: string, fileId: string, baseVersion: number): StoredFile {
    const role = memberRole(this.#world, this.#userId, containerId, 'any');
    const file = readableFile(this.#world, this.#userId, containerId, fileId);
    if (role !== 'admin' && role !== 'editor') fail(403, 'access denied to make board changes');
    const newest = keptOf(file).at(-1)?.version ?? 1;
    if (newest !== baseVersion) {
      fail(409, `the file's newest version is ${String(newest)}, not ${String(baseVersion)}`, {
        reason: FILE_VERSION_STALE,
      });
    }
    return file;
  }

  #write(
    file: StoredFile,
    size: number,
    blob: Blob | undefined,
    restoredFrom: number | null,
  ): FileVersion {
    const kept = keptOf(file);
    const made: KeptFileVersion = {
      version: (kept.at(-1)?.version ?? 1) + 1,
      createAt: this.#world.now(),
      createdBy: this.#userId,
      size,
      restoredFrom,
      ...(blob ? { blob } : {}),
    };
    file.versions = [...kept, made].slice(-LIMITS.fileVersions);
    file.size = size;
    if (blob) file.blob = blob;
    else delete file.blob;
    return told(made, made.version);
  }
}
