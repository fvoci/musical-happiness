/**
 * The versions the server keeps of a file: the bytes it had before it was
 * written anew. A file is read as its newest version wherever it is shown;
 * the ones before stay to be read and gone back to. The newest are kept
 * (fifty, unless the server is set otherwise): the lowest numbers go as new
 * versions come.
 *
 * A version is written against the one it was made from (`baseVersion`):
 * when the file has a newer one by then, nothing changes and the answer is
 * 409 with the reason `FILE_VERSION_STALE`.
 */
import type { FileVersion } from '../file-viewer/versions.svelte.ts';

import type { UploadedFile } from './backend.ts';

export type { FileVersion };

/** The reason of a 409 for a version written from one that is the newest no more. */
export const FILE_VERSION_STALE = 'file_version_stale';

export interface FileVersions {
  /** The versions of a file of the board or wiki, the newest first, for its readers. */
  list(containerId: string, fileId: string): Promise<FileVersion[]>;
  /** The bytes of one version; 404 for one the file has no more. */
  bytes(containerId: string, fileId: string, version: number, signal?: AbortSignal): Promise<Blob>;
  /**
   * Makes an uploaded file of the same board or wiki the file's new version,
   * for who edits it: the upload is no file of its own after. 409 when the
   * file has a newer version than `baseVersion`; the upload stays a file then.
   */
  add(
    containerId: string,
    fileId: string,
    baseVersion: number,
    upload: UploadedFile,
  ): Promise<FileVersion>;
  /**
   * Goes back to a version, for who edits: its bytes become a new version,
   * and the ones between stay. 409 as for `add`; 404 for a version that is
   * kept no more.
   */
  restore(
    containerId: string,
    fileId: string,
    version: number,
    baseVersion: number,
  ): Promise<FileVersion>;
}

/** What a backend that keeps the versions of files has; a reader by a public link has none. */
export interface VersionsOfFiles {
  readonly fileVersions?: FileVersions;
}
