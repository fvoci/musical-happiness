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
  isOwnSpace,
  isPasswordSignInRequired,
  isSignedOut,
  ssoOrgOf,
  type Account,
  type AccountSettings,
  type AccountSettingsPatch,
  type Attachment,
  type Backend,
  type BoardDraft,
  type ContainerRole,
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
  type AccountDeletion,
  type InvalidInput,
  type LeavingOrganization,
  type LinkedIdentity,
  type InvitationPreview,
  type LoginOptions,
  type LoginOrganization,
  type NewAccount,
  type OrgKind,
  type OrgRole,
  type Registration,
  type SignInOrganization,
  type Sharing,
  type UserOrganization,
  type WorkspaceInfo,
  type UploadedFile,
  type User,
  type ViewDraft,
} from './backend.ts';
export { IMPORT_FORMATS, emptyImportCounts } from './import-types.ts';
export type {
  ImportCounts,
  Imported,
  ImportFormat,
  ImportOptions,
  ImportSummary,
  ImportWarning,
  ImportWarningCode,
} from './import-types.ts';
export type { Registered } from './account-types.ts';
export { BoardStore, type CardDrop } from './board.svelte.ts';
export { addViewAtEnd, duplicateView, moveView } from './view-moves.ts';
export { type Shareable } from './member-writes.ts';
export { boardPresence } from './board-presence.svelte.ts';
export type {
  BodyVersion,
  BodyVersionCause,
  BodyVersionContent,
  BodyVersions,
  KeptBodyVersion,
} from './body-versions.ts';
export { CardPreviews } from './card-previews.svelte.ts';
export { type CommentEdit } from './card-parts.ts';
export { CardStore } from './card.svelte.ts';
export { PageStore } from './page.svelte.ts';
export { DiaryStore, firstDiary } from './diary.svelte.ts';
export { EntryStore } from './entry.svelte.ts';
export { DiaryCalendar } from './diary-calendar.svelte.ts';
export { MOODS, NOTE_MAX_CHARS } from './diary-types.ts';
export type {
  DayNotes,
  DiaryDay,
  DiaryEntry,
  DiaryEntryDetail,
  DiaryRun,
  Mood,
} from './diary-types.ts';
export type { PageComments } from './page-comments.svelte.ts';
export { WikiStore, type PageLooks, type WikiLooks } from './wiki.svelte.ts';
export { createWiki } from './wiki-parts.ts';
export { shownWiki } from './shown-wiki.svelte.ts';
export type { PageDraft, PageMove, WikiDraft, WikiPage, WikiPageDetail } from './wiki-types.ts';
export { CourseStore, type CourseLooks } from './course.svelte.ts';
export { CourseScheduleState, type ScheduleWriteResult } from './course-schedule.svelte.ts';
export { LearningTodayStore } from './learning-today.svelte.ts';
export { syncDeviceTimeZone } from './time-zone-sync.ts';
export { UnitStore } from './unit.svelte.ts';
export type { UnitComments } from './unit-comments.svelte.ts';
export { createCourse, createCourseFromTemplate } from './course-parts.ts';
export { shownCourse } from './shown-course.svelte.ts';
export {
  AMOUNT_UNITS,
  courseReviewIntervals,
  DEFAULT_REVIEW_INTERVALS,
  hasControlChars,
  hasDisallowedNoteControlChars,
  MAX_REVIEW_INTERVAL_DAYS,
  MAX_SCHEDULE_SPAN_DAYS,
  MIN_REVIEW_INTERVAL_DAYS,
  STUDY_DAYS_ALL,
  STUDY_DAYS_FRIDAY,
  STUDY_DAYS_MONDAY,
  STUDY_DAYS_SATURDAY,
  STUDY_DAYS_SUNDAY,
  STUDY_DAYS_THURSDAY,
  STUDY_DAYS_TUESDAY,
  STUDY_DAYS_WEDNESDAY,
  STUDY_DAYS_WEEKDAYS,
  STUDY_WEEKDAYS,
  suggestedNextIntervalDays,
  UNIT_AMOUNT_MAX,
  UNIT_AMOUNT_MIN,
  UNIT_NOTE_CONTENT_MAX_CHARS,
  UNIT_NOTE_KINDS,
  UNIT_PROGRESS_POSITION_MAX_CHARS,
  validateUnitNoteContent,
} from './course-types.ts';
export {
  courseReviews,
  CourseReviewsStore,
  todayReviews,
  TodayReviewsStore,
} from './unit-reviews.svelte.ts';
export type {
  AmountUnit,
  CompletedUnitReview,
  CompleteUnitReview,
  CourseDraft,
  CourseProgressSummary,
  CourseReviews,
  CourseSchedule,
  CourseScheduleUpdate,
  CourseScheduleView,
  Courses,
  CourseUnit,
  CourseUnitDetail,
  CreateUnitNote,
  LearningTodayItem,
  ListCourseReviewsQuery,
  MemberCourseProgress,
  PatchUnitNote,
  RedistributeScheduleRequest,
  ReviewStatusFilter,
  ScheduledUnit,
  ScheduleUnitReview,
  StudyWeekdayKey,
  TodayReviewItem,
  UnitDraft,
  UnitKind,
  UnitMove,
  UnitNote,
  UnitNoteContentError,
  UnitNoteKind,
  UnitPatch,
  UnitProgress,
  UnitProgressUpdate,
  UnitReview,
  UnitScheduleUpdate,
  WorkspaceLearningToday,
} from './course-types.ts';
export type { PresenceSource, TreeItem, TreeSource } from './tree-source.ts';
export {
  changeName,
  confirmPassword,
  passwordChanged,
  setFirstPassword,
  rereadEmail,
} from './account.ts';
export type { EmailChange } from './email-change.ts';
export { whileHere } from './here.ts';
export { administers, isGuest, peopleOf, providePeople, type PeopleAccess } from './people.ts';
export type {
  GuestContainerRole,
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
export {
  TRANSFER_STAYS,
  UploadKept,
  type Saying,
  type Transfer,
  type TransferOptions,
  type UploadFailure,
} from './transfers.svelte.ts';
export {
  isOpen,
  leaveOrganization,
  startOrganization,
  type EnterStatus,
} from './workspace-entry.ts';
export {
  containerTrash,
  cardTrash,
  pageTrash,
  unitTrash,
  Trash,
  type TrashStatus,
} from './trash.svelte.ts';
export {
  TimeTracker,
  timeTracker,
  type SwitchedTimerNotice,
  type TimeSessionWall,
} from './time.svelte.ts';
export type {
  CreateTimeEntryRequest,
  ListTimeEntriesQuery,
  StartTimeEntryRequest,
  StartedTimeEntry,
  TimeEntry,
  TimeEntryKind,
  TimeEntryKindTotals,
  TimeEntryList,
  TimeEntryPatch,
  TimeEntrySummary,
  TimeOfBackend,
  TimeTargetKind,
  TimeTracking,
} from './time-types.ts';
export {
  Workspace,
  type LoadStatus,
  type Problem,
  type ProblemAction,
  type WorkspaceOptions,
} from './workspace.svelte.ts';
export { MAX_CACHED_BOARDS, BoardCache } from './board-cache.ts';
export { MAX_CACHED_WIKIS, WikiCache } from './wiki-cache.ts';
export {
  MAX_CACHED_WORKSPACES,
  WorkspaceCache,
  type WorkspaceSnapshot,
} from './workspace-cache.ts';
export type { CacheLimits } from './workspace-caches.ts';
