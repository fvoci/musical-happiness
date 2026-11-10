//! Wire types shared by the fvoci server and web client.
//!
//! Every type here serializes to the JSON the original server and web app exchange, with
//! the original key spelling (a mix of camelCase and `snake_case`). The crate does no I/O
//! and reads no clock: functions that stamp a time take the current time in milliseconds
//! since the Unix epoch as an argument.

mod archive;
mod auth;
mod block;
mod block_ids;
mod board;
mod boards_and_blocks;
mod card;
mod card_property;
mod category;
mod client_config;
mod error_response;
mod file_info;
mod id;
mod serde_util;
mod settings;
mod sharing;
mod subscription;
mod team;
mod user;

pub use archive::{ArchiveHeader, ArchiveLine};
pub use auth::{
    AuthParamError, ChangePasswordRequest, LoginRequest, MAXIMUM_PASSWORD_BYTES,
    MINIMUM_PASSWORD_LENGTH, RegisterRequest, check_new_password, is_email_valid,
};
pub use block::{
    BLOCK_FIELDS_MAX_CHARS, BLOCK_TITLE_MAX_CHARS, Block, BlockError, BlockPatch, BlockPatchBatch,
    BlockType, ParseBlockTypeError, stamp_modification_metadata,
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
pub use card::{Card, CardError, CardPatch};
pub use card_property::{
    CardPropertyError, validate_block_properties, validate_card_property_definitions,
    validate_card_property_values,
};
pub use category::{
    BoardCategoryWebsocketData, CATEGORY_BOARDS_SORT_ORDER_GAP, Category, CategoryBoardMetadata,
    CategoryBoards, CategoryError, CategoryType,
};
pub use client_config::ClientConfig;
pub use error_response::ErrorBody;
pub use file_info::FileInfo;
pub use id::{is_id, new_id};
pub use settings::{
    CLIENT_STATE_MAX_ENTRIES, CLIENT_STATE_MAX_NAME_LENGTH, CLIENT_STATE_MAX_VALUE_BYTES, CardOpen,
    ClientStateError, Density, HourCycle, SettingName, SettingsError, Theme, TimeZone,
    UserSettings, UserSettingsPatch, WeekStart, check_client_state_name, check_client_state_value,
};
pub use sharing::Sharing;
pub use subscription::{Subscriber, SubscriberType, Subscription, SubscriptionError};
pub use team::Team;
pub use user::{Session, User, UserPreferencesPatch};

/// A JSON object with free-form keys, as used for block fields and board properties.
pub type JsonMap = serde_json::Map<String, serde_json::Value>;
