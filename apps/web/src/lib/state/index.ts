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
  isSignedOut,
  ssoOrgOf,
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
  type AcceptedInvitation,
  type InvalidInput,
  type LinkedIdentity,
  type InvitationPreview,
  type LoginOptions,
  type LoginOrganization,
  type NewAccount,
  type Registration,
  type SignInOrganization,
  type Sharing,
  type WorkspaceInfo,
  type UploadedFile,
  type User,
  type ViewDraft,
} from './backend.ts';
export type { Registered } from './account-types.ts';
export { BoardStore, type CardDrop } from './board.svelte.ts';
export { type Shareable } from './member-writes.ts';
export { boardPresence } from './board-presence.svelte.ts';
export type { BodyVersion, BodyVersionContent } from './body-versions.ts';
export { CardPreviews } from './card-previews.svelte.ts';
export { type CommentEdit, type Upload } from './card-parts.ts';
export { CardStore } from './card.svelte.ts';
export { PageStore } from './page.svelte.ts';
export { WikiStore, type PageLooks, type WikiLooks } from './wiki.svelte.ts';
export { createWiki } from './wiki-parts.ts';
export { shownWiki } from './shown-wiki.svelte.ts';
export type { PageDraft, PageMove, WikiDraft, WikiPage, WikiPageDetail } from './wiki-types.ts';
export { changeDisplayName } from './account.ts';
export { whileHere } from './here.ts';
export { administers, isGuest, peopleOf, providePeople, type PeopleAccess } from './people.ts';
export type {
  GuestBoardRole,
  Invitation,
  InvitationMail,
  People,
  WorkspaceMember,
  WorkspaceRole,
} from './people-types.ts';
export { newCardMaker, registerNewCard, type NewCardMaker } from './new-card.ts';
export {
  referenceTitles,
  ReferenceTitles,
  type ReferencePlaces,
} from './reference-titles.svelte.ts';
export type { LinkedBlock } from './references.ts';
export { byCreation, byTitle, type SidebarCategory } from './order.ts';
export { type Transfer } from './transfers.svelte.ts';
export { type EnterStatus } from './workspace-entry.ts';
export { boardTrash, cardTrash, pageTrash, Trash, type TrashStatus } from './trash.svelte.ts';
export {
  Workspace,
  type LoadStatus,
  type Problem,
  type ProblemAction,
  type WorkspaceOptions,
} from './workspace.svelte.ts';
