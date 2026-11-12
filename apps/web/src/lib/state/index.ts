/**
 * What screens use of the state: the stores, the model, and the errors.
 *
 * The implementations of the backend (`memory/`, later the one that calls
 * the server) are not exported from here. Only `src/app/backend.ts` chooses
 * one; a lint rule keeps every other file from importing them.
 */
export {
  BackendError,
  isBackendError,
  isConflict,
  type Account,
  type AccountSettings,
  type AccountSettingsPatch,
  type Attachment,
  type Backend,
  type BoardDraft,
  type BoardRole,
  type CardDetail,
  type CardDraft,
  type CardPatch,
  type CardSummary,
  type Category,
  type Change,
  type Comment,
  type CommentVersion,
  type DeletedItem,
  type InvalidInput,
  type Registration,
  type Sharing,
  type WorkspaceInfo,
  type UploadedFile,
  type User,
  type ViewDraft,
} from './backend.ts';
export { BoardStore, type CardDrop } from './board.svelte.ts';
export { CardPreviews } from './card-previews.svelte.ts';
export { CardStore, type BodyConflict, type CommentEdit, type Upload } from './card.svelte.ts';
export { changeDisplayName } from './account.ts';
export { newCardMaker, registerNewCard, type NewCardMaker } from './new-card.ts';
export { byCreation, byTitle, type SidebarCategory } from './order.ts';
export { boardTrash, cardTrash, Trash, type TrashStatus } from './trash.svelte.ts';
export {
  Workspace,
  type LoadStatus,
  type Problem,
  type ProblemAction,
  type Transfer,
  type WorkspaceOptions,
} from './workspace.svelte.ts';
