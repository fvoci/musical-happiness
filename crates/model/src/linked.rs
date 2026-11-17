//! A card or a page as a reference shows it.
//!
//! A body names a block by its identifier alone (the node `reference`), and what the
//! block is called is read when the body is shown, for whoever reads: [`LinkedBlock`]
//! is that answer, for what a reference names, for what names a block, and for what
//! is found to be named.

use serde::{Deserialize, Serialize};

use crate::block::{Block, BlockType};
use crate::card::CARD_ICON_FIELD;
use crate::container::{Container, ContainerKind};
use crate::page::PAGE_ICON_FIELD;

/// What a reference may name: a card of a board or a page of a wiki.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum LinkedKind {
    /// A card of a board.
    Card,
    /// A page of a wiki.
    Page,
}

impl LinkedKind {
    /// The kind of the block, if a reference may name a block of its type.
    #[must_use]
    pub fn of(block_type: &BlockType) -> Option<Self> {
        match block_type {
            BlockType::Card => Some(Self::Card),
            BlockType::Page => Some(Self::Page),
            _ => None,
        }
    }
}

/// A card or a page as a reference shows it: what it is called, and where it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LinkedBlock {
    /// The identifier of the card or the page.
    pub id: String,
    /// Whether it is a card or a page.
    pub kind: LinkedKind,
    /// Its title; empty for one without.
    pub title: String,
    /// Its icon, at most one grapheme; empty for none.
    pub icon: String,
    /// The identifier of the board the card is on, or of the wiki the page is of.
    pub container_id: String,
    /// Whether that is a board or a wiki.
    pub container_kind: ContainerKind,
}

impl LinkedBlock {
    /// The block as a reference shows it, on the board it is of; `None` for a block
    /// that is neither a card nor a page. The body of the block is not asked for.
    #[must_use]
    pub fn of(block: &Block, container: &Container) -> Option<Self> {
        let kind = LinkedKind::of(&block.block_type)?;
        let icon_field = match kind {
            LinkedKind::Card => CARD_ICON_FIELD,
            LinkedKind::Page => PAGE_ICON_FIELD,
        };
        Some(Self {
            id: block.id.clone(),
            kind,
            title: block.title.clone(),
            icon: block
                .fields
                .get(icon_field)
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            container_id: container.id.clone(),
            container_kind: container.kind,
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn block(block_type: BlockType) -> Block {
        Block {
            id: "0197a5a0-0000-7000-8000-000000000001".to_owned(),
            container_id: "0197a5a0-0000-7000-8000-000000000002".to_owned(),
            block_type,
            title: "Plan".to_owned(),
            fields: json!({"icon": "📌", "order": "a"})
                .as_object()
                .expect("an object")
                .clone(),
            ..Block::default()
        }
    }

    #[test]
    fn a_card_and_a_page_are_shown_and_nothing_else_is() {
        let container = Container {
            id: "0197a5a0-0000-7000-8000-000000000002".to_owned(),
            kind: ContainerKind::Wiki,
            ..Container::default()
        };
        let page = LinkedBlock::of(&block(BlockType::Page), &container).expect("a page is shown");
        assert_eq!(
            serde_json::to_value(&page).expect("serialized"),
            json!({"id": "0197a5a0-0000-7000-8000-000000000001", "kind": "page",
                "title": "Plan", "icon": "📌",
                "containerId": "0197a5a0-0000-7000-8000-000000000002", "containerKind": "wiki"}),
        );
        let card = LinkedBlock::of(&block(BlockType::Card), &container).expect("a card is shown");
        assert_eq!(card.kind, LinkedKind::Card);
        for other in [
            BlockType::Comment,
            BlockType::View,
            BlockType::Attachment,
            BlockType::Board,
            BlockType::Other("text".to_owned()),
        ] {
            assert_eq!(
                LinkedBlock::of(&block(other.clone()), &container),
                None,
                "{other:?}"
            );
        }
    }
}
