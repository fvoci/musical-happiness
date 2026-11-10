//! The allowlist of a body: which nodes and marks a document may have, with which
//! attributes, and which node may be inside which (`docs/s6-design.md` 5.7, version 2).
//!
//! The web app's validator (`apps/web/src/lib/editor/schema.ts`) is the same list as
//! TypeScript; the shared vectors of `contracts/body-vectors.json` hold the two
//! together. Every length is counted in UTF-16 code units, as the web app counts it.

use std::fmt::Write as _;

use serde_json::{Map, Value};

use crate::body::{BODY_MAX_DEPTH, BODY_MAX_NODES, BodyError};
use crate::id::is_id;

/// The members a node may have. Any other, and any member that is `null`, is refused.
const NODE_MEMBERS: [&str; 5] = ["type", "attrs", "content", "marks", "text"];

/// The members a mark may have.
const MARK_MEMBERS: [&str; 2] = ["type", "attrs"];

/// The nodes that stand as blocks.
const BLOCKS: &[&str] = &[
    "paragraph",
    "heading",
    "bulletList",
    "orderedList",
    "taskList",
    "blockquote",
    "codeBlock",
    "horizontalRule",
    "image",
    "blockMath",
];

/// The nodes that stand inside a line.
const INLINE: &[&str] = &["text", "hardBreak", "mention", "inlineMath"];

/// The longest source of a formula.
const LATEX_MAX: usize = 4000;

/// The longest link.
const HREF_MAX: usize = 2048;

/// The schemes a link may have; a link without one is a path inside the app.
const LINK_SCHEMES: [&str; 3] = ["http", "https", "mailto"];

/// What an attribute's value must be.
#[derive(Debug, Clone, Copy)]
enum Rule {
    /// A string of at most this many UTF-16 code units.
    Text(usize),
    /// A string of one to this many UTF-16 code units.
    NonEmptyText(usize),
    /// A whole number in this range.
    Integer(i32, i32),
    /// `true` or `false`.
    Boolean,
    /// The language of a code block: `^[A-Za-z0-9+#._-]{1,40}$`.
    Language,
    /// A link that the rules of 5.7 allow.
    Href,
    /// An identifier of a user.
    Id,
}

/// An attribute of a node or a mark.
#[derive(Debug, Clone, Copy)]
struct Attribute {
    name: &'static str,
    rule: Rule,
    required: bool,
}

const fn required(name: &'static str, rule: Rule) -> Attribute {
    Attribute {
        name,
        rule,
        required: true,
    }
}

const fn optional(name: &'static str, rule: Rule) -> Attribute {
    Attribute {
        name,
        rule,
        required: false,
    }
}

/// A kind of node.
#[derive(Debug, Clone, Copy)]
struct NodeSpec {
    attrs: &'static [Attribute],
    /// The kinds of node allowed directly inside; empty for a leaf.
    children: &'static [&'static str],
    /// The fewest children the node has.
    min_children: usize,
}

const fn node(
    attrs: &'static [Attribute],
    children: &'static [&'static str],
    min_children: usize,
) -> NodeSpec {
    NodeSpec {
        attrs,
        children,
        min_children,
    }
}

const HEADING: &[Attribute] = &[required("level", Rule::Integer(1, 3))];
const ORDERED_LIST: &[Attribute] = &[optional("start", Rule::Integer(0, 1_000_000_000))];
const TASK_ITEM: &[Attribute] = &[required("checked", Rule::Boolean)];
const CODE_BLOCK: &[Attribute] = &[optional("language", Rule::Language)];
const IMAGE: &[Attribute] = &[
    required("fileId", Rule::NonEmptyText(100)),
    optional("alt", Rule::Text(1000)),
];
const MENTION: &[Attribute] = &[required("userId", Rule::Id)];
const MATH: &[Attribute] = &[required("latex", Rule::NonEmptyText(LATEX_MAX))];
const LINK: &[Attribute] = &[required("href", Rule::Href)];

/// The kind of node of this name, if it is on the list.
fn node_spec(name: &str) -> Option<NodeSpec> {
    const LEAF: &[&str] = &[];
    Some(match name {
        "paragraph" => node(&[], INLINE, 0),
        "heading" => node(HEADING, INLINE, 0),
        "bulletList" => node(&[], &["listItem"], 1),
        "orderedList" => node(ORDERED_LIST, &["listItem"], 1),
        "doc" | "listItem" | "blockquote" => node(&[], BLOCKS, 1),
        "taskList" => node(&[], &["taskItem"], 1),
        "taskItem" => node(TASK_ITEM, BLOCKS, 1),
        "codeBlock" => node(CODE_BLOCK, &["text"], 0),
        "horizontalRule" | "hardBreak" | "text" => node(&[], LEAF, 0),
        "image" => node(IMAGE, LEAF, 0),
        "mention" => node(MENTION, LEAF, 0),
        "inlineMath" | "blockMath" => node(MATH, LEAF, 0),
        _ => return None,
    })
}

/// The attributes of the mark of this name, if it is on the list.
fn mark_spec(name: &str) -> Option<&'static [Attribute]> {
    match name {
        "bold" | "italic" | "strike" | "code" => Some(&[]),
        "link" => Some(LINK),
        _ => None,
    }
}

/// The length of a text as the web app counts it: in UTF-16 code units.
fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

/// Whether a link may be stored: one of [`LINK_SCHEMES`], or a path inside the app.
/// Whitespace and control characters do not hide a scheme: browsers drop them before
/// they read one, so they are left out before deciding (but kept in what is stored).
fn is_allowed_href(href: &str) -> bool {
    let length = utf16_len(href);
    if length == 0 || length > HREF_MAX {
        return false;
    }
    let compact: String = href
        .chars()
        .filter(|c| !matches!(c, '\u{0}'..=' ' | '\u{7f}'))
        .collect();
    if compact.is_empty() {
        return false;
    }
    match scheme(&compact) {
        Some(scheme) => LINK_SCHEMES
            .iter()
            .any(|allowed| scheme.eq_ignore_ascii_case(allowed)),
        None => !compact.starts_with("//") && !compact.contains('\\'),
    }
}

/// The scheme of a link: `^[A-Za-z][A-Za-z0-9+.-]*` before a `:`.
fn scheme(link: &str) -> Option<&str> {
    let (scheme, _) = link.split_once(':')?;
    let mut chars = scheme.chars();
    let first = chars.next()?;
    (first.is_ascii_alphabetic()
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-')))
    .then_some(scheme)
}

/// Whether a code block's language is `^[A-Za-z0-9+#._-]{1,40}$`.
fn is_language(text: &str) -> bool {
    (1..=40).contains(&text.len())
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '#' | '.' | '_' | '-'))
}

/// Whether a JSON number is a whole number in the range, however it is written (`1`,
/// `1.0`, `1e0`), as the web app's `Number.isInteger` takes it.
fn is_whole_number_in(value: &Value, low: i32, high: i32) -> bool {
    value.as_f64().is_some_and(|number| {
        number.fract() == 0.0 && (f64::from(low)..=f64::from(high)).contains(&number)
    })
}

impl Rule {
    fn allows(self, value: &Value) -> bool {
        match self {
            Self::Text(max) => value.as_str().is_some_and(|text| utf16_len(text) <= max),
            Self::NonEmptyText(max) => value
                .as_str()
                .is_some_and(|text| (1..=max).contains(&utf16_len(text))),
            Self::Integer(low, high) => is_whole_number_in(value, low, high),
            Self::Boolean => value.is_boolean(),
            Self::Language => value.as_str().is_some_and(is_language),
            Self::Href => value.as_str().is_some_and(is_allowed_href),
            Self::Id => value.as_str().is_some_and(is_id),
        }
    }
}

/// Checks a document against the allowlist and the limits of depth and nodes.
///
/// # Errors
///
/// Returns [`BodyError::NotADocument`] if the value is not a node of type `doc` with
/// content, and otherwise the first thing that is wrong.
pub(crate) fn check(document: &Value) -> Result<(), BodyError> {
    let has_content = document
        .get("content")
        .and_then(Value::as_array)
        .is_some_and(|content| !content.is_empty());
    if document.get("type").and_then(Value::as_str) != Some("doc") || !has_content {
        return Err(BodyError::NotADocument);
    }
    let mut walk = Walk::default();
    walk.node(document, 1, &["doc"])
}

/// A walk over a document: how many nodes it has seen, and where it is.
#[derive(Debug, Default)]
struct Walk {
    nodes: usize,
    /// The index of each node on the way down from the document.
    path: Vec<usize>,
}

impl Walk {
    /// A refusal of the node the walk is at.
    fn invalid(&self, problem: &'static str) -> BodyError {
        let mut path = String::new();
        for index in &self.path {
            // Writing to a string cannot fail.
            let _ = write!(path, "/content/{index}");
        }
        BodyError::InvalidNode { path, problem }
    }

    fn node(&mut self, value: &Value, depth: usize, allowed: &[&str]) -> Result<(), BodyError> {
        self.nodes += 1;
        if self.nodes > BODY_MAX_NODES {
            return Err(BodyError::TooManyNodes);
        }
        if depth > BODY_MAX_DEPTH {
            return Err(BodyError::TooDeep);
        }
        let Some(members) = value.as_object() else {
            return Err(self.invalid("node is not an object"));
        };
        for (name, member) in members {
            if !NODE_MEMBERS.contains(&name.as_str()) {
                return Err(self
                    .invalid("node has a member other than type, attrs, content, marks and text"));
            }
            if member.is_null() {
                return Err(self.invalid("node has a member that is null"));
            }
            if name != "content" && holds_nul(member) {
                return Err(self.invalid("node holds the character U+0000"));
            }
        }
        let name = members.get("type").and_then(Value::as_str).unwrap_or("");
        let Some(spec) = node_spec(name) else {
            return Err(self.invalid("node type is not on the allowlist"));
        };
        if !allowed.contains(&name) {
            return Err(self.invalid("node may not stand here"));
        }
        check_attributes(members.get("attrs"), spec.attrs)
            .map_err(|problem| self.invalid(problem))?;
        if name == "text" {
            check_text(members).map_err(|problem| self.invalid(problem))?;
        } else if members.contains_key("text") || members.contains_key("marks") {
            return Err(self.invalid("node other than text has text or marks"));
        }
        let children = match members.get("content") {
            None => &[][..],
            Some(Value::Array(children)) => children.as_slice(),
            Some(_) => return Err(self.invalid("content is not a list")),
        };
        if children.len() < spec.min_children {
            return Err(self.invalid("node has too few children"));
        }
        if !children.is_empty() && spec.children.is_empty() {
            return Err(self.invalid("node may have no content"));
        }
        if matches!(name, "listItem" | "taskItem")
            && children.first().and_then(|child| child.get("type"))
                != Some(&Value::from("paragraph"))
        {
            return Err(self.invalid("list item does not start with a paragraph"));
        }
        if name == "codeBlock" && children.iter().any(|child| child.get("marks").is_some()) {
            return Err(self.invalid("text of a code block has marks"));
        }
        for (index, child) in children.iter().enumerate() {
            self.path.push(index);
            self.node(child, depth + 1, spec.children)?;
            self.path.pop();
        }
        Ok(())
    }
}

/// Whether a string or a member name in the value holds U+0000, which PostgreSQL cannot
/// store in `jsonb`.
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

/// Checks the attributes of a node or a mark against what its kind has.
fn check_attributes(attrs: Option<&Value>, specs: &[Attribute]) -> Result<(), &'static str> {
    let empty = Map::new();
    let given = match attrs {
        None => &empty,
        Some(Value::Object(given)) => given,
        Some(_) => return Err("attrs is not an object"),
    };
    for (name, value) in given {
        let Some(spec) = specs.iter().find(|spec| spec.name == name) else {
            return Err("attribute is not on the allowlist");
        };
        if !spec.rule.allows(value) {
            return Err("attribute has a value that is not allowed");
        }
    }
    if specs
        .iter()
        .any(|spec| spec.required && !given.contains_key(spec.name))
    {
        return Err("required attribute is missing");
    }
    Ok(())
}

/// Checks the text and the marks of a text node.
fn check_text(members: &Map<String, Value>) -> Result<(), &'static str> {
    if members
        .get("text")
        .and_then(Value::as_str)
        .is_none_or(str::is_empty)
    {
        return Err("text is empty or not a string");
    }
    let marks = match members.get("marks") {
        None => &[][..],
        Some(Value::Array(marks)) => marks.as_slice(),
        Some(_) => return Err("marks is not a list"),
    };
    let mut seen: Vec<&str> = Vec::with_capacity(marks.len());
    for mark in marks {
        let Some(mark) = mark.as_object() else {
            return Err("mark is not an object");
        };
        if mark
            .iter()
            .any(|(name, value)| !MARK_MEMBERS.contains(&name.as_str()) || value.is_null())
        {
            return Err("mark has a member other than type and attrs, or one that is null");
        }
        let name = mark.get("type").and_then(Value::as_str).unwrap_or("");
        let Some(specs) = mark_spec(name) else {
            return Err("mark type is not on the allowlist");
        };
        check_attributes(mark.get("attrs"), specs)?;
        if seen.contains(&name) {
            return Err("mark is given twice");
        }
        seen.push(name);
    }
    Ok(())
}

/// The words of a document without its structure, for searching: a line per
/// paragraph, heading, code block and formula block, the lines of blocks with no words
/// left out. A hard break is a line break inside its block's line and is kept. A
/// formula in a line gives its source there. A mention gives nothing: the name is not
/// stored.
pub(crate) fn plain_text(document: &Value) -> String {
    let mut lines = Vec::new();
    collect_lines(document, &mut lines);
    lines.retain(|line| !line.is_empty());
    lines.join("\n")
}

/// The text a node gives to the line it is in, pushing the lines of the blocks below.
fn collect_lines(node: &Value, lines: &mut Vec<String>) -> String {
    let latex = || {
        node.get("attrs")
            .and_then(|attrs| attrs.get("latex"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned()
    };
    match node.get("type").and_then(Value::as_str).unwrap_or("") {
        "text" => node
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned(),
        "hardBreak" => "\n".to_owned(),
        "inlineMath" => latex(),
        "blockMath" => {
            lines.push(latex());
            String::new()
        }
        kind => {
            let inner: String = node
                .get("content")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|child| collect_lines(child, lines))
                .collect();
            if matches!(kind, "paragraph" | "heading" | "codeBlock") {
                lines.push(inner);
            }
            String::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_are_judged_as_the_web_app_judges_them() {
        for allowed in [
            "https://example.com",
            "HTTP://example.com",
            "mailto:a@example.com",
            "/boards/1",
            "relative/path",
            "#anchor",
        ] {
            assert!(is_allowed_href(allowed), "{allowed}");
        }
        for refused in [
            "",
            " ",
            "javascript:alert(1)",
            "java\tscript:alert(1)",
            " javascript:x",
            "//host/path",
            "/\\host",
            "data:text/html,x",
            "ftp://example.com",
        ] {
            assert!(!is_allowed_href(refused), "{refused:?}");
        }
        assert!(is_allowed_href(&format!("/{}", "a".repeat(HREF_MAX - 1))));
        assert!(!is_allowed_href(&format!("/{}", "a".repeat(HREF_MAX))));
    }

    #[test]
    fn lengths_are_utf16_code_units() {
        assert_eq!(utf16_len("abc"), 3);
        assert_eq!(utf16_len("한글"), 2);
        assert_eq!(utf16_len("😀"), 2);
    }

    #[test]
    fn languages() {
        for language in ["rust", "c++", "objective-c", "c#", "mermaid", "a.b_c"] {
            assert!(is_language(language), "{language}");
        }
        for language in ["", "two words", "é", &"a".repeat(41)] {
            assert!(!is_language(language), "{language:?}");
        }
    }
}
