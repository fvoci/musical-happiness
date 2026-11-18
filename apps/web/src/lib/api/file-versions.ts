/**
 * The versions of a file on the server (`getFileVersions`, `getFileVersion`,
 * `addFileVersion`, `restoreFileVersion`), under the workspace of the file's
 * board or wiki.
 */
import type { UploadedFile } from '../state/backend.ts';
import type { FileVersion, FileVersions } from '../state/file-versions.ts';

import type { Schemas } from './convert.ts';
import type { RequestOptions } from './http.ts';
import type { WorkspacePath } from './requests.ts';

/** A request under the workspace of the board or wiki that `params.containerId` names. */
type Ask = <T>(
  path: WorkspacePath,
  params: Record<string, string>,
  options?: RequestOptions,
) => Promise<T>;

/** The bytes at a path under that workspace. */
type AskBytes = (
  path: WorkspacePath,
  params: Record<string, string>,
  signal?: AbortSignal,
) => Promise<Blob>;

export class ServerFileVersions implements FileVersions {
  readonly #ask: Ask;
  readonly #bytes: AskBytes;

  constructor(ask: Ask, bytes: AskBytes) {
    this.#ask = ask;
    this.#bytes = bytes;
  }

  list(containerId: string, fileId: string): Promise<FileVersion[]> {
    return this.#ask<Schemas['FileVersion'][]>(
      '/containers/{containerId}/files/{fileId}/versions',
      {
        containerId,
        fileId,
      },
    );
  }

  bytes(containerId: string, fileId: string, version: number, signal?: AbortSignal): Promise<Blob> {
    return this.#bytes(
      '/containers/{containerId}/files/{fileId}/versions/{version}',
      { containerId, fileId, version: String(version) },
      signal,
    );
  }

  add(
    containerId: string,
    fileId: string,
    baseVersion: number,
    upload: UploadedFile,
  ): Promise<FileVersion> {
    const json: Schemas['NewFileVersion'] = { baseVersion, fileId: upload.fileId };
    return this.#ask<Schemas['FileVersion']>(
      '/containers/{containerId}/files/{fileId}/versions',
      { containerId, fileId },
      { method: 'POST', json },
    );
  }

  restore(
    containerId: string,
    fileId: string,
    version: number,
    baseVersion: number,
  ): Promise<FileVersion> {
    const json: Schemas['FileVersionBase'] = { baseVersion };
    return this.#ask<Schemas['FileVersion']>(
      '/containers/{containerId}/files/{fileId}/versions/{version}/restore',
      { containerId, fileId, version: String(version) },
      { method: 'POST', json },
    );
  }
}
