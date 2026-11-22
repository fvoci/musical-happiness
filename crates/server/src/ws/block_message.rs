//! The message `UPDATE_BLOCK`: a block that changed. A card's or a page's carries a
//! summary of its body in place of the body, which its room and HTTP give
//! (`contracts/websocket.md` 3); every other block goes whole.

use std::borrow::Cow;

use fvoci_model::{Block, Body, CardPreview};
use serde::Serialize;

use super::Outgoing;

/// What `UPDATE_BLOCK` says of a card's or a page's body in its place, as a list of
/// cards says it (`getCards`, and its `preview`).
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Summary {
    has_body: bool,
    tasks_total: u32,
    tasks_done: u32,
    preview: CardPreview,
}

impl Summary {
    fn of(body: Option<&Body>) -> Self {
        let Some(body) = body else {
            return Self::default();
        };
        let tasks = body.tasks();
        Self {
            has_body: true,
            tasks_total: tasks.total,
            tasks_done: tasks.done,
            preview: CardPreview::of(body),
        }
    }
}

/// `UPDATE_BLOCK`: the block, without its body and with its summary if it is a card or
/// a page.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct UpdateBlockMessage<'a> {
    action: &'static str,
    workspace_id: &'a str,
    block: Cow<'a, Block>,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<Summary>,
}

impl<'a> UpdateBlockMessage<'a> {
    pub(super) fn new(workspace_id: &'a str, block: &'a Block) -> Self {
        let (block, summary) = if block.block_type.has_room() {
            let summary = Summary::of(block.body.as_ref());
            (Cow::Owned(without_body(block)), Some(summary))
        } else {
            (Cow::Borrowed(block), None)
        };
        Self {
            action: "UPDATE_BLOCK",
            workspace_id,
            block,
            summary,
        }
    }

    pub(super) fn workspace_id(&self) -> &'a str {
        self.workspace_id
    }

    /// The block the message carries, to change for whoever is told less of it. The
    /// summary names nobody: its preview's text has nothing of a mention or a reference.
    pub(super) fn block_mut(&mut self) -> &mut Block {
        self.block.to_mut()
    }
}

impl Outgoing for UpdateBlockMessage<'_> {
    fn action(&self) -> &'static str {
        self.action
    }
}

/// `block` without its body, which is not copied.
fn without_body(block: &Block) -> Block {
    let Block {
        id,
        parent_id,
        created_by,
        modified_by,
        schema,
        block_type,
        title,
        fields,
        body: _,
        create_at,
        update_at,
        delete_at,
        container_id,
        edited_at,
    } = block;
    Block {
        id: id.clone(),
        parent_id: parent_id.clone(),
        created_by: created_by.clone(),
        modified_by: modified_by.clone(),
        schema: *schema,
        block_type: block_type.clone(),
        title: title.clone(),
        fields: fields.clone(),
        body: None,
        create_at: *create_at,
        update_at: *update_at,
        delete_at: *delete_at,
        container_id: container_id.clone(),
        edited_at: *edited_at,
    }
}
