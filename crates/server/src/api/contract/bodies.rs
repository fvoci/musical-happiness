//! The request bodies whose shape differs from what the server answers.
//!
//! The server decodes most bodies into the type it also answers with, leniently: a
//! member that is left out, or `null`, is taken as empty, and members the server sets
//! itself are read and thrown away. The contract says less and means it: these types
//! list the members a request is for, and which of them it needs. Nothing is decoded
//! into them. A member that is optional here may also be sent as `null`.

#![expect(
    dead_code,
    reason = "these types describe request bodies; nothing is decoded into them"
)]

use fvoci_model::{
    Body, CardOpen, CategoryType, ContainerType, Density, HourCycle, JsonMap, MinimumRole, Theme,
    WeekStart,
};
use utoipa::ToSchema;

/// The identifiers of the users asked for: at least one, at most
/// [`MAX_USER_IDS`](crate::api::MAX_USER_IDS).
#[derive(Debug)]
pub(crate) struct UserIds;

impl utoipa::PartialSchema for UserIds {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        utoipa::openapi::schema::ArrayBuilder::new()
            .items(String::schema())
            .min_items(Some(1))
            .max_items(Some(crate::api::MAX_USER_IDS))
            .description(Some(
                "The identifiers of the users asked for: at least one, at most 200.",
            ))
            .into()
    }
}

impl ToSchema for UserIds {}

/// The identifiers the references of a body name: at most
/// [`MAX_REFERENCE_IDS`](crate::app::MAX_REFERENCE_IDS).
#[derive(Debug)]
pub(crate) struct ReferenceIds;

impl utoipa::PartialSchema for ReferenceIds {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        utoipa::openapi::schema::ArrayBuilder::new()
            .items(String::schema())
            .max_items(Some(crate::app::MAX_REFERENCE_IDS))
            .description(Some(format!(
                "The identifiers the references name, each the `id` of a node `reference`: \
                 at most {}.",
                crate::app::MAX_REFERENCE_IDS
            )))
            .into()
    }
}

impl ToSchema for ReferenceIds {}

/// A board to make.
#[derive(Debug, ToSchema)]
#[schema(rename_all = "camelCase")]
#[schema(as = NewBoard)]
pub(crate) struct NewContainer {
    /// Only where boards and blocks are made together, and needed there: a name by
    /// which the blocks of the request say which board they belong to. The server gives
    /// the board another identifier. Where one board is made, a board with an `id` is
    /// refused.
    id: Option<String>,
    /// Whether the board is open or private.
    #[schema(rename = "type")]
    container_type: ContainerType,
    /// The role a user gets who joins the board. Left out, `editor`.
    minimum_role: Option<MinimumRole>,
    /// The board's title.
    title: Option<String>,
    /// The board's description.
    description: Option<String>,
    /// The board's icon, usually one emoji.
    icon: Option<String>,
    /// Whether the description is shown in the interface.
    show_description: Option<bool>,
    /// Whether the board is a template.
    is_template: Option<bool>,
    /// The version of the built-in template the board came from.
    template_version: Option<i64>,
    /// Free-form properties of the board itself.
    #[schema(value_type = Option<Object>)]
    properties: Option<JsonMap>,
    /// The property definitions of the board's cards, in display order. Each needs a
    /// non-empty `id` that no other has.
    #[schema(value_type = Option<Vec<Object>>)]
    card_properties: Option<Vec<JsonMap>>,
}

/// A block to insert.
#[derive(Debug, ToSchema)]
#[schema(rename_all = "camelCase")]
pub(crate) struct NewBlock {
    /// A name for the block within the request. The server gives every block a new
    /// identifier, and where one block of the request names another (as its `parentId`,
    /// or in the fields `contentOrder`, `cardOrder` and `defaultTemplateId`) it puts
    /// the new identifier there. Blocks that come under one name get one identifier,
    /// which cannot be stored twice; a block that no other names needs no `id`.
    id: Option<String>,
    /// The board the block belongs to: the board of the path, or, where boards and
    /// blocks are made together, the `id` of one of the request's boards.
    #[schema(rename = "boardId")]
    container_id: String,
    /// The block this one is below: a block of the same board, or the name of another
    /// block of the request. Left out for a block directly on its board.
    parent_id: Option<String>,
    /// The block type: `card`, `view`, `comment` or `attachment`. Any other type is
    /// refused (400 naming `type`): a card's content is its body.
    #[schema(rename = "type")]
    block_type: String,
    /// The schema version of the block.
    schema: Option<i64>,
    /// The display title.
    title: Option<String>,
    /// Type-specific data, free-form: serialized, at most 64,000 characters for a card,
    /// a comment or an attachment and 800,000 for a view. The `properties` of a card
    /// among them are values by property identifier: a string, a list of strings, or
    /// `null`.
    #[schema(value_type = Option<Object>)]
    fields: Option<JsonMap>,
    /// Must be 1 or more. The server does not keep it: the block is created when the
    /// server stores it.
    create_at: i64,
    /// Must be 1 or more. The server does not keep it.
    update_at: i64,
}

/// Boards and their blocks, to make together.
#[derive(Debug, ToSchema)]
#[schema(rename_all = "camelCase")]
#[schema(as = NewBoardsAndBlocks)]
pub(crate) struct NewContainersAndBlocks {
    /// The boards, at least one. Each needs an `id`.
    #[schema(rename = "boards")]
    containers: Vec<NewContainer>,
    /// The blocks, at least one. The `boardId` of each is the `id` of one of `boards`.
    blocks: Vec<NewBlock>,
}

/// A card to make.
#[derive(Debug, ToSchema)]
#[schema(rename_all = "camelCase")]
pub(crate) struct NewCard {
    /// The board of the path, if it is there at all.
    #[schema(rename = "boardId")]
    container_id: Option<String>,
    /// The display title.
    title: Option<String>,
    /// The card's icon; at most one grapheme.
    icon: Option<String>,
    /// Whether the card is a template.
    is_template: Option<bool>,
    /// Property values by property identifier: a string, a list of strings, or `null`.
    #[schema(value_type = Option<Object>)]
    properties: Option<JsonMap>,
    /// The card's body; left out or `null` for none.
    body: Option<Body>,
}

/// A sidebar category to make.
#[derive(Debug, ToSchema)]
#[schema(rename_all = "camelCase")]
pub(crate) struct NewCategory {
    /// The display name; not blank.
    name: String,
    /// The user of the session: a user makes categories for themselves only.
    user_id: String,
    /// The workspace of the path.
    workspace_id: String,
    /// Whether the category is collapsed in the sidebar.
    collapsed: Option<bool>,
    /// The position of the category among the user's categories. It is stored 10
    /// higher: see `createCategory`.
    sort_order: Option<i64>,
    /// Left out, `custom`. A category that is made as `system` can be neither renamed
    /// nor deleted afterwards.
    #[schema(rename = "type")]
    category_type: Option<CategoryType>,
}

/// What a sidebar category is changed to. The name and the collapsed state are what
/// changes; a system category keeps its name.
#[derive(Debug, ToSchema)]
#[schema(rename_all = "camelCase")]
pub(crate) struct CategoryUpdate {
    /// The category of the path.
    id: String,
    /// The display name; not blank.
    name: String,
    /// The user of the session.
    user_id: String,
    /// The workspace of the path.
    workspace_id: String,
    /// Whether the category is collapsed in the sidebar. Left out, it is not.
    collapsed: Option<bool>,
}

/// A user to make a member of a board, and the roles to give them. A member with no
/// role may do nothing on the board, not even view it.
#[derive(Debug, ToSchema)]
#[schema(rename_all = "camelCase")]
pub(crate) struct NewMember {
    /// The identifier of the user, who must be able to see the board's workspace.
    user_id: String,
    /// The user administers the board. Only who manages the board's roles may ask for
    /// this.
    scheme_admin: Option<bool>,
    /// The user may edit the board.
    scheme_editor: Option<bool>,
    /// The user may comment on the board's cards.
    scheme_commenter: Option<bool>,
    /// The user may view the board.
    scheme_viewer: Option<bool>,
}

/// The roles a member of a board is to have. A role that is left out is taken away, and
/// a member with no role may do nothing on the board, not even view it.
#[derive(Debug, ToSchema)]
#[schema(rename_all = "camelCase")]
#[expect(
    clippy::struct_field_names,
    reason = "the members are named as the wire format names them"
)]
pub(crate) struct MemberRoles {
    /// The user administers the board.
    scheme_admin: Option<bool>,
    /// The user may edit the board.
    scheme_editor: Option<bool>,
    /// The user may comment on the board's cards.
    scheme_commenter: Option<bool>,
    /// The user may view the board.
    scheme_viewer: Option<bool>,
}

/// Whether a board is to be shared.
#[derive(Debug, ToSchema)]
#[schema(rename_all = "camelCase")]
pub(crate) struct SharingRequest {
    /// `true` shares the board, `false` stops sharing it. The read token a board had
    /// stops working when sharing stops.
    enabled: bool,
    /// With `enabled`, `true` gives the board a new read token; the old one stops
    /// working.
    regenerate: Option<bool>,
}

/// A subscription to make.
#[derive(Debug, ToSchema)]
#[schema(rename_all = "camelCase")]
pub(crate) struct NewSubscription {
    /// The type of the block subscribed to.
    block_type: String,
    /// The identifier of the block subscribed to.
    block_id: String,
    /// Always `user`.
    subscriber_type: String,
    /// The user of the session: a user subscribes for themselves only.
    subscriber_id: String,
}

/// Whether a user is to be an administrator of the instance.
#[derive(Debug, ToSchema)]
#[schema(rename_all = "camelCase")]
pub(crate) struct AdminChange {
    /// `true` makes the user an administrator, `false` ends that.
    is_admin: bool,
}

/// What a user changes of their account. A member that is left out stays as it is.
#[derive(Debug, ToSchema)]
#[schema(rename_all = "camelCase")]
pub(crate) struct AccountChange {
    /// The name to be shown by: at most 64 UTF-16 code units without control
    /// characters. Empty or `null` for none, and the login name is shown.
    display_name: Option<String>,
}

/// The settings to change. A member that is left out stays as it is; one that is `null`
/// goes back to its default.
#[derive(Debug, ToSchema)]
#[schema(rename_all = "camelCase")]
pub(crate) struct SettingsChange {
    /// `auto`, for the time zone of the device, or the IANA name of a time zone, such
    /// as `Asia/Seoul`.
    time_zone: Option<String>,
    /// The day a week starts on.
    week_start: Option<WeekStart>,
    /// How the hours of a day are written.
    hour_cycle: Option<HourCycle>,
    /// The colors of the interface.
    theme: Option<Theme>,
    /// Where a card opens.
    card_open: Option<CardOpen>,
    /// How much room the interface gives each thing.
    density: Option<Density>,
    /// Whether a device that can do so answers touches with a vibration.
    haptics: Option<bool>,
    /// `auto`, for the language of the browser, or a language tag such as `ko` or
    /// `en-US`, in the shape the browsers' `Intl` reads (Unicode BCP 47), at most 64
    /// characters; the server keeps no list of languages, and keeps a tag as given.
    #[schema(max_length = 64)]
    language: Option<String>,
}

/// The bytes of a file, as the body of an answer.
#[derive(Debug, ToSchema)]
#[schema(value_type = String, format = Binary)]
pub(crate) struct Binary(Vec<u8>);

/// A form with one file.
#[derive(Debug, ToSchema)]
pub(crate) struct FileForm {
    /// The file. An upload needs the part to have a file name; an archive to import
    /// does not.
    #[schema(value_type = String, format = Binary)]
    file: Vec<u8>,
}
