/**
 * The versions of a file attached to a card, for the viewer's list
 * (lib/file-viewer/versions.svelte.ts): read by who reads the board, written
 * by who edits it. A backend that keeps no versions (a reader by the board's
 * public link) has none, and the viewer shows no list.
 *
 * A file that went up to become a version and did not (somebody else wrote
 * one meanwhile) can be attached to the same card as a file of its own. The
 * card's row of the file says the size of the newest version: the server
 * writes it on the attachment, and a version written here is shown at once.
 * Who edits the file holds its lease, through the same versions.
 */
import type {
  FileVersion,
  VersionsSource,
  WriteRefusal,
} from '$lib/file-viewer/versions.svelte.ts';
import { FILE_BEING_EDITED, FILE_VERSION_STALE } from '$lib/state/file-versions.ts';
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
  /** The card's row of the file says the size of the version written here, before it is told. */
  const resized = (written: FileVersion): FileVersion => {
    const row = card.attachments.find((one) => one.id === attachment.id);
    if (row && row.size !== written.size) card.receive({ ...row, size: written.size });
    return written;
  };
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
    add: async (baseVersion, upload) =>
      resized(await versions.add(boardId, fileId, baseVersion, upload)),
    restore: async (version, baseVersion) =>
      resized(await versions.restore(boardId, fileId, version, baseVersion)),
    async attachApart(upload) {
      card.receive(await backend.createAttachment(boardId, card.cardId, upload));
    },
    refusalOf: versionRefusal,
    userName: (userId) => workspace.userName(userId),
    learnUsers: (userIds) => workspace.learnUsers(userIds),
    get me() {
      return workspace.me?.id;
    },
    lease: () => versions.lease(boardId, fileId),
    async holdLease(leaseId) {
      try {
        return await versions.holdLease(boardId, fileId, leaseId);
      } catch (error) {
        if (isBackendError(error) && error.reason === FILE_BEING_EDITED) return 'editing';
        throw error;
      }
    },
    releaseLease: (leaseId, keepalive) =>
      versions.releaseLease(boardId, fileId, leaseId, { keepalive }),
  };
}
