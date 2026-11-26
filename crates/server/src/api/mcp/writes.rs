//! The tools that write: each makes the request body of the route it stands for and
//! hands it to what the route does with it, so that what is checked of a change, who is
//! told of it and how it is kept (the version before an agent's, the agent's own, a body
//! added to while people type) are the route's.

use axum::body::Bytes;
use fvoci_model::{ApiTokenScope, BodyChange, CardPatch, ContainerKinds, PagePatch, UnitPatch};
use serde::Serialize;
use serde_json::{Map, Value, json};

use super::arguments::{
    ItemKind, OutlineEntry, PageOrUnit, PostCommentArguments, SaveCardArguments,
    SavePageOrUnitArguments,
};
use super::reads::{card_in_markdown, listed_comment, page_in_markdown, unit_in_markdown};
use super::tools::{ToolError, enter};
use crate::api::card_comments::{CardCommentParent, NewCardComment};
use crate::api::card_markdown::append_to_card;
use crate::api::cards::{create_card_from, patch_card_from};
use crate::api::comments::{CommentParentKind, CommenterOf, create_comment_on};
use crate::api::courses::{create_unit_from, patch_unit_from, unit_with_permission};
use crate::api::page_comments::{NewPageComment, PageCommentParent};
use crate::api::unit_comments::{NewUnitComment, UnitCommentParent};
use crate::api::unit_progress::put_progress_from;
use crate::api::wikis::{create_page_from, page_with_permission, patch_page_from};
use crate::api::workspace::InWorkspace;
use crate::app::{App, Caller, ChangeNotification, comment_text_of_markdown};
use crate::error::AppError;
use crate::permissions::ContainerPermission;

/// What a page or unit is changed with to be denied.
const CHANGE_DENIED: &str = "access denied to change it";

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
            let new = members([
                ("title", arguments.title.map(Value::String)),
                ("icon", arguments.icon.map(Value::String)),
                ("parentId", arguments.parent_id.map(Value::String)),
                ("properties", arguments.properties.map(Value::Object)),
                ("bodyMarkdown", arguments.body_markdown.map(Value::String)),
            ]);
            let body = request_body(&new)?;
            create_card_from(&ws, board_id, &body, ChangeNotification::Send)
                .await?
                .id
        }
        (Some(card_id), None) => {
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
            card_id
        }
        _ => {
            return Err(ToolError::arguments(
                "boardId makes a card and cardId changes one: send one of them",
            ));
        }
    };
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
    match arguments.kind {
        PageOrUnit::Page => save_page(app, caller, arguments).await,
        PageOrUnit::Unit => save_unit(app, caller, arguments).await,
    }
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
    let (page_id, read_at) = match &arguments.id {
        None => {
            let new = members([
                ("title", arguments.title.clone().map(Value::String)),
                ("icon", arguments.icon.clone().map(Value::String)),
                ("parentId", arguments.parent_id.clone().map(Value::String)),
                ("after", arguments.after.clone().map(Value::String)),
            ]);
            let page = create_page_from(&ws, wiki_id, &request_body(&new)?).await?;
            (page.id, Some(page.updated_at))
        }
        Some(page_id) => (page_id.clone(), arguments.read_at),
    };
    let is_new = arguments.id.is_none();
    let body = match &arguments.body_markdown {
        None => BodyChange::Keep,
        Some(markdown) => {
            let current = page_with_permission(
                &ws,
                wiki_id,
                &page_id,
                ContainerPermission::ManageCards,
                CHANGE_DENIED,
            )
            .await?;
            ws.app
                .body_of_markdown(wiki_id, current.body.as_ref(), markdown)
                .await?
        }
    };
    let patch = PagePatch {
        title: arguments.title.filter(|_| !is_new),
        icon: arguments.icon.filter(|_| !is_new),
        body,
        read_at,
    };
    if patch.title.is_some() || patch.icon.is_some() || !patch.body.is_keep() {
        patch_page_from(&ws, wiki_id, &page_id, &request_body(&patch)?).await?;
    } else if !is_new && arguments.append_markdown.is_none() {
        return Err(ToolError::arguments("there is nothing to change"));
    }
    if let Some(markdown) = &arguments.append_markdown {
        let page = page_with_permission(
            &ws,
            wiki_id,
            &page_id,
            ContainerPermission::ManageCards,
            CHANGE_DENIED,
        )
        .await?;
        ws.app
            .append_markdown(&ws.caller, &page.id, wiki_id, markdown)
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
    let (unit_id, read_at) = match &arguments.id {
        None => {
            let kind = arguments.unit_kind.map_or("unit", OutlineEntry::as_str);
            let new = members([
                ("kind", Some(Value::String(kind.to_owned()))),
                ("title", arguments.title.clone().map(Value::String)),
                ("parentId", arguments.parent_id.clone().map(Value::String)),
                ("after", arguments.after.clone().map(Value::String)),
            ]);
            let unit = create_unit_from(&ws, course_id, &request_body(&new)?).await?;
            (unit.id, Some(unit.updated_at))
        }
        Some(unit_id) => (unit_id.clone(), arguments.read_at),
    };
    let is_new = arguments.id.is_none();
    let body = match &arguments.body_markdown {
        None => BodyChange::Keep,
        Some(markdown) => {
            let current = unit_with_permission(
                &ws,
                course_id,
                &unit_id,
                ContainerPermission::ManageCards,
                CHANGE_DENIED,
            )
            .await?;
            ws.app
                .body_of_markdown(course_id, current.body.as_ref(), markdown)
                .await?
        }
    };
    let patch = UnitPatch {
        title: arguments.title.filter(|_| !is_new),
        body,
        read_at,
        ..UnitPatch::default()
    };
    let changes = patch.title.is_some() || !patch.body.is_keep();
    if changes {
        patch_unit_from(&ws, course_id, &unit_id, &request_body(&patch)?).await?;
    } else if !is_new && arguments.append_markdown.is_none() && arguments.progress.is_none() {
        return Err(ToolError::arguments("there is nothing to change"));
    }
    if let Some(markdown) = &arguments.append_markdown {
        let unit = unit_with_permission(
            &ws,
            course_id,
            &unit_id,
            ContainerPermission::ManageCards,
            CHANGE_DENIED,
        )
        .await?;
        ws.app
            .append_markdown(&ws.caller, &unit.id, course_id, markdown)
            .await?;
    }
    let mut saved = unit_in_markdown(&ws, course_id, &unit_id).await?;
    if let Some(progress) = arguments.progress {
        let update = members([
            ("completed", progress.completed.map(Value::Bool)),
            ("lastPosition", progress.last_position.map(Value::String)),
        ]);
        let recorded = put_progress_from(&ws, course_id, &unit_id, &request_body(&update)?).await?;
        if let Value::Object(members) = &mut saved {
            members.insert(
                "myProgress".to_owned(),
                serde_json::to_value(recorded).unwrap_or_default(),
            );
        }
    }
    Ok(saved)
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
