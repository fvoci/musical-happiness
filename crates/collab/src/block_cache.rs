//! Per-top-level-block projection and validation cache for [`crate::BodyDoc`].
//!
//! A `ProseMirror`/`TipTap` body lives under the root `XmlFragment("default")` as a flat
//! sequence of independent top-level block elements (`paragraph`, `heading`, lists,
//! `blockquote`, `codeBlock`, `horizontalRule`, `image`, `file`, `blockMath`, `table`).
//! Ordinary edits mutate only a handful of those blocks. Storing a compact summary per
//! live top-level block (keyed by its immutable Yjs [`ID`], with no retained
//! `serde_json::Value` tree) lets [`crate::BodyDoc::take_in_place`] project, validate,
//! and size only the blocks touched by an update while keeping peak heap and per-doc
//! memory small.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use fvoci_model::{BODY_MAX_BYTES, BODY_MAX_NODES, Body, BodyError};
use serde_json::{Value, json};
use yrs::branch::Branch;
use yrs::{
    BranchID, ClientID, Doc, ID, IdSet, ReadTxn, Transact as _, Xml as _, XmlElementRef,
    XmlFragment as _, XmlFragmentRef, XmlOut,
};

use crate::FIELD;
use crate::project::{json_size_of, project_element};
use crate::read::{self, Child};

/// Compact JSON byte length of `{"content":[],"type":"doc"}` around a document's blocks.
const DOC_WRAPPER_BYTES: usize = r#"{"content":[],"type":"doc"}"#.len();

/// Compact JSON byte length of a single empty paragraph `{"type":"paragraph"}`.
const EMPTY_PARAGRAPH_BYTES: usize = r#"{"type":"paragraph"}"#.len();

/// What a single in-place `apply_update` transaction changed in the document, captured
/// right after `TransactionMut::commit` in `O(changed)` time without walking the root.
#[derive(Debug)]
pub(crate) struct ApplyDelta {
    /// Nested branch IDs from `TransactionMut::changed_parent_types` (every existing
    /// modified branch and its ancestors up to the root).
    pub(crate) changed_branches: Vec<ID>,
    /// All ID ranges deleted by the transaction.
    pub(crate) delete_set: IdSet,
    /// Shared-type IDs newly introduced by the update (from `update::read`).
    pub(crate) new_types: Vec<ID>,
}

/// Compact validated summary of one live top-level block under `FIELD`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BlockSummary {
    /// Total JSON nodes in this block (the block itself plus all descendants).
    pub(crate) nodes: u32,
    /// Exact compact JSON byte length of this block as serialized by `serde_json`.
    pub(crate) json_bytes: u32,
    /// Whether this block is an empty paragraph (`{"type":"paragraph"}`).
    pub(crate) is_empty_paragraph: bool,
    /// Whether any `tableRow` inside this block contains an empty `XmlText` child,
    /// which `repair::Survey::row` deletes during a repair even though `project` skips it.
    pub(crate) has_empty_row_text: bool,
}

/// A validated block's summary together with any file IDs it references.
#[derive(Debug)]
pub(crate) struct BlockEntry {
    pub(crate) summary: BlockSummary,
    pub(crate) file_ids: Box<[String]>,
}

/// Incremental per-block cache for a valid [`crate::BodyDoc`].
///
/// Invariant when synchronized with a `Doc`:
/// - Every key `id` in `blocks` is a live (`!is_deleted()`) direct `XmlElement` child of
///   the root fragment `FIELD` (`parent_sub` is `None`).
/// - `blocks.len() == root.len(&txn)` (so the root holds no `XmlText`, `XmlFragment`, or
///   non-XML countable item).
/// - `total_nodes`, `total_json_bytes`, and `file_ids` match the exact sums/entries over
///   `blocks`.
#[derive(Debug, Clone, Default)]
pub(crate) struct BlockCache {
    blocks: BTreeMap<ID, BlockSummary>,
    /// Sparse map of deduplicated document-order `fileId`s for blocks that contain at
    /// least one `image` or `file` node. Empty for text-only documents (zero allocation).
    file_ids: HashMap<ID, Box<[String]>>,
    total_nodes: u64,
    total_json_bytes: u64,
}

impl BlockCache {
    /// Creates an empty cache matching an empty document.
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Number of cached live top-level blocks.
    #[must_use]
    pub(crate) fn len(&self) -> usize {
        self.blocks.len()
    }

    /// Whether there are no cached top-level blocks.
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    /// Whether `id` is currently cached as a live top-level block.
    #[must_use]
    fn contains(&self, id: &ID) -> bool {
        self.blocks.contains_key(id)
    }

    /// Returns the cached summary for `id`, if present.
    #[must_use]
    pub(crate) fn get(&self, id: &ID) -> Option<&BlockSummary> {
        self.blocks.get(id)
    }

    /// Inserts or replaces the cached entry for `id`, keeping running sums exact.
    pub(crate) fn upsert(&mut self, id: ID, entry: BlockEntry) {
        if let Some(old) = self.blocks.insert(id, entry.summary) {
            self.total_nodes = self.total_nodes.saturating_sub(u64::from(old.nodes));
            self.total_json_bytes = self
                .total_json_bytes
                .saturating_sub(u64::from(old.json_bytes));
        }
        self.total_nodes = self
            .total_nodes
            .saturating_add(u64::from(entry.summary.nodes));
        self.total_json_bytes = self
            .total_json_bytes
            .saturating_add(u64::from(entry.summary.json_bytes));
        if entry.file_ids.is_empty() {
            self.file_ids.remove(&id);
        } else {
            self.file_ids.insert(id, entry.file_ids);
        }
    }

    /// Removes `id` from the cache if present, updating running sums.
    pub(crate) fn remove(&mut self, id: &ID) {
        if let Some(old) = self.blocks.remove(id) {
            self.total_nodes = self.total_nodes.saturating_sub(u64::from(old.nodes));
            self.total_json_bytes = self
                .total_json_bytes
                .saturating_sub(u64::from(old.json_bytes));
        }
        self.file_ids.remove(id);
    }

    /// Removes every cached top-level block belonging to `client` with `clock` in
    /// `[start, end)`.
    fn remove_range(&mut self, client: ClientID, start: u32, end: u32) {
        if start >= end {
            return;
        }
        let lo = ID::new(client, start);
        let hi = ID::new(client, end);
        let removed: Vec<ID> = self.blocks.range(lo..hi).map(|(id, _)| *id).collect();
        for id in removed {
            self.remove(&id);
        }
    }

    /// Empties all cached blocks and releases backing table allocations.
    pub(crate) fn clear(&mut self) {
        *self = Self::new();
    }

    /// Whether the cached blocks represent a single empty paragraph (`Body::parse` -> `None`).
    #[must_use]
    pub(crate) fn is_empty_document(&self) -> bool {
        self.blocks.len() == 1
            && self
                .blocks
                .values()
                .next()
                .is_some_and(|summary| summary.is_empty_paragraph)
    }

    /// Exact compact JSON byte length of the non-empty body, or `None` when the document
    /// has no body (empty root or one empty paragraph).
    #[must_use]
    pub(crate) fn body_bytes(&self) -> Option<usize> {
        if self.is_empty() || self.is_empty_document() {
            return None;
        }
        Some(
            usize::try_from(document_json_bytes(
                self.total_json_bytes,
                self.blocks.len(),
            ))
            .unwrap_or(usize::MAX),
        )
    }

    /// Enforces whole-document node count and JSON byte limits with the exact precedence
    /// of [`Body::parse`] (`TooManyNodes`, then empty-doc check, then `TooLarge`).
    ///
    /// # Errors
    ///
    /// Returns [`BodyError::TooManyNodes`] or [`BodyError::TooLarge`] when over the limit.
    pub(crate) fn check_whole_limits(&self) -> Result<(), BodyError> {
        check_totals(DocTotals {
            blocks: self.blocks.len(),
            nodes: self.total_nodes,
            json_bytes: self.total_json_bytes,
            is_empty_doc: self.is_empty_document(),
        })
    }

    /// Builds a fresh cache by projecting and validating every top-level block in `doc`
    /// one at a time, immediately dropping each block's JSON tree and stopping early as
    /// soon as `BODY_MAX_NODES` or `BODY_MAX_BYTES` is exceeded.
    ///
    /// # Errors
    ///
    /// Returns a [`BodyError`] if `doc` is not a valid body.
    pub(crate) fn build(doc: &Doc) -> Result<Self, BodyError> {
        let txn = doc.transact();
        let Some(root) = txn.get_xml_fragment(FIELD) else {
            return Ok(Self::new());
        };
        let Some(children) = read::children(&txn, &root) else {
            return Err(BodyError::InvalidNode {
                path: String::new(),
                problem: "holds something that is neither a node nor a text",
            });
        };
        let mut cache = Self::new();
        let max_nodes = u64::try_from(BODY_MAX_NODES).unwrap_or(u64::MAX);
        let max_bytes = u64::try_from(BODY_MAX_BYTES).unwrap_or(u64::MAX);
        for (index, child) in children.into_iter().enumerate() {
            match child {
                Child::Element(element) => {
                    let Some(id) = element_id(&element) else {
                        return Err(BodyError::InvalidNode {
                            path: format!("/content/{index}"),
                            problem: "root child is not a nested element",
                        });
                    };
                    let entry = project_and_validate_block(&txn, &element)?;
                    cache.upsert(id, entry);
                    if 1_u64.saturating_add(cache.total_nodes) > max_nodes {
                        return Err(BodyError::TooManyNodes);
                    }
                    if cache.len() >= 2
                        && document_json_bytes(cache.total_json_bytes, cache.len()) > max_bytes
                    {
                        return Err(BodyError::TooLarge);
                    }
                }
                Child::Text(_) => {
                    return Err(BodyError::InvalidNode {
                        path: format!("/content/{index}"),
                        problem: "a text stands at the root",
                    });
                }
                Child::Other => {
                    return Err(BodyError::InvalidNode {
                        path: format!("/content/{index}"),
                        problem: "shared content is neither a node nor a text",
                    });
                }
            }
        }
        cache.check_whole_limits()?;
        Ok(cache)
    }

    /// Updates `self` incrementally from `delta` after an in-place update on `doc`.
    ///
    /// Returns `Ok(())` if the updated document is a valid body within all allowlist,
    /// node, depth, table, and `BODY_MAX_BYTES` limits. Returns `Err(dirty)` with the
    /// set of dirty top-level block IDs if any block failed validation, the root child
    /// count no longer matches the cached elements, or whole-document limits were
    /// exceeded — signaling that incremental repair should run.
    pub(crate) fn apply_delta(
        &mut self,
        doc: &Doc,
        delta: &ApplyDelta,
    ) -> Result<(), BTreeSet<ID>> {
        let txn = doc.transact();
        let Some(root) = txn.get_xml_fragment(FIELD) else {
            self.clear();
            return Ok(());
        };

        // 1. Drop top-level blocks deleted in this transaction (O(log N + deleted)).
        for (&client, ranges) in delta.delete_set.iter() {
            for range in ranges.iter() {
                self.remove_range(client, range.start, range.end);
            }
        }

        // 2. Collect dirty top-level block IDs: newly inserted live root elements and
        //    existing cached blocks whose subtrees or attributes changed.
        let mut dirty = BTreeSet::new();
        for id in &delta.new_types {
            let Some(branch) = BranchID::get_nested(&txn, id) else {
                continue;
            };
            if branch.is_deleted() {
                continue;
            }
            if let Ok(XmlOut::Element(element)) = XmlOut::try_from(branch)
                && is_direct_child_of_root(&element, &root)
            {
                dirty.insert(*id);
            }
        }
        for id in &delta.changed_branches {
            if self.contains(id) {
                dirty.insert(*id);
            }
        }

        // 3. Re-project and validate only the dirty top-level blocks.
        for &id in &dirty {
            let Some(branch) = BranchID::get_nested(&txn, &id) else {
                return Err(dirty);
            };
            let Ok(XmlOut::Element(element)) = XmlOut::try_from(branch) else {
                return Err(dirty);
            };
            let Ok(entry) = project_and_validate_block(&txn, &element) else {
                return Err(dirty);
            };
            self.upsert(id, entry);
        }

        // 4. Verify root integrity (no stray text/fragment/non-XML countable items AND no
        //    non-countable `ItemContent::Format` shadowing `root.first_child()` when
        //    `root_len > 0`) and enforce whole-document limits.
        let root_len = usize::try_from(root.len(&txn)).unwrap_or(usize::MAX);
        if self.len() != root_len
            || (root_len > 0 && root.first_child().is_none())
            || self.check_whole_limits().is_err()
        {
            return Err(dirty);
        }
        Ok(())
    }

    /// Returns the deduplicated stored file names referenced by `image` and `file` nodes
    /// in exact document order, matching [`Body::file_ids`].
    #[must_use]
    pub(crate) fn file_ids_in_order(&self, doc: &Doc) -> Vec<String> {
        match self.file_ids.len() {
            0 => Vec::new(),
            1 => self
                .file_ids
                .values()
                .next()
                .map_or_else(Vec::new, |ids| ids.to_vec()),
            total_file_blocks => {
                let txn = doc.transact();
                let Some(root) = txn.get_xml_fragment(FIELD) else {
                    return Vec::new();
                };
                let mut seen: HashSet<&str> = HashSet::new();
                let mut out: Vec<String> = Vec::new();
                let mut remaining = total_file_blocks;
                let mut next = root.first_child();
                while let Some(node) = next {
                    let following = match &node {
                        XmlOut::Element(element) => element.siblings(&txn).next(),
                        XmlOut::Text(text) => text.siblings(&txn).next(),
                        XmlOut::Fragment(_) => None,
                    };
                    if let XmlOut::Element(element) = node
                        && let Some(id) = element_id(&element)
                        && let Some(ids) = self.file_ids.get(&id)
                    {
                        for file_id in ids {
                            if seen.insert(file_id.as_str()) {
                                out.push(file_id.clone());
                            }
                        }
                        remaining -= 1;
                        if remaining == 0 {
                            break;
                        }
                    }
                    next = following;
                }
                out
            }
        }
    }
}

/// Projects and validates a single top-level `XmlElement` at depth 2 using
/// [`fvoci_model::Body::parse`], returning its compact summary and file IDs while
/// immediately dropping the temporary JSON tree.
pub(crate) fn project_and_validate_block<T: ReadTxn>(
    txn: &T,
    element: &XmlElementRef,
) -> Result<BlockEntry, BodyError> {
    let (value, nodes, has_empty_row_text) = project_element(txn, element)?;
    let wrapped = json!({"type": "doc", "content": [value]});
    let parsed = Body::parse(wrapped)?;
    match parsed {
        None => Ok(BlockEntry {
            summary: BlockSummary {
                nodes: 1,
                json_bytes: u32::try_from(EMPTY_PARAGRAPH_BYTES).unwrap_or(u32::MAX),
                is_empty_paragraph: true,
                has_empty_row_text,
            },
            file_ids: Box::new([]),
        }),
        Some(body) => {
            let block_bytes = json_size_of(body.document()).saturating_sub(DOC_WRAPPER_BYTES);
            let file_ids: Box<[String]> = body.file_ids().into_iter().map(str::to_owned).collect();
            Ok(BlockEntry {
                summary: BlockSummary {
                    nodes: u32::try_from(nodes).unwrap_or(u32::MAX),
                    json_bytes: u32::try_from(block_bytes).unwrap_or(u32::MAX),
                    is_empty_paragraph: false,
                    has_empty_row_text,
                },
                file_ids,
            })
        }
    }
}

/// Exact compact JSON byte length of `{"content":[...],"type":"doc"}` for `block_count`
/// non-empty blocks whose individual compact JSON lengths sum to `sum_block_bytes`.
#[must_use]
fn document_json_bytes(sum_block_bytes: u64, block_count: usize) -> u64 {
    let wrapper = u64::try_from(DOC_WRAPPER_BYTES).unwrap_or(u64::MAX);
    let commas = u64::try_from(block_count.saturating_sub(1)).unwrap_or(u64::MAX);
    wrapper
        .saturating_add(sum_block_bytes)
        .saturating_add(commas)
}

/// Whole-document aggregate counts checked against `BODY_MAX_NODES` and `BODY_MAX_BYTES`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DocTotals {
    pub(crate) blocks: usize,
    pub(crate) nodes: u64,
    pub(crate) json_bytes: u64,
    pub(crate) is_empty_doc: bool,
}

/// Enforces `BODY_MAX_NODES` and `BODY_MAX_BYTES` in the exact order of [`Body::parse`].
pub(crate) fn check_totals(totals: DocTotals) -> Result<(), BodyError> {
    if totals.blocks == 0 {
        return Ok(());
    }
    let max_nodes = u64::try_from(BODY_MAX_NODES).unwrap_or(u64::MAX);
    if 1_u64.saturating_add(totals.nodes) > max_nodes {
        return Err(BodyError::TooManyNodes);
    }
    if totals.is_empty_doc {
        return Ok(());
    }
    let max_bytes = u64::try_from(BODY_MAX_BYTES).unwrap_or(u64::MAX);
    if document_json_bytes(totals.json_bytes, totals.blocks) > max_bytes {
        return Err(BodyError::TooLarge);
    }
    Ok(())
}

/// Counts JSON nodes, exact compact JSON bytes, and empty-paragraph status of a single
/// already-validated top-level JSON block node (used during incremental repair surveying).
#[must_use]
pub(crate) fn measure_block_value(node: &Value) -> (u64, u64, bool) {
    let nodes = count_json_nodes(node);
    let bytes = u64::try_from(json_size_of(node)).unwrap_or(u64::MAX);
    let is_empty_paragraph = node.get("type").and_then(Value::as_str) == Some("paragraph")
        && node
            .get("content")
            .is_none_or(|content| content.as_array().is_some_and(Vec::is_empty));
    (nodes, bytes, is_empty_paragraph)
}

fn count_json_nodes(node: &Value) -> u64 {
    1 + node
        .get("content")
        .and_then(Value::as_array)
        .map_or(0_u64, |children| {
            children
                .iter()
                .map(count_json_nodes)
                .fold(0_u64, u64::saturating_add)
        })
}

fn is_direct_child_of_root(element: &XmlElementRef, root: &XmlFragmentRef) -> bool {
    matches!(element.parent(), Some(XmlOut::Fragment(parent)) if parent == *root)
}

/// Returns the immutable nested [`ID`] of an XML element, or `None` for a root branch.
#[must_use]
pub(crate) fn element_id(element: &XmlElementRef) -> Option<ID> {
    let branch: &Branch = element.as_ref();
    match branch.id() {
        BranchID::Nested(id) => Some(id),
        BranchID::Root(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_wrapper_and_empty_paragraph_byte_constants_match_serde_json() {
        let empty_doc = serde_json::to_vec(&json!({"type": "doc", "content": []})).expect("json");
        assert_eq!(empty_doc.len(), DOC_WRAPPER_BYTES);
        let empty_para = serde_json::to_vec(&json!({"type": "paragraph"})).expect("json");
        assert_eq!(empty_para.len(), EMPTY_PARAGRAPH_BYTES);

        let blocks = [
            json!({"type": "paragraph"}),
            json!({"type": "paragraph", "content": [{"type": "text", "text": "한글😀"}]}),
            json!({"type": "horizontalRule"}),
        ];
        let sum: u64 = blocks
            .iter()
            .map(|b| u64::try_from(json_size_of(b)).expect("fits"))
            .sum();
        let whole =
            u64::try_from(json_size_of(&json!({"type": "doc", "content": blocks}))).expect("fits");
        assert_eq!(whole, document_json_bytes(sum, blocks.len()));
    }
}
