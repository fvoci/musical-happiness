//! The versions kept of a card's body.

use serde::Serialize;

use crate::body::Body;

/// A version kept of a card's body: the body as it was at a time, and who wrote it up
/// to then.
#[derive(Debug, Clone, PartialEq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BodyVersion {
    /// The place of the version among the card's, counted from 1. The server keeps the
    /// newest ones: the lowest numbers go as new versions come.
    pub version: i64,
    /// When the version was kept, in milliseconds since the Unix epoch. For a body
    /// nobody is known to have written (one from before versions were kept, or from an
    /// import), when the card was last changed.
    pub kept_at: i64,
    /// Who wrote the body up to this version since the one before, in the order of
    /// their identifiers: those who edited it and those who wrote it whole, among them
    /// who restored a version. For a body nobody is known to have written, who changed
    /// the card last.
    pub editors: Vec<String>,
    /// Whether the body of this version is the one the card has now: restoring it
    /// changes nothing.
    pub is_current: bool,
}

/// A version kept of a card's body, with the body.
#[derive(Debug, Clone, PartialEq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BodyVersionContent {
    /// As [`BodyVersion::version`].
    pub version: i64,
    /// As [`BodyVersion::kept_at`].
    pub kept_at: i64,
    /// As [`BodyVersion::editors`].
    pub editors: Vec<String>,
    /// As [`BodyVersion::is_current`].
    pub is_current: bool,
    /// The body of the card then; `null` if it had none.
    pub body: Option<Body>,
}
