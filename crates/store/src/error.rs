//! The error type of the store.

use fvoci_model::{BlockError, BoardError, BoardsAndBlocksError, CategoryError, SubscriptionError};
use thiserror::Error;

/// An error from a [`Store`](crate::Store) operation.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum StoreError {
    /// A record that the operation needs does not exist.
    #[error("{{{0}}} not found")]
    NotFound(String),

    /// A change would leave a user with more entries of client state than there may be.
    #[error("a user keeps at most {limit} entries of client state")]
    TooManyPreferences {
        /// The most entries a user may keep.
        limit: usize,
    },

    /// A user tried to register without the signup token although the instance has
    /// its first user already.
    #[error("the first user has registered already")]
    NotFirstUser,

    /// A lookup of a single session or sharing record matched no row.
    ///
    /// The message stays in the log: the HTTP layer answers "not found" without it.
    #[error("sql: no rows in result set")]
    NoRows,

    /// An update addressed a user that does not exist, by identifier or by name.
    ///
    /// The HTTP layer answers it as "user not found" (404).
    #[error("user not found ({0})")]
    UserNotFound(String),

    /// Fewer records exist than the operation asked for.
    #[error("not all instances of {{{entity}}} in {{{}}} found", .ids.join(", "))]
    NotAllFound {
        /// The kind of record, such as `block`.
        entity: &'static str,
        /// The identifiers that were asked for.
        ids: Vec<String>,
    },

    /// A block failed validation before it was written.
    #[error("error validating block {id}: {source}")]
    InvalidBlock {
        /// The identifier of the block.
        id: String,
        /// What is wrong with it.
        source: BlockError,
    },

    /// A block operation was given unusable arguments.
    #[error(transparent)]
    Block(#[from] BlockError),

    /// A board failed validation before it was written.
    #[error(transparent)]
    InvalidBoard(#[from] BoardError),

    /// A boards-and-blocks request failed validation.
    #[error(transparent)]
    InvalidBoardsAndBlocks(#[from] BoardsAndBlocksError),

    /// A block listed for deletion belongs to none of the boards listed with it.
    #[error("block {block_id} doesn't belong to any of the boards in the delete request")]
    BlockNotInDeletedBoards {
        /// The identifier of the block.
        block_id: String,
    },

    /// A category failed validation before it was written.
    #[error(transparent)]
    InvalidCategory(#[from] CategoryError),

    /// A subscription failed validation before it was written.
    #[error(transparent)]
    InvalidSubscription(#[from] SubscriptionError),

    /// A statement that writes was given text that is not an identifier where it needs
    /// one. The HTTP layer answers it as a bad request.
    #[error("{0:?} is not an identifier")]
    InvalidId(String),

    /// A place of the file storage given to a record, a mark of writing or an upload
    /// session is not spelled the one way ([`fvoci_model::is_storage_path`]). The
    /// server makes its places; it is the server's error.
    #[error("{0:?} is not a place of the file storage")]
    InvalidPath(String),

    /// A user cannot be stored because another one has what must be one user's alone.
    #[error("{0} is taken")]
    Conflict(Taken),

    /// A block of the trash cannot come back where it was: what it was on has since
    /// become a block it may not be on (a comment is on a card, nothing is below one).
    #[error("block {id} cannot come back where it was: {source}")]
    NotRestorable {
        /// The block at the top of what would come back.
        id: String,
        /// What it would break.
        source: BlockError,
    },

    /// A patch was made against a block as it was before its last change.
    #[error("block {id} changed at {update_at}, after it was read at {read_at}")]
    Stale {
        /// The block.
        id: String,
        /// The `updateAt` the patch was made against.
        read_at: i64,
        /// The block's `updateAt`.
        update_at: i64,
    },

    /// The operation is about one organization and the store's handle names none
    /// ([`Tenant`](crate::Tenant)). A mistake of the caller: the HTTP layer answers it
    /// as a server error.
    #[error("the operation needs an organization, and the store was given none")]
    NoOrganization,

    /// The database configuration cannot be used.
    #[error("invalid database configuration: {0}")]
    Config(String),

    /// The database holds a schema this server cannot use.
    #[error("unsupported database schema: {0}")]
    Schema(String),

    /// A stored value cannot be decoded.
    #[error("corrupt value in column {column}: {reason}")]
    Corrupt {
        /// The column, as named in the query.
        column: &'static str,
        /// Why the value was rejected.
        reason: String,
    },

    /// The database reported an error.
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

/// What of a user is another user's already.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Taken {
    /// The user name, in any letter case.
    Username,
    /// The email address, in any letter case.
    Email,
}

impl std::fmt::Display for Taken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Username => "the user name",
            Self::Email => "the email address",
        })
    }
}

impl StoreError {
    /// Reads the violation of a foreign key as "`what` not found": the row that was to
    /// be stored refers to something that does not exist, or no longer does. Any other
    /// error is returned as it is.
    #[must_use]
    pub(crate) fn missing_reference(self, what: &'static str) -> Self {
        match self {
            Self::Database(error)
                if error
                    .as_database_error()
                    .is_some_and(sqlx::error::DatabaseError::is_foreign_key_violation) =>
            {
                Self::not_found(what)
            }
            other => other,
        }
    }

    /// Returns `true` if the error means a record does not exist, which the HTTP layer
    /// reports as "not found" rather than as a server error.
    pub fn is_not_found(&self) -> bool {
        matches!(
            self,
            Self::NotFound(_) | Self::NotAllFound { .. } | Self::NoRows
        )
    }

    pub(crate) fn not_found(what: impl std::fmt::Display) -> Self {
        Self::NotFound(what.to_string())
    }
}
