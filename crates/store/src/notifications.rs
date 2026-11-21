//! What people are told inside the application: the list of whom a change of a block
//! may be told to (`notification_outbox`), written in the transaction of that change,
//! and what they were told (`notifications`).
//!
//! A write of a block by a person notes whom it newly names: the users its body
//! mentions that the body before did not, and the users a person property of a card
//! takes that it did not hold ([`note_named`]). A row of the list says who may be
//! told, never that they are: the server that holds the lock works the list off, asks
//! whether that person reads the block now, and only then makes a notification
//! ([`Store::tell_of`]) or drops the row ([`Store::drop_notification_event`]), in
//! one transaction with taking the row off, so that nobody is told twice.
//!
//! A notification holds no title and no name. Whoever lists them is asked again, of
//! each, whether they read what it is about; the list here is narrowed by its reader
//! and by nothing else, across every organization.

use std::collections::BTreeSet;

use fvoci_model::{
    Block, BlockType, ContainerKind, NotificationKind, OrgKind, SearchKind, assigned_users, new_id,
};
use serde_json::Value;

use crate::Store;
use crate::blocks::Paging;
use crate::body_docs;
use crate::containers;
use crate::db::{Id, OptId, Row, Scope, Sql};
use crate::error::StoreError;
use crate::search_docs::select_documents;

/// How many identifiers one statement binds.
const BATCH: usize = 300;

/// The most people one write of a block notes for one kind: a body may hold as many
/// mentions as it has nodes, and each would be a row. Those that come first in the
/// body, or in the properties, are noted.
const MOST_NAMED_BY_A_WRITE: usize = 100;

/// A row of the list, as the worker reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationEvent {
    /// The organization it was noted in.
    pub org_id: String,
    /// Who may be told.
    pub recipient_id: String,
    /// Of what.
    pub kind: NotificationKind,
    /// Where it happened: the card or the page itself, or a comment on it.
    pub source_id: String,
    /// The card or the page it is about.
    pub block_id: String,
    /// The board or the wiki of that.
    pub container_id: String,
    /// Who did it; empty where nobody is known to have.
    pub actor_id: String,
    /// What the row's `seq` was when it was read.
    pub seq: i64,
    /// How many times in a row it could not be worked off.
    pub attempts: i64,
}

/// Who reads their notifications, and with which session: an organization that takes
/// only the sessions made through its provider keeps what it told its people from
/// any other session of theirs, as it keeps its boards
/// (`Store::container_memberships_of_user`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotificationReader<'a> {
    /// The user of the session.
    pub user_id: &'a str,
    /// The provider the session was made through; `None` for a password.
    pub login_provider: Option<&'a str>,
}

impl NotificationReader<'_> {
    /// Binds the reader as the recipient and leaves out the organizations their
    /// session does not open: for a statement on `notifications`, with no other name
    /// for the table, whose text ends at `recipient_id = `.
    fn push_theirs(self, sql: &mut Sql) {
        sql.bind(Id(self.user_id));
        crate::memberships_everywhere::push_organization_opened_by(
            sql,
            "notifications.org_id",
            self.login_provider,
        );
    }
}

/// A notification as it is stored: what it names, and nothing of what that is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredNotification {
    /// Its identifier.
    pub id: String,
    /// The organization of what it is about.
    pub org_id: String,
    /// Of what it tells.
    pub kind: NotificationKind,
    /// Where it happened.
    pub source_id: String,
    /// The card or the page it is about.
    pub block_id: String,
    /// The board or the wiki of that, when it was made.
    pub container_id: String,
    /// Who did it; empty where nobody is known to have.
    pub actor_id: String,
    /// When it was made.
    pub create_at: i64,
    /// When its reader read it; 0 until then.
    pub read_at: i64,
}

/// Where a card or a page a notification is about is now, and what it is called.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationPlace {
    /// The card or the page.
    pub block_id: String,
    /// What it is.
    pub block_kind: SearchKind,
    /// Its title.
    pub title: String,
    /// The board it is on, or the wiki it is in.
    pub container_id: String,
    /// What that container is.
    pub container_kind: ContainerKind,
    /// The workspace of the container.
    pub workspace_id: String,
}

/// The bounds a notification is made within.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotificationLimits {
    /// For how long after a notification the same (to the same person, of the same
    /// kind, from the same place) makes no other, in milliseconds.
    pub same_within_millis: i64,
    /// The most notifications a person has. Past it the oldest go, those that were
    /// read before those that were not.
    pub most_of_a_person: i64,
}

/// Who is told of what a write of a block newly names, and as whose doing.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Telling {
    /// The people it names, as its writer's doing.
    Written,
    /// The people its body newly names, each as the doing of whoever typed the name:
    /// a body written from its document, by the last of several who typed it. The
    /// updates of the document past `since` (the last the block's body held) say who
    /// sent what: the first of them that holds a user's identifier is where that
    /// user was named. A name no update holds any more (they were folded into the
    /// document's state before the body was written) is the writer's doing.
    Typed {
        /// The last update the body of the block held before this write.
        since: i64,
    },
    /// Nobody: a copy, a card or a board made from a template, what comes back from
    /// the trash or from an archive.
    Nobody,
}

impl Telling {
    /// The people a write names, as its writer's doing, unless the scope tells nobody.
    pub(crate) fn of(scope: &Scope<'_>) -> Self {
        if scope.tells_nobody() {
            Self::Nobody
        } else {
            Self::Written
        }
    }

    /// As [`Telling::of`], for a body written from its document.
    pub(crate) fn of_typed(scope: &Scope<'_>, since: i64) -> Self {
        if scope.tells_nobody() {
            Self::Nobody
        } else {
            Self::Typed { since }
        }
    }
}

const EVENT_COLUMNS: &str =
    "org_id, recipient_id, kind, source_id, block_id, container_id, actor_id, seq, attempts";

const NOTIFICATION_COLUMNS: &str =
    "id, org_id, kind, source_id, block_id, container_id, actor_id, create_at, read_at";

impl Store {
    /// Up to `limit` rows of the list that are due at `now`, those due the longest
    /// first, among the rows this handle reaches.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn due_notification_events(
        &self,
        now: i64,
        limit: i64,
    ) -> Result<Vec<NotificationEvent>, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql(&format!(
                "SELECT {EVENT_COLUMNS} FROM notification_outbox WHERE due_at <= "
            ));
            sql.bind(now).push_bind(
                " ORDER BY due_at, org_id, recipient_id, kind, source_id LIMIT ",
                limit,
            );
            scope
                .fetch_all(sql)
                .await?
                .iter()
                .map(event_from_row)
                .collect()
        })
        .await
    }

    /// How many rows the list has, among the rows this handle reaches.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn waiting_notification_events(&self) -> Result<i64, StoreError> {
        self.with_conn(async |scope| {
            let sql = scope.sql("SELECT COUNT(*) FROM notification_outbox");
            match scope.fetch_optional(sql).await? {
                Some(row) => row.int(0),
                None => Ok(0),
            }
        })
        .await
    }

    /// Tells the person of `event`: takes its row off the list and makes the
    /// notification, at `now`, in one transaction. Returns the identifier of the
    /// notification; `None` if none was made: the row was noted again since it was
    /// read (another `seq`) and stays for the next round, or the person was told the
    /// same within [`NotificationLimits::same_within_millis`]. Where the person then
    /// has more than [`NotificationLimits::most_of_a_person`], the oldest go, those
    /// that were read first. For a handle that reaches every organization: a person's
    /// notifications are counted across them.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a statement fails; nothing is taken off or
    /// made then.
    pub async fn tell_of(
        &self,
        event: &NotificationEvent,
        now: i64,
        limits: NotificationLimits,
    ) -> Result<Option<String>, StoreError> {
        self.transaction(async |scope| {
            if !take_off(scope, event).await? {
                return Ok(None);
            }
            let mut sql = scope.sql("SELECT 1 FROM notifications WHERE recipient_id = ");
            sql.bind(Id(&event.recipient_id))
                .push_bind(" AND source_id = ", Id(&event.source_id))
                .push_bind(" AND kind = ", event.kind.as_str())
                .push_bind(
                    " AND create_at > ",
                    now.saturating_sub(limits.same_within_millis),
                )
                .push(" LIMIT 1");
            if scope.fetch_optional(sql).await?.is_some() {
                return Ok(None);
            }
            let id = new_id();
            let mut sql = scope.sql(&format!(
                "INSERT INTO notifications (recipient_id, {NOTIFICATION_COLUMNS}) VALUES ("
            ));
            sql.bind(Id(&event.recipient_id))
                .push_bind(", ", Id(&id))
                .push_bind(", ", Id(&event.org_id))
                .push_bind(", ", event.kind.as_str())
                .push_bind(", ", Id(&event.source_id))
                .push_bind(", ", Id(&event.block_id))
                .push_bind(", ", Id(&event.container_id))
                .push_bind(", ", OptId(&event.actor_id))
                .push_bind(", ", now)
                .push(", 0)");
            scope.execute(sql).await?;
            keep_newest(scope, &event.recipient_id, limits.most_of_a_person).await?;
            Ok(Some(id))
        })
        .await
    }

    /// Takes the row of `event` off the list, telling nobody. Returns whether it was
    /// taken off: a row noted again since it was read (another `seq`) stays.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the statement fails.
    pub async fn drop_notification_event(
        &self,
        event: &NotificationEvent,
    ) -> Result<bool, StoreError> {
        self.with_conn(async |scope| take_off(scope, event).await)
            .await
    }

    /// Notes that a row could not be worked off, and when to try again. A row noted
    /// again since is due already, and left as it is.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the statement fails.
    pub async fn notification_event_failed(
        &self,
        event: &NotificationEvent,
        retry_at: i64,
    ) -> Result<(), StoreError> {
        self.with_conn(async |scope| {
            let mut sql =
                scope.sql("UPDATE notification_outbox SET attempts = attempts + 1, due_at = ");
            sql.bind(retry_at);
            push_event(&mut sql, event);
            scope.execute(sql).await?;
            Ok(())
        })
        .await
    }

    /// Forgets the notifications that were read before `read_before`, among the rows
    /// this handle reaches. Returns how many went.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the statement fails.
    pub async fn forget_read_notifications(&self, read_before: i64) -> Result<u64, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql("DELETE FROM notifications WHERE read_at > 0 AND read_at < ");
            sql.bind(read_before);
            scope.execute(sql).await
        })
        .await
    }

    /// A page of the notifications of a reader, of every organization their session
    /// opens, the newest first. What each is about is named and no more: whether the
    /// user reads it is for the caller to ask, of each.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn notifications_of_user(
        &self,
        reader: NotificationReader<'_>,
        paging: Paging,
    ) -> Result<Vec<StoredNotification>, StoreError> {
        self.system()
            .with_conn(async |scope| {
                let mut sql = scope.sql(&format!(
                    "SELECT {NOTIFICATION_COLUMNS} FROM notifications WHERE recipient_id = "
                ));
                reader.push_theirs(&mut sql);
                sql.push_bind(
                    " ORDER BY create_at DESC, id DESC LIMIT ",
                    i64::from(paging.per_page),
                )
                .push_bind(" OFFSET ", paging.offset());
                scope
                    .fetch_all(sql)
                    .await?
                    .iter()
                    .map(notification_from_row)
                    .collect()
            })
            .await
    }

    /// How many notifications a reader has not read, of every organization their
    /// session opens.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn unread_notifications_of_user(
        &self,
        reader: NotificationReader<'_>,
    ) -> Result<i64, StoreError> {
        self.system()
            .with_conn(async |scope| {
                let mut sql = scope.sql(
                    "SELECT COUNT(*) FROM notifications WHERE read_at = 0 AND recipient_id = ",
                );
                reader.push_theirs(&mut sql);
                match scope.fetch_optional(sql).await? {
                    Some(row) => row.int(0),
                    None => Ok(0),
                }
            })
            .await
    }

    /// Marks a notification of a reader as read at `now`, in whichever organization
    /// their session opens it is; one that was read keeps the time it was. Returns
    /// whether the reader has such a notification: another's is none of theirs, and
    /// neither is one of an organization the session does not open.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a statement fails.
    pub async fn read_notification(
        &self,
        reader: NotificationReader<'_>,
        notification_id: &str,
        now: i64,
    ) -> Result<bool, StoreError> {
        self.system()
            .transaction(async |scope| {
                let mut sql = scope.sql("SELECT read_at FROM notifications WHERE recipient_id = ");
                reader.push_theirs(&mut sql);
                sql.push_bind(" AND id = ", Id(notification_id));
                let Some(row) = scope.fetch_optional(sql).await? else {
                    return Ok(false);
                };
                if row.int(0)? == 0 {
                    let mut sql = scope.sql("UPDATE notifications SET read_at = ");
                    sql.bind(now)
                        .push_bind(" WHERE recipient_id = ", Id(reader.user_id))
                        .push_bind(" AND id = ", Id(notification_id))
                        .push(" AND read_at = 0");
                    scope.execute(sql).await?;
                }
                Ok(true)
            })
            .await
    }

    /// Marks as read at `now` every notification of a reader that was made up to
    /// `made_by` and not read yet, of every organization their session opens.
    /// Returns how many.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the statement fails.
    pub async fn read_notifications_of_user(
        &self,
        reader: NotificationReader<'_>,
        made_by: i64,
        now: i64,
    ) -> Result<u64, StoreError> {
        self.system()
            .with_conn(async |scope| {
                let mut sql = scope.sql("UPDATE notifications SET read_at = ");
                sql.bind(now).push(" WHERE read_at = 0 AND recipient_id = ");
                reader.push_theirs(&mut sql);
                sql.push_bind(" AND create_at <= ", made_by);
                scope.execute(sql).await
            })
            .await
    }

    /// Where those of `block_ids` are now that a notification may be about, among the
    /// rows this handle reaches: the cards and the pages that a search would find
    /// (none of the trash, no template, on no template). What is none of them has no
    /// place.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a query fails.
    pub async fn notification_places(
        &self,
        block_ids: &[&str],
    ) -> Result<Vec<NotificationPlace>, StoreError> {
        let ids: BTreeSet<&str> = block_ids.iter().copied().collect();
        let ids: Vec<&str> = ids.into_iter().collect();
        self.with_conn(async |scope| {
            let mut places = Vec::with_capacity(ids.len());
            for batch in ids.chunks(BATCH) {
                let mut sql = select_documents(
                    scope,
                    "b.id, b.type, b.title, b.container_id, bo.kind, bo.workspace_id",
                );
                sql.push(" AND b.id IN (")
                    .bind_list(batch.iter().map(|id| Id(id)))
                    .push(")");
                for row in scope.fetch_all(sql).await? {
                    places.push(place_from_row(&row)?);
                }
            }
            Ok(places)
        })
        .await
    }

    /// The comments among `comment_ids` that are there now, among the rows this
    /// handle reaches, each with the block it is on. One in the trash is not there.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a query fails.
    pub async fn comments_with_parents(
        &self,
        comment_ids: &[&str],
    ) -> Result<Vec<(String, String)>, StoreError> {
        let ids: BTreeSet<&str> = comment_ids.iter().copied().collect();
        let ids: Vec<&str> = ids.into_iter().collect();
        self.with_conn(async |scope| {
            let mut found = Vec::with_capacity(ids.len());
            for batch in ids.chunks(BATCH) {
                let mut sql = scope.sql("SELECT id, parent_id FROM blocks WHERE type = ");
                sql.bind(BlockType::Comment.as_str())
                    .push(" AND id IN (")
                    .bind_list(batch.iter().map(|id| Id(id)))
                    .push(")");
                for row in scope.fetch_all(sql).await? {
                    found.push((row.id(0)?, row.id(1)?));
                }
            }
            Ok(found)
        })
        .await
    }
}

/// Takes the row of `event` off the list, if it is as it was read.
async fn take_off(scope: &mut Scope<'_>, event: &NotificationEvent) -> Result<bool, StoreError> {
    let mut sql = scope.sql("DELETE FROM notification_outbox");
    push_event(&mut sql, event);
    Ok(scope.execute(sql).await? > 0)
}

/// Narrows a statement to the row `event` was read from, as it was read.
fn push_event(sql: &mut Sql, event: &NotificationEvent) {
    sql.push_bind(" WHERE org_id = ", Id(&event.org_id))
        .push_bind(" AND recipient_id = ", Id(&event.recipient_id))
        .push_bind(" AND kind = ", event.kind.as_str())
        .push_bind(" AND source_id = ", Id(&event.source_id))
        .push_bind(" AND seq = ", event.seq);
}

/// Leaves a person the `most` newest of their notifications: of the others the oldest
/// go, those that were read before those that were not.
async fn keep_newest(scope: &mut Scope<'_>, user_id: &str, most: i64) -> Result<(), StoreError> {
    let mut sql = scope.sql("SELECT COUNT(*) FROM notifications WHERE recipient_id = ");
    sql.bind(Id(user_id));
    let held = match scope.fetch_optional(sql).await? {
        Some(row) => row.int(0)?,
        None => 0,
    };
    let over = held.saturating_sub(most);
    if over <= 0 {
        return Ok(());
    }
    let mut sql = scope.sql("DELETE FROM notifications WHERE recipient_id = ");
    sql.bind(Id(user_id))
        .push_bind(
            " AND id IN (SELECT id FROM notifications WHERE recipient_id = ",
            Id(user_id),
        )
        .push_bind(
            " ORDER BY CASE WHEN read_at = 0 THEN 1 ELSE 0 END, create_at, id LIMIT ",
            over,
        )
        .push(")");
    scope.execute(sql).await?;
    Ok(())
}

fn kind_from_row(row: &Row, index: usize) -> Result<NotificationKind, StoreError> {
    row.text(index)?
        .parse()
        .map_err(
            |error: fvoci_model::UnknownNotificationKind| StoreError::Corrupt {
                column: "kind",
                reason: error.to_string(),
            },
        )
}

fn event_from_row(row: &Row) -> Result<NotificationEvent, StoreError> {
    Ok(NotificationEvent {
        org_id: row.id(0)?,
        recipient_id: row.id(1)?,
        kind: kind_from_row(row, 2)?,
        source_id: row.id(3)?,
        block_id: row.id(4)?,
        container_id: row.id(5)?,
        actor_id: row.id(6)?,
        seq: row.int(7)?,
        attempts: row.int(8)?,
    })
}

fn notification_from_row(row: &Row) -> Result<StoredNotification, StoreError> {
    Ok(StoredNotification {
        id: row.id(0)?,
        org_id: row.id(1)?,
        kind: kind_from_row(row, 2)?,
        source_id: row.id(3)?,
        block_id: row.id(4)?,
        container_id: row.id(5)?,
        actor_id: row.id(6)?,
        create_at: row.int(7)?,
        read_at: row.int(8)?,
    })
}

fn place_from_row(row: &Row) -> Result<NotificationPlace, StoreError> {
    let block_type = BlockType::from(row.text(1)?.as_str());
    let block_kind = SearchKind::of_block(&block_type).ok_or_else(|| StoreError::Corrupt {
        column: "type",
        reason: format!("a block of type {block_type} is no card and no page"),
    })?;
    let container_kind = crate::container_kinds::kind_from_text(&row.text(4)?)?;
    Ok(NotificationPlace {
        block_id: row.id(0)?,
        block_kind,
        title: row.text(2)?,
        container_id: row.id(3)?,
        container_kind,
        workspace_id: row.id(5)?,
    })
}

/// The users a property value of a card held before a write: none for a block that is
/// new.
fn assigned_before<'b>(
    before: Option<&'b Block>,
    definitions: &[fvoci_model::JsonMap],
) -> BTreeSet<&'b str> {
    before
        .and_then(|before| before.fields.get("properties"))
        .and_then(Value::as_object)
        .map(|values| assigned_users(values, definitions).into_iter().collect())
        .unwrap_or_default()
}

/// Notes whom the write of `block` by `writer` newly names, in the transaction of the
/// write: the users its body mentions that the body of `before` did not, and, of a
/// card, the users its person properties take that those of `before` did not hold.
/// `before` is the block as it was stored, `None` for a new one. A comment is about
/// the card or the page it is on; a card that is a template of its board tells
/// nobody, and neither does anything else that is no card, page or comment.
pub(crate) async fn note_named(
    scope: &mut Scope<'_>,
    block: &Block,
    before: Option<&Block>,
    writer: &str,
    telling: Telling,
) -> Result<(), StoreError> {
    if matches!(telling, Telling::Nobody) {
        return Ok(());
    }
    let about = match block.block_type {
        BlockType::Card | BlockType::Page => block.id.as_str(),
        BlockType::Comment => block.parent_id.as_str(),
        _ => return Ok(()),
    };
    if block.fields.get("isTemplate") == Some(&Value::Bool(true)) {
        return Ok(());
    }
    let mentioned_before: BTreeSet<&str> = before
        .and_then(|before| before.body.as_ref())
        .map(|body| body.mention_ids().into_iter().collect())
        .unwrap_or_default();
    let mentioned: Vec<&str> = block
        .body
        .as_ref()
        .map(|body| body.mention_ids())
        .unwrap_or_default()
        .into_iter()
        .filter(|user| !mentioned_before.contains(user))
        .collect();
    // Who typed what is read only for a body that names somebody new.
    let typed = match telling {
        Telling::Typed { since } if !mentioned.is_empty() => {
            body_docs::updates_since(scope, &block.id, since).await?
        }
        _ => Vec::new(),
    };
    let event = Noted {
        block,
        about,
        writer,
        typed: &typed,
    };
    event
        .note(scope, NotificationKind::Mention, &mentioned)
        .await?;

    // The definitions are read only for a card whose values changed: most writes
    // change none.
    let values = block.fields.get("properties").and_then(Value::as_object);
    let values_before = before.and_then(|before| before.fields.get("properties"));
    let (BlockType::Card, Some(values)) = (&block.block_type, values) else {
        return Ok(());
    };
    if values.is_empty() || values_before.and_then(Value::as_object) == Some(values) {
        return Ok(());
    }
    let Some(board) = containers::find_container(scope, &block.container_id).await? else {
        return Ok(());
    };
    let held = assigned_before(before, &board.card_properties);
    let assigned: Vec<&str> = assigned_users(values, &board.card_properties)
        .into_iter()
        .filter(|user| !held.contains(user))
        .collect();
    event
        .note(scope, NotificationKind::Assignment, &assigned)
        .await
}

/// Whether `bytes` hold `word` somewhere. An update of a document holds the text of
/// what it puts in as it is, an identifier that an attribute takes among it.
fn holds(bytes: &[u8], word: &[u8]) -> bool {
    !word.is_empty() && bytes.windows(word.len()).any(|window| window == word)
}

/// A write of a block that names people.
struct Noted<'a> {
    /// The block that was written: where it happened.
    block: &'a Block,
    /// The card or the page it is about.
    about: &'a str,
    writer: &'a str,
    /// The updates of its document that its body did not hold before, the oldest
    /// first, each with who sent it ([`Telling::Typed`]); none for a block that was
    /// written as it is.
    typed: &'a [(String, Vec<u8>)],
}

impl Noted<'_> {
    /// Who named `user`: whoever sent the first update that holds the user's
    /// identifier, or the writer where no update does. `None` where that is the user:
    /// nobody is told of their own doing.
    fn doer_for(&self, user: &str) -> Option<&str> {
        let doer = self
            .typed
            .iter()
            .find(|(_, bytes)| holds(bytes, user.as_bytes()))
            .map_or(self.writer, |(typist, _)| typist.as_str());
        (doer != user).then_some(doer)
    }

    /// Notes that `users` may be told of `kind`: those of them that are accounts (one
    /// that was erased is none), for
    /// a block of an organization people work in together and of no template. In the
    /// space of one account nobody else reads anything, and nothing is noted. A row
    /// that is there already (the same person, kind and place) moves its `seq` on and
    /// is due again now.
    async fn note(
        &self,
        scope: &mut Scope<'_>,
        kind: NotificationKind,
        users: &[&str],
    ) -> Result<(), StoreError> {
        let told: Vec<(&str, &str)> = users
            .iter()
            .filter(|user| fvoci_model::is_id(user))
            .filter_map(|user| Some((*user, self.doer_for(user)?)))
            .take(MOST_NAMED_BY_A_WRITE)
            .collect();
        let doers: BTreeSet<&str> = told.iter().map(|(_, doer)| *doer).collect();
        let now = crate::db::now_millis();
        for doer in doers {
            let users: Vec<&str> = told
                .iter()
                .filter(|(_, by)| *by == doer)
                .map(|(user, _)| *user)
                .collect();
            for batch in users.chunks(BATCH) {
                let mut sql = scope.sql(
                    "INSERT INTO notification_outbox (org_id, recipient_id, kind, source_id, \
                     block_id, container_id, actor_id, seq, due_at) SELECT w.org_id, u.id, ",
                );
                sql.bind(kind.as_str())
                    .push_bind(", ", Id(&self.block.id))
                    .push_bind(", ", Id(self.about))
                    .push_bind(", bo.id, ", OptId(doer))
                    .push_bind(", ", now)
                    .push_bind(", ", now)
                    .push(" FROM containers AS bo JOIN workspaces AS w ON w.id = bo.workspace_id")
                    .push(&scope.same_organization(&[("w", "bo")]))
                    .push(
                        " JOIN organizations AS o ON o.id = w.org_id JOIN users AS u ON u.id IN (",
                    )
                    .bind_list(batch.iter().map(|user| Id(user)))
                    .push_bind(
                        ") WHERE u.erased_at = 0 AND bo.id = ",
                        Id(&self.block.container_id),
                    )
                    .push_bind(" AND bo.is_template = ", false)
                    .push_bind(" AND o.kind = ", OrgKind::Team.as_str())
                    // In one order, whoever notes: two writers that name the same
                    // people wait for one another's rows, and never each for the other's.
                    .push(
                        " ORDER BY u.id ON CONFLICT (org_id, recipient_id, kind, source_id) \
                         DO UPDATE SET seq = CASE WHEN notification_outbox.seq >= excluded.seq \
                         THEN notification_outbox.seq + 1 ELSE excluded.seq END, \
                         due_at = excluded.due_at, attempts = 0, actor_id = excluded.actor_id, \
                         block_id = excluded.block_id, container_id = excluded.container_id",
                    );
                scope.execute(sql).await?;
            }
        }
        Ok(())
    }
}
