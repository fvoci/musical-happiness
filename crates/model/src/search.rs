//! What a search of the bodies answers with.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::BlockType;

/// The kind of thing a search finds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum SearchKind {
    /// A card of a board.
    Card,
}

impl SearchKind {
    /// The kind of thing a block of this type is to a search; `None` for a block a
    /// search never finds: a view, a comment, an attachment.
    #[must_use]
    pub fn of_block(block_type: &BlockType) -> Option<Self> {
        match block_type {
            BlockType::Card => Some(Self::Card),
            _ => None,
        }
    }

    /// The word of the kind, as the API and the index write it.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Card => "card",
        }
    }
}

impl fmt::Display for SearchKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A word that names no kind of thing a search finds.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0:?} is no kind a search finds")]
pub struct UnknownSearchKind(String);

impl FromStr for SearchKind {
    type Err = UnknownSearchKind;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        match text {
            "card" => Ok(Self::Card),
            other => Err(UnknownSearchKind(other.to_owned())),
        }
    }
}

/// A stretch of a snippet: text as it is in the body, never markup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SnippetPart {
    /// The text of the stretch.
    pub text: String,
    /// Whether the stretch is what the search matched.
    pub matched: bool,
}

/// One thing a search found: the user reads it now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    /// The identifier of the card.
    pub id: String,
    /// What it is.
    pub kind: SearchKind,
    /// The identifier of the board it is on.
    pub container_id: String,
    /// Its title as it is now.
    pub title: String,
    /// A piece of its body around what matched, in stretches that follow one
    /// another; empty where the body has changed since it was indexed, or has nothing
    /// to show.
    pub snippet: Vec<SnippetPart>,
}
