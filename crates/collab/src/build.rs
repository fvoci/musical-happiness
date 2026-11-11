//! Making the Y document of a stored body.

use std::sync::Arc;

use fvoci_model::Body;
use serde_json::Value;
use yrs::types::Attrs;
use yrs::{
    Any, ClientID, Doc, Number, OffsetKind, Options, Text, Transact, TransactionMut, Xml,
    XmlElementPrelim, XmlFragment, XmlTextPrelim,
};

use crate::FIELD;
use crate::schema::{self, Fallback};

/// An empty Y document for a body, whose root is the fragment the editors are bound to.
/// A card without a body starts from this: an editor shows its empty paragraph over an
/// empty fragment and writes when somebody types.
#[must_use]
pub fn new_doc() -> Doc {
    doc_of_client(None)
}

/// A Y document that counts the places in a text as the editors do, in UTF-16 code units.
pub(crate) fn doc_of_client(client: Option<u64>) -> Doc {
    let options = Options {
        offset_kind: OffsetKind::Utf16,
        ..client.map_or_else(Options::default, |id| {
            Options::with_client_id(ClientID::new(id))
        })
    };
    let doc = Doc::with_options(options);
    doc.get_or_insert_xml_fragment(FIELD);
    doc
}

/// Makes the Y document of a stored body, in the shape the editor's binding makes of it:
/// an element per node named for its type, the neighbouring text nodes of a line in one
/// text with their marks as its formatting, and an attribute the body leaves out
/// written with the value an editor gives it.
///
/// The server makes it when a body is first opened for editing together; no editor does.
/// The body is written by a client whose identifier is taken at random, anew each
/// time: an update made against another document, or against an earlier making of
/// this one, builds on changes this document does not have and is refused
/// ([`crate::UpdateError::MissingChanges`]) instead of landing somewhere in it.
#[must_use]
pub fn from_body(body: &Body) -> Doc {
    // 32 bits, as the editors take theirs.
    from_body_by(body, ClientID::random().get() & 0xffff_ffff)
}

/// [`from_body`] with the identifier of the writing client given. The same body and
/// writer give the same document, byte for byte; two documents by the same writer take
/// each other's updates as their own, which is why [`from_body`] gives each its own.
#[must_use]
pub fn from_body_by(body: &Body, writer: u64) -> Doc {
    let doc = doc_of_client(Some(writer));
    {
        let root = doc.get_or_insert_xml_fragment(FIELD);
        let mut txn = doc.transact_mut();
        push_nodes(&mut txn, &root, content_of(body.document()));
    }
    doc
}

/// The content of a node; none if it has none.
fn content_of(node: &Value) -> &[Value] {
    node.get("content")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

fn is_text(node: &Value) -> bool {
    node.get("type").and_then(Value::as_str) == Some("text")
}

/// Fills an empty parent with nodes: an element for each node that is no text, one
/// text for each stretch of neighbouring text nodes.
///
/// The last goes in first and each one before it at the front: the library finds the
/// front of a parent at once, and its end only by walking to it, which for a parent of
/// twenty thousand children is two hundred million steps.
fn push_nodes(txn: &mut TransactionMut<'_>, parent: &impl XmlFragment, nodes: &[Value]) {
    let mut rest = nodes;
    while let Some(last) = rest.last() {
        let stretch = if is_text(last) {
            rest.iter().rev().take_while(|node| is_text(node)).count()
        } else {
            1
        };
        let (before, these) = rest.split_at(rest.len() - stretch);
        match these {
            [node] if !is_text(node) => push_element(txn, parent, node),
            texts => push_text(txn, parent, texts),
        }
        rest = before;
    }
}

fn push_element(txn: &mut TransactionMut<'_>, parent: &impl XmlFragment, node: &Value) {
    let kind = node.get("type").and_then(Value::as_str).unwrap_or_default();
    let element = parent.push_front(txn, XmlElementPrelim::empty(kind));
    push_nodes(txn, &element, content_of(node));
    let given = node.get("attrs");
    for attribute in schema::node_attributes(kind).unwrap_or(&[]) {
        let value = match given.and_then(|attrs| attrs.get(attribute.name)) {
            Some(value) => Some(any_of(value)),
            None => match attribute.fallback {
                Fallback::None => None,
                Fallback::Integer(number) => Some(Any::Number(Number::Int(number))),
                Fallback::Boolean(truth) => Some(Any::Bool(truth)),
            },
        };
        if let Some(value) = value {
            element.insert_attribute(txn, attribute.name, value);
        }
    }
}

/// Puts the text nodes of one stretch at the front of a parent as one text. Each mark
/// is written on its own and in the editor's order, so that what is written does not
/// depend on the order in which a map happens to hand its members out.
fn push_text(txn: &mut TransactionMut<'_>, parent: &impl XmlFragment, nodes: &[Value]) {
    let whole: String = nodes.iter().map(letters_of).collect();
    let text = parent.push_front(txn, XmlTextPrelim::new(whole));
    let mut start = 0_u32;
    for node in nodes {
        let length = utf16_length(letters_of(node));
        let mut marks: Vec<(usize, &str, Any)> = node
            .get("marks")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|mark| {
                let name = mark.get("type")?.as_str()?;
                let attrs = mark.get("attrs").map_or_else(empty_attributes, any_of);
                Some((schema::mark_rank(name).unwrap_or(usize::MAX), name, attrs))
            })
            .collect();
        marks.sort_by(|one, other| (one.0, one.1).cmp(&(other.0, other.1)));
        for (_, name, attrs) in marks {
            let formatting: Attrs = [(Arc::from(name), attrs)].into_iter().collect();
            text.format(txn, start, length, formatting);
        }
        start = start.saturating_add(length);
    }
}

/// The letters of a text node.
fn letters_of(node: &Value) -> &str {
    node.get("text").and_then(Value::as_str).unwrap_or_default()
}

/// The attributes of a mark that has none: an empty object, as the binding writes it.
fn empty_attributes() -> Any {
    Any::Map(Arc::default())
}

/// The length of a text in UTF-16 code units. A body is at most 1 MiB, so it fits.
fn utf16_length(text: &str) -> u32 {
    u32::try_from(text.encode_utf16().count()).unwrap_or(u32::MAX)
}

/// A JSON value as a value of the shared content. A number that is whole is written as
/// a whole number, however the JSON wrote it (`1`, `1.0`, `1e0`): JavaScript has one
/// kind of number, and the editors write a whole one as an integer.
fn any_of(value: &Value) -> Any {
    match value {
        Value::Null => Any::Null,
        Value::Bool(truth) => Any::Bool(*truth),
        Value::Number(number) => Any::Number(number.as_i64().map_or_else(
            || Number::try_i64(number.as_f64().unwrap_or(f64::NAN)),
            Number::Int,
        )),
        Value::String(text) => Any::String(Arc::from(text.as_str())),
        Value::Array(items) => Any::Array(items.iter().map(any_of).collect()),
        Value::Object(members) => Any::Map(Arc::new(
            members
                .iter()
                .map(|(name, member)| (name.clone(), any_of(member)))
                .collect(),
        )),
    }
}
