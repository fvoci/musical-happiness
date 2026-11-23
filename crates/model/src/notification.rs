//! What a person is told inside the application: that somebody mentioned them, gave
//! them a card, or commented on what they watch.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::{ContainerKind, SearchKind};

/// What a notification tells of.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    utoipa::ToSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum NotificationKind {
    /// A body, of a card, a page, a unit or a comment, names the person.
    Mention,
    /// A person property of a card took the person.
    Assignment,
    /// A card, a page or a unit the person watches has a new comment.
    Comment,
    /// A scheduled personal review of a course unit is due today.
    ReviewDue,
}

impl NotificationKind {
    /// The word of the kind, as the API and the database write it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Mention => "mention",
            Self::Assignment => "assignment",
            Self::Comment => "comment",
            Self::ReviewDue => "review_due",
        }
    }
}

impl fmt::Display for NotificationKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A word that is no kind of notification.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown kind of notification {0:?}")]
pub struct UnknownNotificationKind(pub String);

impl FromStr for NotificationKind {
    type Err = UnknownNotificationKind;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        match text {
            "mention" => Ok(Self::Mention),
            "assignment" => Ok(Self::Assignment),
            "comment" => Ok(Self::Comment),
            "review_due" => Ok(Self::ReviewDue),
            other => Err(UnknownNotificationKind(other.to_owned())),
        }
    }
}

/// What a notification is about, for a reader who reads it now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NotificationSubject {
    /// What happened.
    pub kind: NotificationKind,
    /// Who did the latest of it; empty where nobody is known to have. The identifier
    /// alone, as a card says who wrote it: the name is asked of the workspace's
    /// people, where its reader is told the names they may see.
    pub actor_id: String,
    /// How many times it happened: what happens to the same card or page, of the same
    /// kind, within an hour of the first of it, is told in one notification.
    pub times: i64,
    /// How many people did it, `actorId` among them; 0 where nobody is known to have.
    /// A hundred are counted at most.
    pub people: i64,
    /// The workspace of the card or the page.
    pub workspace_id: String,
    /// The board the card is on, or the wiki the page is in.
    pub container_id: String,
    /// What that container is: a board, or a wiki.
    pub container_kind: ContainerKind,
    /// The card or the page it is about: what the reader opens.
    pub block_id: String,
    /// What that is.
    pub block_kind: SearchKind,
    /// Its title as it is now.
    pub title: String,
    /// The comment the latest of it happened in, where that is still there; empty
    /// where it happened in the card or the page itself, or the comment went.
    pub comment_id: String,
}

/// One thing a person was told.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Notification {
    /// Its identifier.
    pub id: String,
    /// When it was made: when the first of it happened.
    pub create_at: i64,
    /// When the latest of it happened; the notifications are listed by it, the
    /// newest first.
    pub update_at: i64,
    /// When its reader read it; 0 until then, and again once more of it happens.
    pub read_at: i64,
    /// What it is about; `null` where its reader does not read that now (it is gone,
    /// or theirs no more), and is told nothing of it: not what, not where, not by
    /// whom.
    pub subject: Option<NotificationSubject>,
}

/// How many notifications a person has not read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UnreadNotifications {
    /// How many.
    pub count: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_reads_back_from_its_word() {
        for kind in [
            NotificationKind::Mention,
            NotificationKind::Assignment,
            NotificationKind::Comment,
            NotificationKind::ReviewDue,
        ] {
            assert_eq!(kind.as_str().parse(), Ok(kind));
            assert_eq!(
                serde_json::to_value(kind).expect("JSON"),
                serde_json::json!(kind.as_str())
            );
        }
        assert!("mentions".parse::<NotificationKind>().is_err());
    }
}
