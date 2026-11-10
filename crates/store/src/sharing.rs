//! Public sharing of boards through read tokens.
//!
//! A board that is shared has a read token; whoever has the token reads the board
//! without signing in. The token is the server's: made when sharing is turned on,
//! forgotten when it is turned off, so that a link that was closed does not open again.

use fvoci_model::Sharing;

use crate::Store;
use crate::boards;
use crate::db::{Id, OptId, Scope, now_millis};
use crate::error::StoreError;

/// What to do about the sharing of a board.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SharingChange {
    /// Share the board. A board that is shared already keeps its token.
    On,
    /// Share the board under a new token: the old link stops working.
    OnWithNewToken,
    /// Stop sharing the board and forget its token.
    Off,
}

impl Store {
    /// Changes whether a board is shared and returns the sharing state as stored.
    ///
    /// `fresh_token` is the token to share the board under if it needs a new one: when
    /// it was not shared, and with [`SharingChange::OnWithNewToken`]. The caller makes
    /// it, from a source of secrets; the store decides whether it is used.
    ///
    /// Reading the state and writing the new one are one step: of two requests that
    /// share a board at once, both are answered the same token.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if there is no such board.
    pub async fn set_sharing(
        &self,
        board_id: &str,
        change: SharingChange,
        fresh_token: &str,
        user_id: &str,
    ) -> Result<Sharing, StoreError> {
        self.transaction(async |scope| {
            if !boards::lock_board(scope, board_id).await? {
                return Err(StoreError::not_found(format_args!("board ID={board_id}")));
            }
            // The token a shared board has, which it keeps unless a new one is asked for.
            let kept = find_sharing(scope, board_id)
                .await?
                .filter(|current| current.enabled && !current.token.is_empty())
                .map(|current| current.token);
            let token = match (change, kept) {
                (SharingChange::Off, _) => String::new(),
                (SharingChange::On, Some(token)) => token,
                (SharingChange::On | SharingChange::OnWithNewToken, _) => fresh_token.to_owned(),
            };
            let sharing = Sharing {
                board_id: board_id.to_owned(),
                enabled: change != SharingChange::Off,
                token,
                modified_by: user_id.to_owned(),
                update_at: now_millis(),
            };
            upsert_sharing(scope, &sharing).await?;
            Ok(sharing)
        })
        .await
    }

    /// Returns the sharing state of a board.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NoRows`] if the board was never shared.
    pub async fn get_sharing(&self, board_id: &str) -> Result<Sharing, StoreError> {
        self.with_conn(async |scope| {
            find_sharing(scope, board_id)
                .await?
                .ok_or(StoreError::NoRows)
        })
        .await
    }
}

async fn upsert_sharing(scope: &mut Scope<'_>, sharing: &Sharing) -> Result<(), StoreError> {
    let mut sql = scope
        .sql("INSERT INTO sharing (board_id, enabled, token, modified_by, update_at) VALUES (");
    sql.bind(Id(&sharing.board_id))
        .push_bind(", ", sharing.enabled);
    // No token is no value: the column is unique among the tokens there are.
    if sharing.token.is_empty() {
        sql.push(", NULL");
    } else {
        sql.push_bind(", ", &sharing.token);
    }
    sql.push_bind(", ", OptId(&sharing.modified_by))
        .push_bind(", ", sharing.update_at)
        .push(
            ") ON CONFLICT (board_id) DO UPDATE SET enabled = EXCLUDED.enabled, \
             token = EXCLUDED.token, modified_by = EXCLUDED.modified_by, \
             update_at = EXCLUDED.update_at",
        );
    scope.execute(sql).await?;
    Ok(())
}

async fn find_sharing(
    scope: &mut Scope<'_>,
    board_id: &str,
) -> Result<Option<Sharing>, StoreError> {
    let mut sql = scope.sql(
        "SELECT board_id, enabled, token, modified_by, update_at FROM sharing \
         WHERE board_id = ",
    );
    sql.bind(Id(board_id));
    let Some(row) = scope.fetch_optional(sql).await? else {
        return Ok(None);
    };
    Ok(Some(Sharing {
        board_id: row.id(0)?,
        enabled: row.bool(1)?,
        token: row.text(2)?,
        modified_by: row.id(3)?,
        update_at: row.int(4)?,
    }))
}
