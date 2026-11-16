//! Boards and board membership.

use std::collections::{HashMap, HashSet};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::JsonMap;
use crate::allowlist::holds_nul;
use crate::card_property::{
    CardPropertyError, PersonProperties, validate_card_property_definitions,
};
use crate::serde_util::null_as_default;

/// Who may see and join a board.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, utoipa::ToSchema,
)]
#[schema(as = BoardType)]
pub enum ContainerType {
    /// Any member of the workspace may find and join the board.
    #[default]
    #[serde(rename = "O")]
    Open,
    /// Only invited users may access the board.
    #[serde(rename = "P")]
    Private,
}

impl ContainerType {
    /// Returns the one-letter wire and database form.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "O",
            Self::Private => "P",
        }
    }
}

/// What a container of a workspace is for: a board holds cards and their views, a wiki
/// holds pages. A container is of one kind from when it is made and never of another.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, utoipa::ToSchema,
)]
#[serde(rename_all = "lowercase")]
#[schema(as = BoardKind)]
pub enum ContainerKind {
    /// A board: cards, their views and their comments.
    #[default]
    Board,
    /// A wiki: a tree of pages.
    Wiki,
}

impl ContainerKind {
    /// Every kind there is.
    pub const ALL: [Self; 2] = [Self::Board, Self::Wiki];

    /// Returns the wire and database form.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Board => "board",
            Self::Wiki => "wiki",
        }
    }
}

impl std::fmt::Display for ContainerKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Error returned when a string names no kind of board.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{0:?} is no kind of board")]
pub struct ParseContainerKindError(String);

impl FromStr for ContainerKind {
    type Err = ParseContainerKindError;

    /// Reads the wire and database form ([`ContainerKind::as_str`]), exactly.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.as_str() == text)
            .ok_or_else(|| ParseContainerKindError(text.to_owned()))
    }
}

/// The kinds of container something reaches: what an operation is for, and what a list
/// holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContainerKinds {
    /// Boards and no wiki.
    Boards,
    /// Wikis and no board.
    Wikis,
    /// Boards and wikis alike.
    Every,
}

impl ContainerKinds {
    /// Whether a container of `kind` is among them.
    #[must_use]
    pub const fn holds(self, kind: ContainerKind) -> bool {
        match (self, kind) {
            (Self::Every, _)
            | (Self::Boards, ContainerKind::Board)
            | (Self::Wikis, ContainerKind::Wiki) => true,
            (Self::Boards, ContainerKind::Wiki) | (Self::Wikis, ContainerKind::Board) => false,
        }
    }

    /// The one kind they are, if they are one.
    #[must_use]
    pub const fn only(self) -> Option<ContainerKind> {
        match self {
            Self::Boards => Some(ContainerKind::Board),
            Self::Wikis => Some(ContainerKind::Wiki),
            Self::Every => None,
        }
    }
}

/// The kinds that are `kind` alone: what reaches a board reaches no wiki.
impl From<ContainerKind> for ContainerKinds {
    fn from(kind: ContainerKind) -> Self {
        match kind {
            ContainerKind::Board => Self::Boards,
            ContainerKind::Wiki => Self::Wikis,
        }
    }
}

/// A role on a board, from least to most privileged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ContainerRole {
    /// May view the board.
    #[serde(rename = "viewer")]
    Viewer,
    /// May also comment on cards.
    #[serde(rename = "commenter")]
    Commenter,
    /// May also edit cards and properties.
    #[serde(rename = "editor")]
    Editor,
    /// May also manage the board, its members and its sharing.
    #[serde(rename = "admin")]
    Admin,
}

impl ContainerRole {
    /// Returns the wire form.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Viewer => "viewer",
            Self::Commenter => "commenter",
            Self::Editor => "editor",
            Self::Admin => "admin",
        }
    }
}

impl std::str::FromStr for ContainerRole {
    type Err = crate::organization::UnknownRole;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        [Self::Viewer, Self::Commenter, Self::Editor, Self::Admin]
            .into_iter()
            .find(|role| role.as_str() == text)
            .ok_or_else(|| crate::organization::UnknownRole(text.to_owned()))
    }
}

/// The minimum role of a board: the role a user gets who joins the board, or is added
/// to it by somebody who does not manage its roles.
///
/// A board always has one, and it is never admin: nobody becomes an administrator of a
/// board by joining it.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, utoipa::ToSchema,
)]
pub enum MinimumRole {
    /// Whoever joins may view the board.
    #[serde(rename = "viewer")]
    Viewer,
    /// Whoever joins may also comment on cards.
    #[serde(rename = "commenter")]
    Commenter,
    /// Whoever joins may also edit cards and properties. What a new board starts with.
    #[default]
    #[serde(rename = "editor")]
    Editor,
}

impl MinimumRole {
    /// Every minimum role there is.
    pub const ALL: [Self; 3] = [Self::Viewer, Self::Commenter, Self::Editor];

    /// Returns the wire and database form.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Viewer => "viewer",
            Self::Commenter => "commenter",
            Self::Editor => "editor",
        }
    }
}

impl From<MinimumRole> for ContainerRole {
    fn from(role: MinimumRole) -> Self {
        match role {
            MinimumRole::Viewer => Self::Viewer,
            MinimumRole::Commenter => Self::Commenter,
            MinimumRole::Editor => Self::Editor,
        }
    }
}

/// Reasons a [`Container`], or a change of one, is rejected before it is stored.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ContainerError {
    /// The board names no workspace.
    #[error("empty-workspace-id")]
    EmptyWorkspaceId,
    /// A card property definition of the board is malformed.
    #[error(transparent)]
    InvalidCardProperty(#[from] CardPropertyError),
    /// A member of the board holds U+0000, which PostgreSQL cannot store in text or
    /// `jsonb`.
    #[error("the {member} of the board holds the character U+0000")]
    NulCharacter {
        /// The member, as the API names it.
        member: &'static str,
    },
    /// A new order of the card properties names a property twice.
    #[error("the order names each property of the board once: {id:?} is named twice")]
    PropertyNamedTwice {
        /// The property's identifier.
        id: String,
    },
    /// A new order of the card properties names a property the board does not have.
    #[error("the order names the properties of the board: it has no {id:?}")]
    PropertyUnknown {
        /// The identifier the order named.
        id: String,
    },
    /// A new order of the card properties leaves some of the board's properties out.
    #[error("the order names every property of the board: {named} of its {total} are named")]
    PropertiesLeftOut {
        /// How many the order names.
        named: usize,
        /// How many the board has.
        total: usize,
    },
    /// The board is stored as of another kind than it was made as.
    #[error("a {stored} does not become a {asked}: the kind of a board never changes")]
    KindChanges {
        /// The kind it is.
        stored: ContainerKind,
        /// The kind it was to become.
        asked: ContainerKind,
    },
    /// A wiki is stored as a template.
    #[error("a wiki is no template")]
    WikiIsTemplate,
    /// A wiki is stored with card properties.
    #[error("a wiki has no card properties: it holds pages and no cards")]
    WikiHasCardProperties,
}

/// A board: a set of blocks and the property definitions of its cards.
// `type` is required when decoding; every other member defaults to its empty value.
// (No doc comment: the doc comment is the description in the API contract.)
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = Board)]
pub struct Container {
    /// The board's identifier.
    #[serde(default)]
    #[serde(deserialize_with = "null_as_default")]
    pub id: String,
    /// The identifier of the workspace the board belongs to.
    #[serde(default)]
    #[serde(deserialize_with = "null_as_default")]
    pub workspace_id: String,
    /// The identifier of the user who created the board; empty for a built-in template,
    /// which no user made.
    #[serde(default)]
    #[serde(deserialize_with = "null_as_default")]
    pub created_by: String,
    /// The identifier of the user who last modified the board.
    #[serde(default)]
    #[serde(deserialize_with = "null_as_default")]
    pub modified_by: String,
    /// Whether the board is open or private.
    #[serde(rename = "type")]
    pub container_type: ContainerType,
    /// What the board is for: `board` for cards, `wiki` for pages. It is what the board
    /// was made as and never changes; a board decoded without it is a `board`. A wiki
    /// is no template and has no card properties, and the operations that are about
    /// cards answer for a wiki as for a board that is not there.
    #[serde(default)]
    #[serde(deserialize_with = "null_as_default")]
    pub kind: ContainerKind,
    /// The role a user gets who joins the board, or is added by somebody who does not
    /// manage its roles. Never empty and never admin.
    #[serde(default)]
    #[serde(deserialize_with = "null_as_default")]
    pub minimum_role: MinimumRole,
    /// The board's title. No text of a board (this, `description`, `icon`, and the
    /// strings and member names of `properties` and `cardProperties`) holds U+0000:
    /// 400 naming the member.
    #[serde(default)]
    #[serde(deserialize_with = "null_as_default")]
    pub title: String,
    /// The board's description.
    #[serde(default)]
    #[serde(deserialize_with = "null_as_default")]
    pub description: String,
    /// The board's icon, usually one emoji.
    #[serde(default)]
    #[serde(deserialize_with = "null_as_default")]
    pub icon: String,
    /// Whether the description is shown in the interface.
    #[serde(default)]
    #[serde(deserialize_with = "null_as_default")]
    pub show_description: bool,
    /// Whether the board is a template.
    #[serde(default)]
    #[serde(deserialize_with = "null_as_default")]
    pub is_template: bool,
    /// Whether the board is one of the templates the server ships with. They are
    /// offered to every workspace and nobody can change them. The server decides this:
    /// what a client sends here is ignored.
    #[serde(default)]
    #[serde(deserialize_with = "null_as_default")]
    pub is_builtin: bool,
    /// The version of the built-in template this board came from.
    #[serde(default)]
    #[serde(deserialize_with = "null_as_default")]
    pub template_version: i64,
    /// Free-form properties of the board itself.
    #[serde(default, deserialize_with = "null_as_default")]
    #[schema(value_type = Object)]
    pub properties: JsonMap,
    /// The property definitions of the board's cards, in display order.
    #[serde(default, deserialize_with = "null_as_default")]
    #[schema(value_type = Vec<Object>)]
    pub card_properties: Vec<JsonMap>,
    /// Creation time in milliseconds since the Unix epoch.
    #[serde(default)]
    #[serde(deserialize_with = "null_as_default")]
    pub create_at: i64,
    /// Last modification time in milliseconds since the Unix epoch.
    #[serde(default)]
    #[serde(deserialize_with = "null_as_default")]
    pub update_at: i64,
    /// Deletion time in milliseconds since the Unix epoch; zero if not deleted.
    #[serde(default)]
    #[serde(deserialize_with = "null_as_default")]
    pub delete_at: i64,
}

impl Container {
    /// Puts the card properties into the order of their identifiers in `order`, which
    /// must name each of them exactly once.
    ///
    /// # Errors
    ///
    /// Returns [`ContainerError::PropertyNamedTwice`], [`ContainerError::PropertyUnknown`] or
    /// [`ContainerError::PropertiesLeftOut`], and changes nothing, if `order` names one twice,
    /// names one the board does not have, or leaves one out.
    pub fn reorder_card_properties(&mut self, order: &[String]) -> Result<(), ContainerError> {
        let known: HashSet<String> = self
            .card_properties
            .iter()
            .filter_map(property_id)
            .collect();
        let mut rank = HashMap::with_capacity(order.len());
        for (at, id) in order.iter().enumerate() {
            if !known.contains(id) {
                return Err(ContainerError::PropertyUnknown { id: id.clone() });
            }
            if rank.insert(id.as_str(), at).is_some() {
                return Err(ContainerError::PropertyNamedTwice { id: id.clone() });
            }
        }
        // Each named once and each known: the order is the board's when it names as many
        // as the board has (a stored board's identifiers are its own and distinct).
        if rank.len() != self.card_properties.len() {
            return Err(ContainerError::PropertiesLeftOut {
                named: rank.len(),
                total: self.card_properties.len(),
            });
        }
        self.card_properties.sort_by_key(|property| {
            property_id(property).and_then(|id| rank.get(id.as_str()).copied())
        });
        Ok(())
    }

    /// The board as it goes to whoever reads it by its link, signed in or not: no
    /// `createdBy` or `modifiedBy`
    /// (`docs/s6-design.md`, decision 7). The blocks go so by [`Block::hide_people`].
    ///
    /// [`Block::hide_people`]: crate::Block::hide_people
    pub fn hide_people(&mut self) {
        self.created_by.clear();
        self.modified_by.clear();
    }

    /// The properties of the board's cards whose values name users.
    #[must_use]
    pub fn person_properties(&self) -> PersonProperties {
        PersonProperties::of(&self.card_properties)
    }

    /// Checks that the board may be stored.
    ///
    /// # Errors
    ///
    /// Returns [`ContainerError::EmptyWorkspaceId`] if the board names no workspace, or
    /// [`ContainerError::InvalidCardProperty`] if a card property definition is malformed.
    pub fn validate(&self) -> Result<(), ContainerError> {
        if self.workspace_id.is_empty() {
            return Err(ContainerError::EmptyWorkspaceId);
        }
        self.validate_card_properties()?;
        Ok(())
    }

    /// Checks what every board that is stored must be, wherever it came from: its card
    /// property definitions are well-formed, none of its texts (`title`,
    /// `description`, `icon`, and the strings and member names of `properties` and
    /// `cardProperties`) holds U+0000, and a wiki is no template and has no card
    /// properties.
    ///
    /// # Errors
    ///
    /// Returns [`ContainerError::InvalidCardProperty`], [`ContainerError::NulCharacter`],
    /// [`ContainerError::WikiIsTemplate`] or [`ContainerError::WikiHasCardProperties`].
    pub fn check_storable(&self) -> Result<(), ContainerError> {
        self.validate_card_properties()?;
        if self.kind == ContainerKind::Wiki {
            if self.is_template {
                return Err(ContainerError::WikiIsTemplate);
            }
            if !self.card_properties.is_empty() {
                return Err(ContainerError::WikiHasCardProperties);
            }
        }
        let texts = [
            ("title", self.title.contains('\0')),
            ("description", self.description.contains('\0')),
            ("icon", self.icon.contains('\0')),
            (
                "properties",
                self.properties
                    .iter()
                    .any(|(name, value)| name.contains('\0') || holds_nul(value)),
            ),
            (
                "cardProperties",
                self.card_properties.iter().any(|definition| {
                    definition
                        .iter()
                        .any(|(name, value)| name.contains('\0') || holds_nul(value))
                }),
            ),
        ];
        match texts.into_iter().find(|(_, holds)| *holds) {
            Some((member, _)) => Err(ContainerError::NulCharacter { member }),
            None => Ok(()),
        }
    }

    /// Checks the card property definitions of the board alone, for where a board is
    /// stored that came no way that checks the rest.
    ///
    /// # Errors
    ///
    /// Returns the error of [`validate_card_property_definitions`].
    pub fn validate_card_properties(&self) -> Result<(), CardPropertyError> {
        validate_card_property_definitions(&self.card_properties)
    }
}

/// A partial update of a `Board`. Absent members leave the board unchanged.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(default, rename_all = "camelCase")]
#[schema(as = BoardPatch)]
pub struct ContainerPatch {
    /// The new board type.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub container_type: Option<ContainerType>,
    /// The new minimum role.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimum_role: Option<MinimumRole>,
    /// The new title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The new description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The new icon.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Whether the description is shown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub show_description: Option<bool>,
    /// Board properties to set.
    #[serde(deserialize_with = "null_as_default")]
    #[schema(value_type = Object)]
    pub updated_properties: JsonMap,
    /// Keys of board properties to remove.
    #[serde(deserialize_with = "null_as_default")]
    pub deleted_properties: Vec<String>,
    /// Card property definitions to add or replace, matched by their `id`.
    #[serde(deserialize_with = "null_as_default")]
    #[schema(value_type = Vec<Object>)]
    pub updated_card_properties: Vec<JsonMap>,
    /// Identifiers of card property definitions to remove.
    #[serde(deserialize_with = "null_as_default")]
    pub deleted_card_properties: Vec<String>,
}

impl ContainerPatch {
    /// Checks the card property definitions the patch sets. Applying the patch would
    /// drop a definition without an `id` silently; a client that sends one is told.
    ///
    /// # Errors
    ///
    /// Returns the error of [`validate_card_property_definitions`].
    pub fn validate(&self) -> Result<(), CardPropertyError> {
        validate_card_property_definitions(&self.updated_card_properties)
    }

    /// Applies the patch to `container`.
    ///
    /// Card properties keep their existing order; replaced definitions stay in place and
    /// new ones are appended. Deletions are applied before updates, so a definition listed
    /// in both is kept with its updated value, and then appears twice: once at its old
    /// position and once at the end. Whenever the patch touches card properties, existing
    /// definitions without a string `id` are dropped.
    pub fn apply(&self, container: &mut Container) {
        if let Some(container_type) = self.container_type {
            container.container_type = container_type;
        }
        if let Some(title) = &self.title {
            container.title.clone_from(title);
        }
        if let Some(minimum_role) = self.minimum_role {
            container.minimum_role = minimum_role;
        }
        if let Some(description) = &self.description {
            container.description.clone_from(description);
        }
        if let Some(icon) = &self.icon {
            container.icon.clone_from(icon);
        }
        if let Some(show_description) = self.show_description {
            container.show_description = show_description;
        }
        for (key, value) in &self.updated_properties {
            container.properties.insert(key.clone(), value.clone());
        }
        for key in &self.deleted_properties {
            container.properties.remove(key);
        }
        if !self.updated_card_properties.is_empty() || !self.deleted_card_properties.is_empty() {
            self.apply_card_properties(container);
        }
    }

    fn apply_card_properties(&self, container: &mut Container) {
        let mut order: Vec<String> = Vec::new();
        let mut by_id: HashMap<String, JsonMap> = HashMap::new();
        for property in std::mem::take(&mut container.card_properties) {
            if let Some(id) = property_id(&property) {
                order.push(id.clone());
                by_id.insert(id, property);
            }
        }
        for id in &self.deleted_card_properties {
            by_id.remove(id);
        }
        for property in &self.updated_card_properties {
            if let Some(id) = property_id(property) {
                if !by_id.contains_key(&id) {
                    order.push(id.clone());
                }
                by_id.insert(id, property.clone());
            }
        }
        // An identifier can be in `order` twice: the patch deletes it and adds it back.
        // The definition is emitted once, at its first place. (The original emits it once
        // per occurrence, which leaves a board with two definitions of one identifier.)
        let mut emitted = HashSet::new();
        container.card_properties = order
            .iter()
            .filter(|id| emitted.insert(id.as_str()))
            .filter_map(|id| by_id.get(id).cloned())
            .collect();
    }
}

/// Returns the string `id` of a card property definition, if it has one.
fn property_id(property: &JsonMap) -> Option<String> {
    match property.get("id") {
        Some(Value::String(id)) => Some(id.clone()),
        _ => None,
    }
}

/// A user's membership of a board.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(default, rename_all = "camelCase")]
#[expect(
    clippy::struct_excessive_bools,
    reason = "the four scheme flags and `synthetic` are separate members of the wire format"
)]
#[schema(as = BoardMember)]
pub struct ContainerMember {
    /// The identifier of the board.
    #[serde(deserialize_with = "null_as_default")]
    #[serde(rename = "boardId")]
    pub container_id: String,
    /// The identifier of the user.
    #[serde(deserialize_with = "null_as_default")]
    pub user_id: String,
    /// The user's independent roles on the board. Nothing stores any; not part of the
    /// JSON form.
    #[serde(skip)]
    pub roles: String,
    /// The user administers the board.
    #[serde(deserialize_with = "null_as_default")]
    pub scheme_admin: bool,
    /// The user may edit the board.
    #[serde(deserialize_with = "null_as_default")]
    pub scheme_editor: bool,
    /// The user may comment on the board's cards.
    #[serde(deserialize_with = "null_as_default")]
    pub scheme_commenter: bool,
    /// The user may view the board.
    #[serde(deserialize_with = "null_as_default")]
    pub scheme_viewer: bool,
    /// The membership has no directly stored role record.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub synthetic: bool,
    /// The highest role inherited from teams; direct flags remain unchanged.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inherited_role: Option<MinimumRole>,
}

impl ContainerMember {
    /// The role the membership has: the highest of its flags, `None` if it has none.
    #[must_use]
    pub fn role(&self) -> Option<ContainerRole> {
        [
            (self.scheme_admin, ContainerRole::Admin),
            (self.scheme_editor, ContainerRole::Editor),
            (self.scheme_commenter, ContainerRole::Commenter),
            (self.scheme_viewer, ContainerRole::Viewer),
        ]
        .into_iter()
        .find_map(|(held, role)| held.then_some(role))
        .max(self.inherited_role.map(ContainerRole::from))
    }

    /// The membership with exactly the flag of `role`, and none for `None`.
    #[must_use]
    pub fn with_role(self, role: Option<ContainerRole>) -> Self {
        Self {
            scheme_admin: role == Some(ContainerRole::Admin),
            scheme_editor: role == Some(ContainerRole::Editor),
            scheme_commenter: role == Some(ContainerRole::Commenter),
            scheme_viewer: role == Some(ContainerRole::Viewer),
            ..self
        }
    }
}

/// The board member that a board search matches against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContainerSearchField {
    /// Match the search term against board titles.
    Title,
    /// Match the search term against the names of card properties.
    PropertyName,
}

/// Error returned when a string names no board search field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("invalid board search field")]
pub struct ParseContainerSearchFieldError;

impl FromStr for ContainerSearchField {
    type Err = ParseContainerSearchFieldError;

    fn from_str(field: &str) -> Result<Self, Self::Err> {
        match field {
            "title" => Ok(Self::Title),
            "property_name" => Ok(Self::PropertyName),
            _ => Err(ParseContainerSearchFieldError),
        }
    }
}
