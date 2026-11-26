//! The tools that write: each makes the request body of the route it stands for and
//! hands it to what the route does with it, so that what is checked of a change, who is
//! told of it and how it is kept (the version before an agent's, the agent's own, a body
//! added to while people type) are the route's.

use axum::body::Bytes;
use fvoci_model::{
    ApiTokenScope, BlockError, Body, BodyChange, CardPatch, ContainerKinds, PagePatch, Unit,
    UnitKind, UnitPatch,
};
use serde::Serialize;
use serde_json::{Map, Value, json};

use super::arguments::{
    ItemKind, OutlineEntry, PageOrUnit, PostCommentArguments, ProgressArguments, SaveCardArguments,
    SavePageOrUnitArguments,
};
use super::reads::{card_in_markdown, listed_comment, page_in_markdown, to_json, unit_in_markdown};
use super::tools::{ToolError, enter};
use crate::api::card_comments::{CardCommentParent, NewCardComment};
use crate::api::card_markdown::append_to_card;
use crate::api::cards::{card_with_permission, create_card_from, patch_card_from};
use crate::api::comments::{CommentParentKind, CommenterOf, create_comment_on};
use crate::api::courses::{create_unit_from, patch_unit_from, unit_with_permission};
use crate::api::page_comments::{NewPageComment, PageCommentParent};
use crate::api::unit_comments::{NewUnitComment, UnitCommentParent};
use crate::api::unit_progress::{ProgressToRecord, progress_to_record, record_progress};
use crate::api::wikis::{create_page_from, page_with_permission, patch_page_from};
use crate::api::workspace::InWorkspace;
use crate::app::{App, Caller, ChangeNotification, comment_text_of_markdown};
use crate::error::AppError;
use crate::permissions::ContainerPermission;

/// What a page or unit is changed with to be denied.
const CHANGE_DENIED: &str = "access denied to change it";

/// Why a `readAt` sent with nothing written whole is refused: it would be checked by a
/// change of nothing.
const READ_AT_ALONE: &str = "readAt is checked of what is written whole (the title, the \
     body and the like); appendMarkdown keeps what changed since and takes none";

/// `value` as a request body.
fn request_body(value: &impl Serialize) -> Result<Bytes, AppError> {
    serde_json::to_vec(value)
        .map(Bytes::from)
        .map_err(|error| AppError::internal(format_args!("a request body as JSON: {error}")))
}

/// The members of a request body, those of `members` that are there.
fn members<const N: usize>(members: [(&str, Option<Value>); N]) -> Value {
    Value::Object(
        members
            .into_iter()
            .filter_map(|(name, value)| value.map(|value| (name.to_owned(), value)))
            .collect::<Map<_, _>>(),
    )
}

/// Refuses the arguments that write a body two ways at once.
fn one_body(
    body_markdown: Option<&String>,
    append_markdown: Option<&String>,
) -> Result<(), ToolError> {
    if body_markdown.is_some() && append_markdown.is_some() {
        return Err(ToolError::arguments(
            "bodyMarkdown writes the body whole and appendMarkdown adds to it: send one",
        ));
    }
    Ok(())
}

pub(super) async fn save_card(
    app: &App,
    caller: &Caller,
    arguments: SaveCardArguments,
) -> Result<Value, ToolError> {
    one_body(
        arguments.body_markdown.as_ref(),
        arguments.append_markdown.as_ref(),
    )?;
    let writes_whole = arguments.title.is_some()
        || arguments.icon.is_some()
        || arguments.parent_id.is_some()
        || arguments.properties.is_some()
        || arguments.deleted_properties.is_some()
        || arguments.body_markdown.is_some();
    if arguments.read_at.is_some() && !writes_whole {
        return Err(ToolError::arguments(READ_AT_ALONE));
    }
    let ws = enter(
        app,
        caller,
        &arguments.workspace_id,
        ApiTokenScope::Write,
        ContainerKinds::BOARDS,
    )
    .await?;
    let card_id = match (arguments.card_id, &arguments.board_id) {
        (None, Some(board_id)) => {
            if arguments.read_at.is_some() || arguments.deleted_properties.is_some() {
                return Err(ToolError::arguments(
                    "readAt and deletedProperties change a card: a new card has neither",
                ));
            }
            // A new card has no body to add to: what is added is its body, made with it.
            let body_markdown = arguments.body_markdown.or(arguments.append_markdown);
            let new = members([
                ("title", arguments.title.map(Value::String)),
                ("icon", arguments.icon.map(Value::String)),
                ("parentId", arguments.parent_id.map(Value::String)),
                ("properties", arguments.properties.map(Value::Object)),
                ("bodyMarkdown", body_markdown.map(Value::String)),
            ]);
            let body = request_body(&new)?;
            let card = create_card_from(&ws, board_id, &body, ChangeNotification::Send).await?;
            return Ok(card_in_markdown(&ws, &card.id).await?);
        }
        (Some(card_id), None) => card_id,
        _ => {
            return Err(ToolError::arguments(
                "boardId makes a card and cardId changes one: send one of them",
            ));
        }
    };
    // What is added after the other changes is checked before them, so that nothing is
    // changed when it is refused.
    if let Some(markdown) = &arguments.append_markdown {
        let card = card_with_permission(
            &ws.app,
            &ws.caller.user_id,
            &card_id,
            ContainerPermission::ManageCards,
            "access denied to change card",
        )
        .await?;
        ws.app
            .markdown_to_append(&card.container_id, markdown)
            .await?;
    }
    let patch = CardPatch {
        parent_id: arguments.parent_id,
        title: arguments.title,
        icon: arguments.icon,
        updated_properties: arguments.properties.unwrap_or_default(),
        deleted_properties: arguments.deleted_properties.unwrap_or_default(),
        body: BodyChange::Keep,
        read_at: arguments.read_at,
        body_markdown: arguments.body_markdown,
    };
    if patch != CardPatch::default() {
        let body = request_body(&patch)?;
        patch_card_from(&ws, &card_id, &body, ChangeNotification::Send).await?;
    } else if arguments.append_markdown.is_none() {
        return Err(ToolError::arguments("there is nothing to change"));
    }
    // The fields first, then what is added to the body: the body's room is asked for
    // last, by the one write that keeps what its editors type.
    if let Some(markdown) = &arguments.append_markdown {
        append_to_card(&ws, &card_id, markdown).await?;
    }
    Ok(card_in_markdown(&ws, &card_id).await?)
}

pub(super) async fn save_page_or_unit(
    app: &App,
    caller: &Caller,
    arguments: SavePageOrUnitArguments,
) -> Result<Value, ToolError> {
    one_body(
        arguments.body_markdown.as_ref(),
        arguments.append_markdown.as_ref(),
    )?;
    if arguments.id.is_some() && (arguments.parent_id.is_some() || arguments.after.is_some()) {
        return Err(ToolError::arguments(
            "parentId and after place a new one; one that is there stays where it is",
        ));
    }
    if arguments.id.is_none() && arguments.read_at.is_some() {
        return Err(ToolError::arguments("readAt is of one that is changed"));
    }
    let writes_whole =
        arguments.title.is_some() || arguments.icon.is_some() || arguments.body_markdown.is_some();
    if arguments.read_at.is_some() && !writes_whole {
        return Err(ToolError::arguments(READ_AT_ALONE));
    }
    match arguments.kind {
        PageOrUnit::Page => save_page(app, caller, arguments).await,
        PageOrUnit::Unit => save_unit(app, caller, arguments).await,
    }
}

/// What a page's or unit's body is to be written whole with, and what is to be added
/// after it, both checked before anything is written.
struct BodyWrites {
    /// The body written whole, with the fields.
    whole: BodyChange,
    /// The Markdown added after them.
    added: Option<String>,
}

/// The page or unit whose body is written: one that is made, or one that is there,
/// with its body.
#[derive(Clone, Copy)]
enum Written<'a> {
    New,
    There(Option<&'a Body>),
}

/// The writes of the body of a page or unit of the container `container_id`, checked
/// as the routes check them: a new one has no body to add to, and what is added is its
/// body.
async fn body_writes(
    ws: &InWorkspace,
    container_id: &str,
    written: Written<'_>,
    body_markdown: Option<String>,
    append_markdown: Option<String>,
) -> Result<BodyWrites, AppError> {
    let (whole, added, current) = match written {
        Written::New => (body_markdown.or(append_markdown), None, None),
        Written::There(current) => (body_markdown, append_markdown, current),
    };
    let whole = match whole {
        None => BodyChange::Keep,
        Some(markdown) => {
            let change = ws
                .app
                .body_of_markdown(container_id, current, &markdown)
                .await?;
            ws.app
                .check_body_files(container_id, change.new_body())
                .await
                .map_err(|error| error.moved("/body", "/bodyMarkdown"))?;
            change
        }
    };
    if let Some(markdown) = &added {
        ws.app.markdown_to_append(container_id, markdown).await?;
    }
    Ok(BodyWrites { whole, added })
}

/// Of the page or unit `current`, if it is there, its body as it is written.
fn written_of(current: Option<&Option<Body>>) -> Written<'_> {
    current.map_or(Written::New, |body| Written::There(body.as_ref()))
}

/// The progress `progress` says, checked before anything is written.
async fn progress_checked(
    ws: &InWorkspace,
    course_id: &str,
    progress: Option<&ProgressArguments>,
) -> Result<Option<ProgressToRecord>, AppError> {
    let Some(progress) = progress else {
        return Ok(None);
    };
    let update = members([
        ("completed", progress.completed.map(Value::Bool)),
        (
            "lastPosition",
            progress.last_position.clone().map(Value::String),
        ),
    ]);
    Ok(Some(
        progress_to_record(ws, course_id, &request_body(&update)?).await?,
    ))
}

async fn save_page(
    app: &App,
    caller: &Caller,
    arguments: SavePageOrUnitArguments,
) -> Result<Value, ToolError> {
    if arguments.progress.is_some() || arguments.unit_kind.is_some() {
        return Err(ToolError::arguments(
            "progress and unitKind are of a course's units, not of a page",
        ));
    }
    let ws = enter(
        app,
        caller,
        &arguments.workspace_id,
        ApiTokenScope::Write,
        ContainerKinds::WIKIS,
    )
    .await?;
    let wiki_id = arguments.container_id.as_str();
    let current = match &arguments.id {
        None => None,
        Some(page_id) => Some(
            page_with_permission(
                &ws,
                wiki_id,
                page_id,
                ContainerPermission::ManageCards,
                CHANGE_DENIED,
            )
            .await?,
        ),
    };
    let is_new = current.is_none();
    let fields = arguments.title.is_some() || arguments.icon.is_some();
    if !is_new
        && !fields
        && arguments.read_at.is_none()
        && arguments.body_markdown.is_none()
        && arguments.append_markdown.is_none()
    {
        return Err(ToolError::arguments("there is nothing to change"));
    }
    let writes = body_writes(
        &ws,
        wiki_id,
        written_of(current.as_ref().map(|page| &page.body)),
        arguments.body_markdown,
        arguments.append_markdown,
    )
    .await?;
    let (page_id, patch) = match current {
        None => {
            let new = members([
                ("title", arguments.title.map(Value::String)),
                ("icon", arguments.icon.map(Value::String)),
                ("parentId", arguments.parent_id.map(Value::String)),
                ("after", arguments.after.map(Value::String)),
            ]);
            let page = create_page_from(&ws, wiki_id, &request_body(&new)?).await?;
            let patch = PagePatch {
                read_at: Some(page.updated_at),
                body: writes.whole,
                ..PagePatch::default()
            };
            (page.id, patch)
        }
        Some(page) => {
            let patch = PagePatch {
                title: arguments.title,
                icon: arguments.icon,
                body: writes.whole,
                read_at: arguments.read_at,
            };
            (page.id, patch)
        }
    };
    if !patch.body.is_keep() || (!is_new && fields) {
        patch_page_from(&ws, wiki_id, &page_id, &request_body(&patch)?).await?;
    }
    if let Some(markdown) = &writes.added {
        ws.app
            .append_markdown(&ws.caller, &page_id, wiki_id, markdown)
            .await?;
    }
    Ok(page_in_markdown(&ws, wiki_id, &page_id).await?)
}

async fn save_unit(
    app: &App,
    caller: &Caller,
    arguments: SavePageOrUnitArguments,
) -> Result<Value, ToolError> {
    if arguments.icon.is_some() {
        return Err(ToolError::arguments("a unit has no icon"));
    }
    if arguments.id.is_some() && arguments.unit_kind.is_some() {
        return Err(ToolError::arguments(
            "unitKind is of a new entry; one that is there stays what it is",
        ));
    }
    let ws = enter(
        app,
        caller,
        &arguments.workspace_id,
        ApiTokenScope::Write,
        ContainerKinds::COURSES,
    )
    .await?;
    let course_id = arguments.container_id.as_str();
    let has_body = arguments.body_markdown.is_some() || arguments.append_markdown.is_some();
    let changes = arguments.title.is_some() || arguments.read_at.is_some() || has_body;
    // Progress alone is a reader's to record; a change is an editor's.
    let needs = if changes {
        ContainerPermission::ManageCards
    } else {
        ContainerPermission::View
    };
    let current = unit_there(&ws, course_id, arguments.id.as_deref(), needs).await?;
    let is_new = current.is_none();
    if !is_new && !changes && arguments.progress.is_none() {
        return Err(ToolError::arguments("there is nothing to change"));
    }
    let kind = current
        .as_ref()
        .map_or_else(|| kind_asked(arguments.unit_kind), |unit| unit.kind);
    if kind == UnitKind::Chapter && has_body {
        return Err(AppError::from(BlockError::BodyNotAllowed).into());
    }
    // Everything that is written is checked before the first of it.
    let progress = progress_checked(&ws, course_id, arguments.progress.as_ref()).await?;
    let writes = body_writes(
        &ws,
        course_id,
        written_of(current.as_ref().map(|unit| &unit.body)),
        arguments.body_markdown,
        arguments.append_markdown,
    )
    .await?;
    let (unit_id, patch) = match current {
        None => {
            let new = members([
                ("kind", Some(Value::String(kind_name(kind).to_owned()))),
                ("title", arguments.title.map(Value::String)),
                ("parentId", arguments.parent_id.map(Value::String)),
                ("after", arguments.after.map(Value::String)),
            ]);
            let unit = create_unit_from(&ws, course_id, &request_body(&new)?).await?;
            let patch = UnitPatch {
                read_at: Some(unit.updated_at),
                body: writes.whole,
                ..UnitPatch::default()
            };
            (unit.id, patch)
        }
        Some(unit) => {
            let patch = UnitPatch {
                title: arguments.title,
                body: writes.whole,
                read_at: arguments.read_at,
                ..UnitPatch::default()
            };
            (unit.id, patch)
        }
    };
    if !patch.body.is_keep() || (!is_new && patch.title.is_some()) {
        patch_unit_from(&ws, course_id, &unit_id, &request_body(&patch)?).await?;
    }
    if let Some(markdown) = &writes.added {
        ws.app
            .append_markdown(&ws.caller, &unit_id, course_id, markdown)
            .await?;
    }
    let mut saved = unit_in_markdown(&ws, course_id, &unit_id).await?;
    if let Some(progress) = &progress {
        let recorded = record_progress(&ws, course_id, &unit_id, progress).await?;
        if let Value::Object(members) = &mut saved {
            members.insert("myProgress".to_owned(), to_json(&recorded)?);
        }
    }
    Ok(saved)
}

/// The unit `unit_id` of the course `course_id`, if one is named, for a caller who has
/// `permission` there.
async fn unit_there(
    ws: &InWorkspace,
    course_id: &str,
    unit_id: Option<&str>,
    permission: ContainerPermission,
) -> Result<Option<Unit>, AppError> {
    let Some(unit_id) = unit_id else {
        return Ok(None);
    };
    let block = unit_with_permission(ws, course_id, unit_id, permission, CHANGE_DENIED).await?;
    Ok(Some(Unit::from(block)))
}

/// What a new entry of a course is, as `unitKind` asks: a unit if it is left out.
fn kind_asked(asked: Option<OutlineEntry>) -> UnitKind {
    match asked {
        Some(OutlineEntry::Chapter) => UnitKind::Chapter,
        Some(OutlineEntry::Unit) | None => UnitKind::Unit,
    }
}

/// The word `createUnit` takes for a unit of `kind`.
fn kind_name(kind: UnitKind) -> &'static str {
    match kind {
        UnitKind::Chapter => OutlineEntry::Chapter.as_str(),
        UnitKind::Unit => OutlineEntry::Unit.as_str(),
    }
}

pub(super) async fn post_comment(
    app: &App,
    caller: &Caller,
    arguments: PostCommentArguments,
) -> Result<Value, ToolError> {
    let text = comment_text_of_markdown(&arguments.markdown, "markdown")?;
    let body = request_body(&json!({ "body": text }))?;
    let PostCommentArguments {
        workspace_id,
        kind,
        container_id,
        item_id,
        ..
    } = arguments;
    let ws = enter(
        app,
        caller,
        &workspace_id,
        ApiTokenScope::Write,
        ContainerKinds::EVERY,
    )
    .await?;
    let comment = match kind {
        ItemKind::Card => {
            comment_on::<CardCommentParent, NewCardComment>(
                ws,
                container_id,
                &item_id,
                "card",
                &body,
            )
            .await?
        }
        ItemKind::Page => {
            comment_on::<PageCommentParent, NewPageComment>(
                ws,
                container_id,
                &item_id,
                "page",
                &body,
            )
            .await?
        }
        ItemKind::Unit => {
            comment_on::<UnitCommentParent, NewUnitComment>(
                ws,
                container_id,
                &item_id,
                "unit",
                &body,
            )
            .await?
        }
    };
    Ok(comment)
}

/// Writes the comment `body` on the item `parent_id` of `container_id`, as the route
/// that comments on a `K` does, and answers with it as `list_comments` lists it.
async fn comment_on<K, T>(
    ws: InWorkspace,
    container_id: String,
    parent_id: &str,
    parent_what: &'static str,
    body: &Bytes,
) -> Result<Value, AppError>
where
    K: CommentParentKind,
    T: serde::de::DeserializeOwned,
{
    let commenter = CommenterOf::<K>::admit(ws, container_id, parent_id, None).await?;
    let comment = create_comment_on::<T>(
        &commenter.ws,
        &commenter.container_id,
        commenter.parent,
        parent_what,
        body,
        ChangeNotification::Send,
    )
    .await?;
    let listed = listed_comment(&commenter.ws, &commenter.container_id, comment).await?;
    serde_json::to_value(listed)
        .map_err(|error| AppError::internal(format_args!("a comment as JSON: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_body_has_the_members_that_are_there() {
        let body = members([
            ("title", Some(Value::String("A".to_owned()))),
            ("icon", None),
        ]);
        assert_eq!(body, json!({ "title": "A" }));
    }

    #[test]
    fn a_body_is_written_one_way() {
        let markdown = "text".to_owned();
        assert!(one_body(Some(&markdown), None).is_ok());
        assert!(matches!(
            one_body(Some(&markdown), Some(&markdown)),
            Err(ToolError::Arguments(_))
        ));
    }
}
