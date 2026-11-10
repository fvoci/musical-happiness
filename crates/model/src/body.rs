//! The body of a card or a comment: one document of the web app's editor, stored whole.
//!
//! A body is checked when it is read from a request, before it is stored: its shape, its
//! size, how deep it nests and how many nodes it has. A document that is no more than an
//! empty paragraph is no body at all, and is stored as none.

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use thiserror::Error;

/// The version of the set of nodes and marks a stored body is written in.
pub const BODY_VERSION: i64 = 1;

/// The largest body, as serialized JSON, in bytes.
pub const BODY_MAX_BYTES: usize = 1024 * 1024;

/// How deep a body may nest; the document itself is at depth one.
pub const BODY_MAX_DEPTH: usize = 32;

/// How many nodes a body may have, the document itself included.
pub const BODY_MAX_NODES: usize = 20_000;

/// The members a node may have. Anything else, and any member that is `null`, is
/// refused.
const NODE_MEMBERS: [&str; 5] = ["type", "attrs", "content", "marks", "text"];

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
        if value.is_null() || is_empty_document(&value) {
            return Ok(None);
        }
        check_document(&value)?;
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

/// Whether the document is no more than one empty paragraph.
fn is_empty_document(value: &Value) -> bool {
    let Some(content) = value.get("content").and_then(Value::as_array) else {
        return false;
    };
    let [only] = content.as_slice() else {
        return false;
    };
    value.get("type").and_then(Value::as_str) == Some("doc")
        && only.get("type").and_then(Value::as_str) == Some("paragraph")
        && only.as_object().is_some_and(|node| {
            node.keys().all(|key| key == "type" || key == "content")
                && node
                    .get("content")
                    .is_none_or(|content| content.as_array().is_some_and(Vec::is_empty))
        })
}

/// Checks a document that is to be stored.
fn check_document(value: &Value) -> Result<(), BodyError> {
    let has_content = value
        .get("content")
        .and_then(Value::as_array)
        .is_some_and(|content| !content.is_empty());
    if value.get("type").and_then(Value::as_str) != Some("doc") || !has_content {
        return Err(BodyError::NotADocument);
    }
    let mut nodes = 0;
    check_node(value, 1, "", &mut nodes)?;
    // Serializing a JSON value cannot fail.
    let bytes = serde_json::to_vec(value).map_or(usize::MAX, |json| json.len());
    if bytes > BODY_MAX_BYTES {
        return Err(BodyError::TooLarge);
    }
    Ok(())
}

/// Checks one node and what is below it; `nodes` counts the nodes seen so far.
fn check_node(node: &Value, depth: usize, path: &str, nodes: &mut usize) -> Result<(), BodyError> {
    if depth > BODY_MAX_DEPTH {
        return Err(BodyError::TooDeep);
    }
    *nodes += 1;
    if *nodes > BODY_MAX_NODES {
        return Err(BodyError::TooManyNodes);
    }
    let invalid = |problem| BodyError::InvalidNode {
        path: path.to_owned(),
        problem,
    };
    let Some(members) = node.as_object() else {
        return Err(invalid("a node is an object"));
    };
    for (name, member) in members {
        if !NODE_MEMBERS.contains(&name.as_str()) {
            return Err(invalid(
                "a node has only type, attrs, content, marks and text",
            ));
        }
        if member.is_null() {
            return Err(invalid("a member of a node is not null"));
        }
        if name != "content" && holds_nul(member) {
            return Err(invalid("no text holds the character U+0000"));
        }
    }
    if !members.get("type").is_some_and(Value::is_string) {
        return Err(invalid("a node has a type"));
    }
    if members.get("attrs").is_some_and(|attrs| !attrs.is_object()) {
        return Err(invalid("attrs is an object"));
    }
    if members
        .get("text")
        .is_some_and(|text| text.as_str().is_none_or(str::is_empty))
    {
        return Err(invalid("text is a string that is not empty"));
    }
    if let Some(marks) = members.get("marks") {
        let Some(marks) = marks.as_array() else {
            return Err(invalid("marks is a list"));
        };
        if !marks.iter().all(Value::is_object) {
            return Err(invalid("a mark is an object"));
        }
    }
    if let Some(content) = members.get("content") {
        let Some(children) = content.as_array() else {
            return Err(invalid("content is a list"));
        };
        for (index, child) in children.iter().enumerate() {
            check_node(child, depth + 1, &format!("{path}/content/{index}"), nodes)?;
        }
    }
    Ok(())
}

/// Whether a string or a member name in the value holds U+0000, which PostgreSQL cannot
/// store in `jsonb` (and the web app could not show).
fn holds_nul(value: &Value) -> bool {
    match value {
        Value::String(text) => text.contains('\0'),
        Value::Array(items) => items.iter().any(holds_nul),
        Value::Object(members) => members
            .iter()
            .any(|(name, member)| name.contains('\0') || holds_nul(member)),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}

/// Calls `f` for every node of the document, the document first.
fn visit(node: &Value, f: &mut impl FnMut(&Value)) {
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
