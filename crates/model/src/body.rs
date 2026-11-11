//! The body of a card or a comment: one document of the web app's editor, stored whole.
//!
//! A body is checked when it is read from a request, before it is stored, against the
//! allowlist of `allowlist.rs`: its nodes and marks, how they nest, its
//! size, how deep it nests and how many nodes it has. A document that is no more than an
//! empty paragraph is no body at all, and is stored as none.

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::allowlist;

/// The version of the set of nodes and marks a stored body is written in.
pub const BODY_VERSION: i64 = 2;

/// The largest body, as serialized JSON, in bytes.
pub const BODY_MAX_BYTES: usize = 1024 * 1024;

/// How deep a body may nest; the document itself is at depth one.
pub const BODY_MAX_DEPTH: usize = 32;

/// How many nodes a body may have, the document itself included.
pub const BODY_MAX_NODES: usize = 20_000;

/// Why a body is refused.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BodyError {
    /// The value is no document: not an object of type `doc` with content.
    #[error("the body is no document")]
    NotADocument,
    /// The serialized body is larger than [`BODY_MAX_BYTES`].
    #[error("the body is larger than {BODY_MAX_BYTES} bytes")]
    TooLarge,
    /// The body nests deeper than [`BODY_MAX_DEPTH`].
    #[error("the body nests deeper than {BODY_MAX_DEPTH} levels")]
    TooDeep,
    /// The body has more than [`BODY_MAX_NODES`] nodes.
    #[error("the body has more than {BODY_MAX_NODES} nodes")]
    TooManyNodes,
    /// A node is malformed.
    #[error("invalid body node at {path}: {problem}")]
    InvalidNode {
        /// Where the node is, as a JSON pointer.
        path: String,
        /// What is wrong with it.
        problem: &'static str,
    },
}

/// How many task items a body has, and how many of them are checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TaskCount {
    /// The task items.
    pub total: u32,
    /// The task items that are checked.
    pub done: u32,
}

/// What a list of cards says about a card's body, without the body: whether there is
/// one and its task items. The store keeps it beside the body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BodySummary {
    /// Whether there is a body.
    pub has_body: bool,
    /// The task items of the body.
    pub tasks: TaskCount,
}

/// A body that has been checked. Its JSON form is the document as it was received.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Body {
    document: Value,
}

impl Body {
    /// Reads a body from what a client sent: `null` and an empty document are no body.
    ///
    /// # Errors
    ///
    /// Returns a [`BodyError`] if the value is a document that may not be stored.
    pub fn parse(value: Value) -> Result<Option<Self>, BodyError> {
        if value.is_null() {
            return Ok(None);
        }
        allowlist::check(&value)?;
        if is_empty_document(&value) {
            return Ok(None);
        }
        check_size(&value)?;
        Ok(Some(Self { document: value }))
    }

    /// A body as it was stored, which was checked when it was.
    #[must_use]
    pub fn from_stored(document: Value) -> Self {
        Self { document }
    }

    /// The document.
    #[must_use]
    pub fn document(&self) -> &Value {
        &self.document
    }

    /// The words of the body without its structure, for searching: a line per paragraph,
    /// heading, code block and formula block. A mention gives nothing.
    #[must_use]
    pub fn text(&self) -> String {
        allowlist::plain_text(&self.document)
    }

    /// What a list says about the body.
    #[must_use]
    pub fn summary(&self) -> BodySummary {
        BodySummary {
            has_body: true,
            tasks: self.tasks(),
        }
    }

    /// Counts the task items of the body.
    #[must_use]
    pub fn tasks(&self) -> TaskCount {
        let mut count = TaskCount::default();
        visit(&self.document, &mut |node| {
            if node.get("type").and_then(Value::as_str) == Some("taskItem") {
                count.total = count.total.saturating_add(1);
                let checked = node
                    .get("attrs")
                    .and_then(|attrs| attrs.get("checked"))
                    .and_then(Value::as_bool);
                if checked == Some(true) {
                    count.done = count.done.saturating_add(1);
                }
            }
        });
        count
    }

    /// The stored names of the files that the images of the body show, in the order
    /// they come, each once.
    #[must_use]
    pub fn file_ids(&self) -> Vec<&str> {
        let mut ids: Vec<&str> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        visit(&self.document, &mut |node| {
            if let Some(id) = image_file(node)
                && seen.insert(id)
            {
                ids.push(id);
            }
        });
        ids
    }

    /// Points the images of the body at other files: `rename` gives the new name of a
    /// file, or `None` for a file that has none, whose image becomes an empty
    /// paragraph (a body has no image without its file, and the place of a block
    /// stays filled). A copy of a body whose files were copied is made so.
    pub fn rename_files(&mut self, rename: impl Fn(&str) -> Option<String>) {
        rename_in(&mut self.document, &rename);
    }
}

/// The stored name of the file of an image node.
fn image_file(node: &Value) -> Option<&str> {
    if node.get("type").and_then(Value::as_str) != Some("image") {
        return None;
    }
    node.get("attrs")?.get("fileId")?.as_str()
}

/// Renames the file of every image below and at `node`, and turns an image whose file
/// has no new name into an empty paragraph.
fn rename_in(node: &mut Value, rename: &impl Fn(&str) -> Option<String>) {
    if let Some(old_name) = image_file(node) {
        match rename(old_name) {
            Some(new_name) => {
                if let Some(attrs) = node.get_mut("attrs").and_then(Value::as_object_mut) {
                    attrs.insert("fileId".to_owned(), Value::String(new_name));
                }
            }
            None => *node = serde_json::json!({"type": "paragraph"}),
        }
        return;
    }
    if let Some(children) = node.get_mut("content").and_then(Value::as_array_mut) {
        for child in children {
            rename_in(child, rename);
        }
    }
}

impl<'de> Deserialize<'de> for Body {
    /// Reads a body and checks it. An empty document is refused here: where a body may
    /// be absent, the member is optional.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        match Self::parse(value) {
            Ok(Some(body)) => Ok(body),
            Ok(None) => Err(serde::de::Error::custom(
                "an empty body is sent as null, or left out",
            )),
            Err(error) => Err(serde::de::Error::custom(error)),
        }
    }
}

impl utoipa::PartialSchema for Body {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        utoipa::openapi::ObjectBuilder::new()
            .schema_type(utoipa::openapi::schema::Type::Object)
            .description(Some(
                "A document of the editor: a node of type `doc` whose `content` is its \
                 blocks. Which nodes, marks and attributes it may have, and how they \
                 nest, is the allowlist of `docs/s6-design.md` 5.7. At most 1 MiB as \
                 JSON, 32 levels deep and 20,000 nodes.",
            ))
            .into()
    }
}

impl utoipa::ToSchema for Body {}

/// What a patch does to a body. Its JSON form is the member `body` of the patch: left
/// out keeps the body, `null` (or an empty document) takes it away, and a document
/// replaces it.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum BodyChange {
    /// The body stays as it is.
    #[default]
    Keep,
    /// The body is taken away.
    Clear,
    /// The body becomes this one.
    Set(Body),
}

impl BodyChange {
    /// Reads what a client sent as the member `body`: `null` and an empty document take
    /// the body away.
    ///
    /// # Errors
    ///
    /// Returns a [`BodyError`] if the value is a document that may not be stored.
    pub fn parse(value: Value) -> Result<Self, BodyError> {
        Ok(Body::parse(value)?.map_or(Self::Clear, Self::Set))
    }

    /// The body the change sets, if it sets one.
    #[must_use]
    pub fn new_body(&self) -> Option<&Body> {
        match self {
            Self::Set(body) => Some(body),
            Self::Keep | Self::Clear => None,
        }
    }

    /// Whether the patch leaves the body as it is.
    #[must_use]
    pub fn is_keep(&self) -> bool {
        matches!(self, Self::Keep)
    }

    /// The body after the change, for something that had none.
    #[must_use]
    pub fn into_body(self) -> Option<Body> {
        match self {
            Self::Keep | Self::Clear => None,
            Self::Set(body) => Some(body),
        }
    }

    /// Applies the change to a body.
    pub fn apply_to(&self, body: &mut Option<Body>) {
        match self {
            Self::Keep => {}
            Self::Clear => *body = None,
            Self::Set(new) => *body = Some(new.clone()),
        }
    }
}

impl Serialize for BodyChange {
    /// `null` for [`BodyChange::Clear`] and the document for [`BodyChange::Set`];
    /// [`BodyChange::Keep`] is left out by the member's `skip_serializing_if`, and
    /// written as `null` without it.
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Keep | Self::Clear => serializer.serialize_none(),
            Self::Set(body) => body.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for BodyChange {
    /// Reads the member when it is there; a member that is left out is
    /// [`BodyChange::Keep`] by its `default`.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::parse(Value::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// Whether a document the allowlist accepted is no more than one empty paragraph, as
/// the web app's `isEmptyDoc` judges it.
fn is_empty_document(value: &Value) -> bool {
    let Some([only]) = value
        .get("content")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
    else {
        return false;
    };
    only.get("type").and_then(Value::as_str) == Some("paragraph")
        && only
            .get("content")
            .is_none_or(|content| content.as_array().is_some_and(Vec::is_empty))
}

/// Counts the bytes written to it, and stops the writing past [`BODY_MAX_BYTES`].
#[derive(Debug, Default)]
struct ByteCount(usize);

impl std::io::Write for ByteCount {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 += bytes.len();
        if self.0 > BODY_MAX_BYTES {
            return Err(std::io::Error::other("the body is too large"));
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Checks the size of a document that the allowlist accepted, as JSON without
/// whitespace, without making that JSON.
fn check_size(value: &Value) -> Result<(), BodyError> {
    // Writing a JSON value fails only when the count passes the limit.
    serde_json::to_writer(ByteCount::default(), value).map_err(|_| BodyError::TooLarge)
}

/// Calls `f` for every node of the document, the document first.
fn visit<'v>(node: &'v Value, f: &mut impl FnMut(&'v Value)) {
    f(node);
    if let Some(children) = node.get("content").and_then(Value::as_array) {
        for child in children {
            visit(child, f);
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn paragraph(text: &str) -> Value {
        json!({"type": "paragraph", "content": [{"type": "text", "text": text}]})
    }

    fn document(blocks: &[Value]) -> Value {
        json!({"type": "doc", "content": blocks})
    }

    #[test]
    fn null_and_an_empty_paragraph_are_no_body() {
        for empty in [
            Value::Null,
            document(&[json!({"type": "paragraph"})]),
            document(&[json!({"type": "paragraph", "content": []})]),
        ] {
            assert_eq!(Body::parse(empty.clone()), Ok(None), "{empty}");
        }
        let body = Body::parse(document(&[paragraph("x")]))
            .expect("a body")
            .expect("not empty");
        assert_eq!(body.document(), &document(&[paragraph("x")]));
    }

    /// An image whose file has a new name points at it; one whose file has none becomes
    /// an empty paragraph, and the body stays valid.
    #[test]
    fn an_image_without_its_file_becomes_an_empty_paragraph() {
        let image = |file: &str| json!({"type": "image", "attrs": {"fileId": file}});
        let mut body = Body::parse(document(&[
            image("kept.png"),
            json!({"type": "blockquote", "content": [image("lost.png")]}),
            paragraph("x"),
        ]))
        .expect("a body")
        .expect("not empty");
        body.rename_files(|old| (old == "kept.png").then(|| "copy.png".to_owned()));
        let expected = document(&[
            image("copy.png"),
            json!({"type": "blockquote", "content": [{"type": "paragraph"}]}),
            paragraph("x"),
        ]);
        assert_eq!(body.document(), &expected);
        assert!(
            Body::parse(expected).is_ok_and(|body| body.is_some()),
            "still a valid body"
        );
        assert_eq!(body.file_ids(), ["copy.png"]);
    }

    #[test]
    fn what_is_no_document_is_refused() {
        for value in [
            json!("text"),
            json!([]),
            json!({"type": "paragraph", "content": [paragraph("x")]}),
            json!({"type": "doc"}),
            json!({"type": "doc", "content": []}),
        ] {
            assert_eq!(
                Body::parse(value.clone()),
                Err(BodyError::NotADocument),
                "{value}"
            );
        }
    }

    #[test]
    fn malformed_nodes_are_refused() {
        for node in [
            json!({"type": "paragraph", "extra": 1}),
            json!({"type": "paragraph", "attrs": null}),
            json!({"content": []}),
            json!({"type": "text", "text": ""}),
            json!({"type": "text", "text": 1}),
            json!({"type": "paragraph", "marks": [1]}),
            json!({"type": "paragraph", "content": {}}),
            json!(7),
        ] {
            let value = document(&[paragraph("x"), node.clone()]);
            assert!(
                matches!(Body::parse(value), Err(BodyError::InvalidNode { .. })),
                "{node}"
            );
        }
    }

    /// A document nested `depth` levels deep, the document counted.
    fn nested(depth: usize) -> Value {
        let mut node = json!({"type": "paragraph"});
        for _ in 2..depth {
            node = json!({"type": "blockquote", "content": [node]});
        }
        document(&[node])
    }

    #[test]
    fn the_depth_is_limited() {
        assert!(
            Body::parse(nested(BODY_MAX_DEPTH)).is_ok(),
            "{BODY_MAX_DEPTH} levels"
        );
        assert_eq!(
            Body::parse(nested(BODY_MAX_DEPTH + 1)),
            Err(BodyError::TooDeep)
        );
    }

    #[test]
    fn the_nodes_are_limited() {
        // The document and its paragraphs.
        let fits = document(&vec![json!({"type": "horizontalRule"}); BODY_MAX_NODES - 1]);
        assert!(Body::parse(fits).is_ok(), "{BODY_MAX_NODES} nodes");
        let over = document(&vec![json!({"type": "horizontalRule"}); BODY_MAX_NODES]);
        assert_eq!(Body::parse(over), Err(BodyError::TooManyNodes));
    }

    #[test]
    fn the_size_is_limited() {
        // The JSON of the document around the text of its one paragraph.
        let frame = serde_json::to_vec(&document(&[paragraph("")]))
            .expect("JSON")
            .len();
        let fits = "x".repeat(BODY_MAX_BYTES - frame);
        assert!(
            Body::parse(document(&[paragraph(&fits)])).is_ok(),
            "{BODY_MAX_BYTES} bytes"
        );
        let over = "x".repeat(BODY_MAX_BYTES - frame + 1);
        assert_eq!(
            Body::parse(document(&[paragraph(&over)])),
            Err(BodyError::TooLarge)
        );
    }

    #[test]
    fn task_items_are_counted() {
        let item = |checked: bool| json!({"type": "taskItem", "attrs": {"checked": checked}, "content": [paragraph("t")]});
        let body = Body::parse(document(&[
            json!({"type": "taskList", "content": [item(true), item(false), item(true)]}),
        ]))
        .expect("a body")
        .expect("not empty");
        assert_eq!(body.tasks(), TaskCount { total: 3, done: 2 });
    }
}
