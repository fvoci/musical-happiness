//! Where each page, card, unit and entry of an export of Markdown files goes: a folder
//! of the container's name, and in it a wiki's tree (a page with pages below it has a
//! folder of its name beside its file), a board's cards in `cards/` by number, a course's
//! chapters as folders of their units, and a diary's entries in `entries/` by day. What
//! a body names is written where the export put it (`Exported`).

use std::collections::{HashMap, HashSet};

use fvoci_model::{Block, BlockType, ContainerKind, ENTRY_DATE_FIELD, PAGE_ORDER_FIELD};
use serde_json::Value;

use super::names::{Folder, name_of};

/// A file of the export: what it is of, and where it goes.
#[derive(Debug)]
pub(super) struct Placed<'a> {
    pub(super) block: &'a Block,
    /// Its folders, from the root of the export.
    pub(super) folder: Vec<String>,
    /// Its name in its folder, `.md` with it.
    pub(super) name: String,
    /// What its heading says.
    pub(super) title: String,
}

impl Placed<'_> {
    /// Its folders and its name, from the root of the export.
    pub(super) fn path(&self) -> Vec<String> {
        let mut path = self.folder.clone();
        path.push(self.name.clone());
        path
    }
}

/// The files of the blocks of a container of `kind`, below `root`: in the order they are
/// written, each a name of its own in its folder. Comments, views, attachments, a
/// board's templates and a chapter (a folder) have none.
pub(super) fn layout<'a>(
    kind: ContainerKind,
    root: &[String],
    blocks: &'a [Block],
) -> Vec<Placed<'a>> {
    match kind {
        ContainerKind::Board => cards(root, blocks),
        ContainerKind::Wiki => tree(root, blocks, &[BlockType::Page]),
        ContainerKind::Course => tree(root, blocks, &[BlockType::Chapter, BlockType::Unit]),
        ContainerKind::Diary => entries(root, blocks),
    }
}

/// A board's cards, templates aside, in `cards/` by number: `12-Title.md`.
fn cards<'a>(root: &[String], blocks: &'a [Block]) -> Vec<Placed<'a>> {
    let mut cards: Vec<&Block> = blocks.iter().filter(|block| block.takes_number()).collect();
    cards.sort_by_key(|card| (card.shown_number(), card.id.as_str()));
    let folder = with(root, "cards");
    let mut names = Folder::default();
    cards
        .into_iter()
        .map(|card| {
            let title = name_of(&card.title, "card");
            let stem = match card.shown_number() {
                Some(number) => format!("{number}-{title}"),
                None => title,
            };
            Placed {
                block: card,
                folder: folder.clone(),
                name: names.take(&stem, ".md"),
                title: card.title.clone(),
            }
        })
        .collect()
}

/// A diary's entries in `entries/`, by day: `2026-11-28.md`.
fn entries<'a>(root: &[String], blocks: &'a [Block]) -> Vec<Placed<'a>> {
    let mut entries: Vec<(&str, &Block)> = blocks
        .iter()
        .filter(|block| block.block_type == BlockType::Entry)
        .map(|entry| (text(entry, ENTRY_DATE_FIELD), entry))
        .collect();
    entries.sort_by_key(|(date, entry)| (*date, entry.id.as_str()));
    let folder = with(root, "entries");
    let mut names = Folder::default();
    entries
        .into_iter()
        .map(|(date, entry)| Placed {
            block: entry,
            folder: folder.clone(),
            name: names.take(&name_of(date, "entry"), ".md"),
            title: if entry.title.is_empty() {
                date.to_owned()
            } else {
                entry.title.clone()
            },
        })
        .collect()
}

/// A tree of `types` by `parent_id`, siblings by their order, each before those below
/// it: a file in the folder of its parent, and a folder of the same name beside it for
/// those below it; a chapter has its folder alone. A block whose parent is not of the
/// tree, or that no walk from the top reaches (a loop of parents), is put at the top.
fn tree<'a>(root: &[String], blocks: &'a [Block], types: &[BlockType]) -> Vec<Placed<'a>> {
    let nodes: Vec<&Block> = blocks
        .iter()
        .filter(|block| types.contains(&block.block_type))
        .collect();
    let ids: HashSet<&str> = nodes.iter().map(|node| node.id.as_str()).collect();
    let mut below: HashMap<&str, Vec<&Block>> = HashMap::new();
    for node in &nodes {
        let parent = node.parent_id.as_str();
        let parent = if ids.contains(parent) && parent != node.id {
            parent
        } else {
            ""
        };
        below.entry(parent).or_default().push(node);
    }
    for siblings in below.values_mut() {
        siblings.sort_by(|one, other| {
            (text(one, PAGE_ORDER_FIELD), one.id.as_str())
                .cmp(&(text(other, PAGE_ORDER_FIELD), other.id.as_str()))
        });
    }
    let mut placed = Vec::new();
    let mut folders: HashMap<Vec<String>, Folder> = HashMap::new();
    let mut reached = HashSet::new();
    let top = below.get("").map_or_default(Vec::clone);
    let mut walk: Vec<(&Block, Vec<String>)> = top
        .into_iter()
        .rev()
        .map(|node| (node, root.to_vec()))
        .collect();
    while let Some((node, folder)) = walk.pop() {
        if !reached.insert(node.id.as_str()) {
            continue;
        }
        let chapter = node.block_type == BlockType::Chapter;
        let title = name_of(&node.title, fallback(node.block_type));
        let name = folders
            .entry(folder.clone())
            .or_default()
            .take(&title, if chapter { "" } else { ".md" });
        let inner = with(&folder, name.strip_suffix(".md").unwrap_or(&name));
        if let Some(children) = below.get(node.id.as_str()) {
            walk.extend(children.iter().rev().map(|child| (*child, inner.clone())));
        }
        if !chapter {
            placed.push(Placed {
                block: node,
                folder,
                name,
                title: node.title.clone(),
            });
        }
    }
    let names = folders.entry(root.to_vec()).or_default();
    for node in nodes {
        if reached.contains(node.id.as_str()) || node.block_type == BlockType::Chapter {
            continue;
        }
        placed.push(Placed {
            block: node,
            folder: root.to_vec(),
            name: names.take(&name_of(&node.title, fallback(node.block_type)), ".md"),
            title: node.title.clone(),
        });
    }
    placed
}

/// What a file is called whose title gives no name.
fn fallback(block_type: BlockType) -> &'static str {
    match block_type {
        BlockType::Unit => "unit",
        BlockType::Chapter => "chapter",
        _ => "page",
    }
}

/// `folder` with one more folder in it.
fn with(folder: &[String], name: &str) -> Vec<String> {
    let mut path = folder.to_vec();
    path.push(name.to_owned());
    path
}

/// The text of a field of a block; empty for none.
fn text<'a>(block: &'a Block, field: &str) -> &'a str {
    block
        .fields
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use fvoci_model::{Block, BlockType, ContainerKind};
    use serde_json::json;

    use super::layout;

    fn block(id: &str, block_type: BlockType, parent_id: &str, title: &str, order: &str) -> Block {
        Block {
            id: id.to_owned(),
            block_type,
            parent_id: parent_id.to_owned(),
            title: title.to_owned(),
            fields: json!({"order": order})
                .as_object()
                .cloned()
                .unwrap_or_default(),
            ..Block::default()
        }
    }

    fn paths(kind: ContainerKind, blocks: &[Block]) -> Vec<String> {
        layout(kind, &["Root".to_owned()], blocks)
            .iter()
            .map(|placed| placed.path().join("/"))
            .collect()
    }

    #[test]
    fn a_wiki_is_a_tree_of_folders_beside_their_pages() {
        let blocks = [
            block("b", BlockType::Page, "", "Second", "b"),
            block("a", BlockType::Page, "", "First", "a"),
            block("c", BlockType::Page, "a", "Child", "a"),
            block("d", BlockType::Page, "a", "child", "b"),
            block("e", BlockType::Comment, "a", "", ""),
            block("f", BlockType::Page, "g", "Loop", "a"),
            block("g", BlockType::Page, "f", "Back", "a"),
        ];
        assert_eq!(
            paths(ContainerKind::Wiki, &blocks),
            [
                "Root/First.md",
                "Root/First/Child.md",
                "Root/First/child (2).md",
                "Root/Second.md",
                "Root/Loop.md",
                "Root/Back.md",
            ]
        );
    }

    #[test]
    fn a_course_has_its_chapters_as_folders_and_a_board_its_cards_by_number() {
        let course = [
            block("u2", BlockType::Unit, "", "Alone", "b"),
            block("ch", BlockType::Chapter, "", "Basics", "a"),
            block("u1", BlockType::Unit, "ch", "One: start", "a"),
        ];
        assert_eq!(
            paths(ContainerKind::Course, &course),
            ["Root/Basics/One_ start.md", "Root/Alone.md"]
        );
        let mut first = block("x", BlockType::Card, "", "Plan", "");
        first.number = Some(2);
        let mut second = block("y", BlockType::Card, "", "", "");
        second.number = Some(1);
        let mut template = block("z", BlockType::Card, "", "Template", "");
        template.fields.insert("isTemplate".to_owned(), json!(true));
        assert_eq!(
            paths(ContainerKind::Board, &[first, second, template]),
            ["Root/cards/1-card.md", "Root/cards/2-Plan.md"]
        );
    }
}
