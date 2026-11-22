//! Wire types shared by the fvoci server and web client.
//!
//! The types here are what the server and its clients exchange as JSON. Most began as
//! the original server's and keep its key spelling (a mix of camelCase and
//! `snake_case`); what this server added or changed says so at the type. The crate does
//! no I/O. Functions that stamp a time take the current time in milliseconds since the
//! Unix epoch as an argument; the one place that reads a clock is [`new_id`], whose
//! identifiers begin with the time they were made.

mod account_deletion;
mod allowlist;
mod archive;
mod auth;
mod block;
mod block_ids;
mod body;
mod body_version;
mod card;
mod card_property;
mod category;
mod client_config;
mod comment_version;
mod container;
mod container_teams;
mod containers_and_blocks;
mod date_value;
mod deleted_item;
mod diary;
mod file_info;
mod file_lease;
mod file_version;
mod id;
mod invitation;
mod linked;
mod name;
mod notification;
mod oidc;
mod org_admin;
mod org_kind;
mod organization;
mod page;
mod page_tree;
mod person_name;
mod problem;
mod refusal;
mod relation;
mod search;
mod serde_util;
mod settings;
mod sharing;
mod subscription;
mod team;
mod upload;
mod user;
mod workspace;

pub use account_deletion::{
    ACCOUNT_DELETION_GRACE_DAYS, AccountDeletion, AccountDeletionAsked, DeleteAccountRequest,
    DeletionProof, Leaving, LeavingOrganization, OrganizationName,
};
pub use allowlist::body_node_types;
pub use archive::{ArchiveHeader, ArchiveLine};
pub use auth::{
    AuthParamError, ChangePasswordRequest, ConfirmPasswordRequest, DISPLAY_NAME_MAX_LENGTH,
    LoginRequest, MAXIMUM_EMAIL_BYTES, MAXIMUM_PASSWORD_BYTES, MINIMUM_PASSWORD_LENGTH, NewAccount,
    NewOrganization, ORGANIZATION_NAME_MAX_LENGTH, RegisterRequest, SetPasswordRequest,
    check_new_password, display_name, is_email_valid, organization_name,
};
pub use block::{
    BLOCK_FIELDS_MAX_CHARS, BLOCK_TITLE_MAX_CHARS, Block, BlockError, BlockPatch, BlockPatchBatch,
    BlockType, CARD_FIELDS_MAX_CHARS, ParseBlockTypeError, PropertyChanges,
    stamp_modification_metadata,
};
pub use block_ids::{UnmatchedRelation, generate_block_ids, renew_block_ids};
pub use body::{
    BODY_MAX_BYTES, BODY_MAX_DEPTH, BODY_MAX_NODES, BODY_VERSION, Body, BodyChange, BodyError,
    BodySummary, TaskCount,
};
pub use body_version::{
    BodyVersion, BodyVersionCause, BodyVersionContent, KeptBodyVersion, KeptOutcome,
    ParseBodyVersionCauseError,
};
pub use card::{CARD_PREVIEW_TEXT_LENGTH, Card, CardError, CardPatch, CardPreview};
pub use card_property::{
    CardPropertyError, DATE_PROPERTY_TYPE, PersonProperties, assigned_users,
    clear_values_of_another_kind, validate_block_properties, validate_card_property_definitions,
    validate_card_property_kinds, validate_card_property_values,
};
pub use category::{
    CATEGORY_CONTAINERS_SORT_ORDER_GAP, Category, CategoryContainerMetadata, CategoryContainers,
    CategoryError, CategoryType, ContainerCategoryWebsocketData, DEFAULT_CATEGORY_NAME,
};
pub use client_config::ClientConfig;
pub use comment_version::CommentVersion;
pub use container::{
    Container, ContainerError, ContainerKind, ContainerKinds, ContainerMember, ContainerPatch,
    ContainerRole, ContainerSearchField, ContainerType, MinimumRole,
    ParseContainerSearchFieldError,
};
pub use container_teams::{ContainerTeam, ContainerTeamRole};
pub use containers_and_blocks::{
    ContainersAndBlocks, ContainersAndBlocksError, DeleteContainersAndBlocks,
    PatchContainersAndBlocks, Renewed, generate_containers_and_blocks_ids,
    renew_containers_and_blocks_ids, renew_containers_and_blocks_ids_told,
};
pub use date_value::{
    DateValueError, check_date_value, date_property_ids, day_at, days_between, move_dates,
    rewrite_previous_date_values,
};
pub use deleted_item::DeletedItem;
pub use diary::{
    DAYS_PER_LIST, DIARIES_PER_SPACE, DeletedDiaryEntry, DiaryDate, DiaryDay, DiaryEntry,
    DiaryEntryPatch, DiaryMaking, DiaryRun, ENTRIES_PER_DIARY, ENTRY_DATE_FIELD, ENTRY_DAYS_AHEAD,
    ENTRY_MOOD_FIELD, ENTRY_NOTE_MAX_CHARS, ENTRY_PLACE_FIELD, ENTRY_WEATHER_FIELD, MonthDay, Mood,
    MoodChange, NewDiaryEntry, ParseDiaryDateError, ParseMonthDayError, ParseMoodError, entry_date,
    entry_mood,
};
pub use file_info::{FileInfo, FileKey, is_storage_path};
pub use file_lease::{FileLease, FileLeaseRequest, HeldFileLease};
pub use file_version::{FileVersion, FileVersionBase, NewFileVersion};
pub use id::{is_id, new_id};
pub use invitation::{
    AcceptInvitation, AcceptedInvitation, GUEST_INVITATION_USES, INVITATION_LIFETIME_MILLIS,
    Invitation, InvitationMail, InvitationPreview, InvitationToken, NewInvitation,
};
pub use linked::{LinkedBlock, LinkedKind};
pub use name::{Named, required_name};
pub use notification::{
    Notification, NotificationKind, NotificationSubject, UnknownNotificationKind,
    UnreadNotifications,
};
pub use oidc::{
    LinkedIdentity, LoginOptions, LoginOrganization, OIDC_CLIENT_FIELD_MAX_BYTES,
    OIDC_ISSUER_MAX_BYTES, OidcFormerProvider, OidcFormerProviderEnd, OidcPendingProvider,
    OidcProvider, OidcProviderSettings, OidcStart, OidcStartRequest, UnlinkRequest,
};
pub use org_admin::{
    AuditAction, AuditPerson, AuditRecord, AuditRecordActor, MAX_NAME_CHARS, OrgMember,
    OrgRoleChange, OrgWorkspace, Organization, OrganizationPatch, UnknownAuditAction,
    WorkspaceName,
};
pub use org_kind::{OrgKind, UnknownOrgKind};
pub use organization::{
    OrgRole, UnknownRole, UserWorkspace, WorkspaceMember, WorkspaceRole, WorkspaceRoleChange,
};
pub use page::{
    DeletedPage, NewPage, OrderError, PAGE_ICON_FIELD, PAGE_ORDER_FIELD, Page, PageDuplicate,
    PageMove, PageNode, PagePatch, order_between, sort_pages,
};
pub use page_tree::{DEFAULT_PAGE_DEPTH, DEFAULT_WIKI_PAGES, PageLimits, PageTree, Place};
pub use person_name::{
    Credential, MAX_CREDENTIALS, NAME_PART_MAX_LENGTH, NameShown, PersonName, PersonNameChange,
    Title, UnknownCode,
};
pub use problem::{InvalidInput, PROBLEM_MEDIA_TYPE, Problem, json_pointer, pointer_token};
pub use refusal::{NAME_SHOWN_CHARS, Refusal, RefusalReason, Refusals, shown_name};
pub use relation::{
    RELATION_LIMIT, RELATION_PROPERTY_TYPE, RelationError, RelationProblem, RelationProperties,
    check_relation_value, relation_property_ids, tidy_copied_relations,
};
pub use search::{SearchHit, SearchKind, SnippetPart, UnknownSearchKind};
pub use settings::{
    CLIENT_STATE_MAX_ENTRIES, CLIENT_STATE_MAX_NAME_LENGTH, CLIENT_STATE_MAX_VALUE_BYTES, CardOpen,
    ClientStateError, Density, HourCycle, Language, ParseSettingError, SettingName, SettingsError,
    Theme, TimeZone, UserSettings, UserSettingsPatch, WeekStart, check_client_state_name,
    check_client_state_value,
};
pub use sharing::Sharing;
pub use subscription::{Subscriber, SubscriberType, Subscription, SubscriptionError};
pub use team::{Team, TeamMember, TeamName, team_name, team_name_key};
pub use upload::{MAX_UPLOAD_PARTS, Upload, UploadPart, UploadPurpose};
pub use user::{LoginOrigin, Me, Session, User, UserPreferencesPatch};
pub use workspace::{GuestInvitationsChange, Workspace};

/// A JSON object with free-form keys, as used for block fields and board properties.
pub type JsonMap = serde_json::Map<String, serde_json::Value>;
