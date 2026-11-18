/**
 * The versions of a file attached to a card, for the viewer's list
 * (lib/file-viewer/versions.svelte.ts): read by who reads the board, written
 * by who edits it. A backend that keeps no versions (a reader by the board's
 * public link) has none, and the viewer shows no list.
 *
 * A file that went up to become a version and did not (somebody else wrote
 * one meanwhile) can be attached to the same card as a file of its own.
 */
import type { VersionsSource, WriteRefusal } from '$lib/file-viewer/versions.svelte.ts';
import { FILE_VERSION_STALE } from '$lib/state/file-versions.ts';
import {
  isBackendError,
  type Attachment,
  type BoardStore,
  type CardStore,
} from '$lib/state/index.ts';

/**
 * Why the server refused a version: the file got a newer one meanwhile, the
 * version to go back to is kept no more, or the file picked cannot be one.
 */
export function versionRefusal(error: unknown): WriteRefusal {
  if (!isBackendError(error)) return 'failed';
  if (error.reason === FILE_VERSION_STALE) return 'stale';
  if (error.status === 404) return 'gone';
  return error.status === 400 ? 'unusable' : 'failed';
}

export function attachmentVersions(
  board: BoardStore,
  card: CardStore,
  attachment: Attachment,
): VersionsSource | undefined {
  const { workspace, boardId } = board;
  const { backend } = workspace;
  const versions = backend.fileVersions;
  if (!versions) return undefined;
  const { fileId } = attachment;
  return {
    get canWrite() {
      return board.canEdit;
    },
    list: () => versions.list(boardId, fileId),
    bytes: (version, signal) => versions.bytes(boardId, fileId, version, signal),
    async upload(file) {
      try {
        return await backend.upload(boardId, file);
      } catch (error) {
        workspace.report('upload', error);
        return undefined;
      }
    },
    add: (baseVersion, upload) => versions.add(boardId, fileId, baseVersion, upload),
    restore: (version, baseVersion) => versions.restore(boardId, fileId, version, baseVersion),
    async attachApart(upload) {
      card.receive(await backend.createAttachment(boardId, card.cardId, upload));
    },
    refusalOf: versionRefusal,
    userName: (userId) => workspace.userName(userId),
    learnUsers: (userIds) => workspace.learnUsers(userIds),
  };
}
