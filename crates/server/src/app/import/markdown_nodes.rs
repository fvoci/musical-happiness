//! `ProseMirror` JSON node builders and allowlist-aligned helper functions for Markdown conversion.

use fvoci_model::{BODY_MAX_BYTES, Body, BodyError};
use pulldown_cmark::HeadingLevel;
use serde_json::{Map, Value, json};

use super::bundle::has_url_scheme;
use super::summary::ImportWarningCode;
use crate::error::AppError;

/// Whether the converted body belongs to a document (card/page) or a comment.
///
/// Comments may not contain `file` blocks (`BlockError::FileBlockInComment`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BodyContext {
    /// Card or wiki page body (all document blocks allowed).
    Document,
    /// Comment body (`file` blocks are emitted as plain links/text instead).
    Comment,
}

/// Resolution result for a Markdown link or image target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ResolvedTarget {
    /// A local image inside the uploaded bundle (`fileId` placeholder).
    LocalImage { file_id: String },
    /// A local non-image file inside the uploaded bundle (`fileId` placeholder).
    LocalFile { file_id: String },
    /// An internal link to another imported block (valid `UUIDv7` block ID).
    InternalReference { block_id: String },
    /// An external URL: NEVER fetched; preserved only as a link if allowed.
    ExternalUrl,
    /// A relative target not present in the bundle (or traversing outside it).
    MissingLocal,
}

/// Resolves Markdown link and image targets against the uploaded bundle.
pub(crate) trait AssetResolver {
    /// Resolves an image destination (`![alt](dest)`).
    fn resolve_image(&mut self, dest: &str) -> ResolvedTarget;

    /// Resolves a link destination (`[label](dest)`).
    fn resolve_link(&mut self, dest: &str) -> ResolvedTarget;
}

/// Resolver used when there are no local bundle assets (e.g. Trello JSON).
#[derive(Debug, Default)]
pub(crate) struct ExternalOnlyResolver;

impl AssetResolver for ExternalOnlyResolver {
    fn resolve_image(&mut self, dest: &str) -> ResolvedTarget {
        if has_url_scheme(dest.trim()) {
            ResolvedTarget::ExternalUrl
        } else {
            ResolvedTarget::MissingLocal
        }
    }

    fn resolve_link(&mut self, dest: &str) -> ResolvedTarget {
        if has_url_scheme(dest.trim()) {
            ResolvedTarget::ExternalUrl
        } else {
            ResolvedTarget::MissingLocal
        }
    }
}

/// Non-fatal conversion warning produced while converting Markdown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MarkdownNotice {
    pub(crate) code: ImportWarningCode,
    pub(crate) reason: String,
}

/// Output of converting Markdown into block nodes.
#[derive(Debug, Default)]
pub(crate) struct MarkdownBlocks {
    pub(crate) nodes: Vec<Value>,
    pub(crate) notices: Vec<MarkdownNotice>,
}

/// Maximum UTF-16 code units allowed in an `image` node's `alt` attribute (`allowlist.rs`).
const IMAGE_ALT_MAX_UTF16: usize = 1000;

/// Maximum UTF-16 code units allowed in a `link` mark's `href` attribute (`allowlist.rs`).
const HREF_MAX_UTF16: usize = 2048;

/// Allowed URL schemes for links (`allowlist.rs`).
const ALLOWED_SCHEMES: [&str; 3] = ["http", "https", "mailto"];

/// An inline element collected inside a paragraph, heading, link, or table cell.
#[derive(Debug, Clone)]
pub(crate) enum InlineItem {
    /// A `ProseMirror` inline node (`text`, `hardBreak`, `reference`).
    Node(Value),
    /// A block node (`image` or `file`) that must split out of its enclosing paragraph.
    BlockBreak(Value),
}

/// Wraps block nodes into a validated [`Body`], returning `None` if empty.
///
/// # Errors
///
/// Returns a 413 error if the serialized body exceeds `BODY_MAX_BYTES`, or a 400 error
/// for any other [`BodyError`].
pub(crate) fn blocks_to_body(nodes: Vec<Value>) -> Result<Option<Body>, AppError> {
    if nodes.is_empty() {
        return Ok(None);
    }
    let mut doc = Map::with_capacity(2);
    doc.insert("type".to_owned(), Value::String("doc".to_owned()));
    doc.insert("content".to_owned(), Value::Array(nodes));
    Body::parse(Value::Object(doc)).map_err(|error| match error {
        BodyError::TooLarge => {
            AppError::too_large(error, u64::try_from(BODY_MAX_BYTES).unwrap_or(u64::MAX))
        }
        other => AppError::bad_request(other),
    })
}

/// Builds a simple plain-text paragraph node.
#[must_use]
pub(crate) fn text_paragraph(text: &str) -> Value {
    if text.is_empty() {
        json!({"type": "paragraph"})
    } else {
        json!({"type": "paragraph", "content": [{"type": "text", "text": text}]})
    }
}

/// Builds a heading node of `level` (clamped to 1..=3) with plain text.
#[must_use]
pub(crate) fn text_heading(level: u8, text: &str) -> Value {
    let clamped = level.clamp(1, 3);
    if text.is_empty() {
        json!({"type": "heading", "attrs": {"level": clamped}})
    } else {
        json!({
            "type": "heading",
            "attrs": {"level": clamped},
            "content": [{"type": "text", "text": text}],
        })
    }
}

/// Builds a text node with a validated `link` mark if `href` is allowed, or plain text.
#[must_use]
pub(crate) fn linked_text_node(label: &str, href: &str) -> (Value, bool) {
    let shown = if label.is_empty() { href } else { label };
    if is_allowed_href(href) {
        (
            json!({
                "type": "text",
                "text": shown,
                "marks": [{"type": "link", "attrs": {"href": href}}],
            }),
            true,
        )
    } else {
        (json!({"type": "text", "text": shown}), false)
    }
}

/// Whether `href` is permitted by `fvoci_model::allowlist` (`http`, `https`, `mailto`,
/// or a safe relative path without `//` or `\`, at most 2048 UTF-16 code units).
#[must_use]
pub(crate) fn is_allowed_href(href: &str) -> bool {
    let utf16 = href.encode_utf16().count();
    if utf16 == 0 || utf16 > HREF_MAX_UTF16 {
        return false;
    }
    let compact: String = href
        .chars()
        .filter(|ch| !matches!(ch, '\u{0}'..=' ' | '\u{7f}'))
        .collect();
    if compact.is_empty() {
        return false;
    }
    match compact.split_once(':') {
        Some((scheme, _)) if is_valid_scheme(scheme) => ALLOWED_SCHEMES
            .iter()
            .any(|allowed| scheme.eq_ignore_ascii_case(allowed)),
        _ => !compact.starts_with("//") && !compact.contains('\\'),
    }
}

fn is_valid_scheme(scheme: &str) -> bool {
    let mut chars = scheme.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_alphabetic()
        && chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '.' | '-'))
}

pub(crate) fn is_valid_language(text: &str) -> bool {
    (1..=40).contains(&text.len())
        && text
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '#' | '.' | '_' | '-'))
}

pub(crate) fn heading_level_num(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// Emits active formatting marks in deterministic order (`bold`, `italic`, `strike`) in O(1).
pub(crate) fn active_mark_values(counts: [usize; 3]) -> Vec<Value> {
    const NAMES: [&str; 3] = ["bold", "italic", "strike"];
    let mut out = Vec::new();
    for (count, name) in counts.into_iter().zip(NAMES) {
        if count > 0 {
            out.push(json!({"type": name}));
        }
    }
    out
}

pub(crate) fn image_node(file_id: &str, alt: &str) -> Value {
    let mut attrs = Map::new();
    attrs.insert("fileId".to_owned(), Value::String(file_id.to_owned()));
    let alt_trimmed = truncate_utf16(alt.trim(), IMAGE_ALT_MAX_UTF16);
    if !alt_trimmed.is_empty() {
        attrs.insert("alt".to_owned(), Value::String(alt_trimmed));
    }
    json!({"type": "image", "attrs": attrs})
}

fn truncate_utf16(text: &str, max_units: usize) -> String {
    let mut out = String::new();
    let mut units = 0;
    for ch in text.chars() {
        let len = ch.len_utf16();
        if units + len > max_units {
            break;
        }
        out.push(ch);
        units += len;
    }
    out
}

pub(crate) fn apply_link_mark(item: InlineItem, link_mark: &Value) -> InlineItem {
    let InlineItem::Node(mut node) = item else {
        return item;
    };
    if node.get("type").and_then(Value::as_str) != Some("text") {
        return InlineItem::Node(node);
    }
    let Some(obj) = node.as_object_mut() else {
        return InlineItem::Node(node);
    };
    let marks = obj
        .entry("marks")
        .or_insert_with(|| Value::Array(Vec::new()));
    if let Some(list) = marks.as_array_mut() {
        let has_code_or_link = list.iter().any(|existing| {
            matches!(
                existing.get("type").and_then(Value::as_str),
                Some("code" | "link")
            )
        });
        if !has_code_or_link {
            list.push(link_mark.clone());
        }
    }
    InlineItem::Node(node)
}

pub(crate) fn partition_inlines(inlines: Vec<InlineItem>) -> (Vec<Value>, Vec<Value>) {
    let mut nodes = Vec::new();
    let mut blocks = Vec::new();
    for item in inlines {
        match item {
            InlineItem::Node(node) => nodes.push(node),
            InlineItem::BlockBreak(block) => blocks.push(block),
        }
    }
    (nodes, blocks)
}

pub(crate) fn collect_plain_text(inlines: &[InlineItem]) -> String {
    let mut out = String::new();
    for item in inlines {
        if let InlineItem::Node(node) = item
            && let Some(text) = node.get("text").and_then(Value::as_str)
        {
            out.push_str(text);
        }
    }
    out
}

pub(crate) fn prepend_prefix_to_first_paragraph(blocks: &mut [Value], prefix: &str) {
    let Some(first) = blocks.first_mut().and_then(Value::as_object_mut) else {
        return;
    };
    let mut existing = first
        .remove("content")
        .and_then(|value| match value {
            Value::Array(items) => Some(items),
            _ => None,
        })
        .unwrap_or_default();
    existing.insert(0, json!({"type": "text", "text": prefix}));
    first.insert(
        "content".to_owned(),
        Value::Array(merge_text_nodes(existing)),
    );
}

/// Merges adjacent `text` nodes sharing identical `marks` in amortized O(N) time by
/// appending into the previous `Value::String` in place.
pub(crate) fn merge_text_nodes(nodes: Vec<Value>) -> Vec<Value> {
    let mut merged: Vec<Value> = Vec::with_capacity(nodes.len());
    for node in nodes {
        if node.get("type").and_then(Value::as_str) == Some("text") {
            let text = node.get("text").and_then(Value::as_str).unwrap_or("");
            if text.is_empty() {
                continue;
            }
            let same_marks = merged.last().is_some_and(|prev| {
                prev.get("type").and_then(Value::as_str) == Some("text")
                    && prev.get("marks") == node.get("marks")
            });
            if same_marks
                && let Some(Value::String(prev_text)) = merged
                    .last_mut()
                    .and_then(Value::as_object_mut)
                    .and_then(|obj| obj.get_mut("text"))
            {
                prev_text.push_str(text);
                continue;
            }
        }
        merged.push(node);
    }
    merged
}
