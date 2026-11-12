//! Wire types shared by the fvoci server and web client.
//!
//! The types here are what the server and its clients exchange as JSON. Most began as
//! the original server's and keep its key spelling (a mix of camelCase and
//! `snake_case`); what this server added or changed says so at the type. The crate does
//! no I/O. Functions that stamp a time take the current time in milliseconds since the
//! Unix epoch as an argument; the one place that reads a clock is [`new_id`], whose
//! identifiers begin with the time they were made.

mod allowlist;
mod archive;
mod auth;
mod block;
mod block_ids;
mod board;
mod boards_and_blocks;
mod body;
mod card;
mod card_property;
mod category;
mod client_config;
mod comment_version;
mod date_value;
mod deleted_item;
mod file_info;
mod id;
mod oidc;
mod organization;
mod problem;
mod refusal;
mod serde_util;
mod settings;
mod sharing;
mod subscription;
mod upload;
mod user;
mod workspace;

pub use archive::{ArchiveHeader, ArchiveLine};
pub use auth::{
    AuthParamError, ChangePasswordRequest, DISPLAY_NAME_MAX_LENGTH, LoginRequest,
    MAXIMUM_PASSWORD_BYTES, MINIMUM_PASSWORD_LENGTH, RegisterRequest, check_new_password,
    display_name, is_email_valid,
};
pub use block::{
    BLOCK_FIELDS_MAX_CHARS, BLOCK_TITLE_MAX_CHARS, Block, BlockError, BlockPatch, BlockPatchBatch,
    BlockType, CARD_FIELDS_MAX_CHARS, ParseBlockTypeError, PropertyChanges, TYPE_SHOWN_CHARS,
    stamp_modification_metadata,
};
pub use block_ids::generate_block_ids;
pub use board::{
    Board, BoardError, BoardMember, BoardPatch, BoardRole, BoardSearchField, BoardType,
    MinimumRole, ParseBoardSearchFieldError,
};
pub use boards_and_blocks::{
    BoardsAndBlocks, BoardsAndBlocksError, DeleteBoardsAndBlocks, PatchBoardsAndBlocks,
    generate_boards_and_blocks_ids, renew_boards_and_blocks_ids,
};
pub use body::{
    BODY_MAX_BYTES, BODY_MAX_DEPTH, BODY_MAX_NODES, BODY_VERSION, Body, BodyChange, BodyError,
    BodySummary, TaskCount,
};
pub use card::{CARD_PREVIEW_TEXT_LENGTH, Card, CardError, CardPatch, CardPreview};
pub use card_property::{
    CardPropertyError, DATE_PROPERTY_TYPE, PersonProperties, clear_values_of_another_kind,
    validate_block_properties, validate_card_property_definitions, validate_card_property_kinds,
    validate_card_property_values,
};
pub use category::{
    BoardCategoryWebsocketData, CATEGORY_BOARDS_SORT_ORDER_GAP, Category, CategoryBoardMetadata,
    CategoryBoards, CategoryError, CategoryType, DEFAULT_CATEGORY_NAME,
};
pub use client_config::ClientConfig;
pub use comment_version::CommentVersion;
pub use date_value::{
    DateValueError, check_date_value, date_property_ids, rewrite_previous_date_values,
};
pub use deleted_item::DeletedItem;
pub use file_info::FileInfo;
pub use id::{is_id, new_id};
pub use oidc::{
    LoginOptions, LoginOrganization, OIDC_CLIENT_FIELD_MAX_BYTES, OIDC_ISSUER_MAX_BYTES,
    OidcProvider, OidcProviderSettings,
};
pub use organization::{OrgRole, UnknownRole, UserWorkspace, WorkspaceRole};
pub use problem::{InvalidInput, PROBLEM_MEDIA_TYPE, Problem, json_pointer, pointer_token};
pub use refusal::{NAME_SHOWN_CHARS, Refusal, RefusalReason, Refusals, shown_name};
pub use settings::{
    CLIENT_STATE_MAX_ENTRIES, CLIENT_STATE_MAX_NAME_LENGTH, CLIENT_STATE_MAX_VALUE_BYTES, CardOpen,
    ClientStateError, Density, HourCycle, Language, ParseSettingError, SettingName, SettingsError,
    Theme, TimeZone, UserSettings, UserSettingsPatch, WeekStart, check_client_state_name,
    check_client_state_value,
};
pub use sharing::Sharing;
pub use subscription::{Subscriber, SubscriberType, Subscription, SubscriptionError};
pub use upload::{MAX_UPLOAD_PARTS, Upload, UploadPart, UploadPurpose};
pub use user::{Session, User, UserPreferencesPatch};
pub use workspace::Workspace;

/// A JSON object with free-form keys, as used for block fields and board properties.
pub type JsonMap = serde_json::Map<String, serde_json::Value>;
