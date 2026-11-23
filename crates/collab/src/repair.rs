//! Repairing a Y document that the allowlist refuses.

use fvoci_model::{BODY_MAX_DEPTH, Body, BodyError};
use serde_json::{Value, json};
use yrs::{Array, ArrayRef, Doc, ReadTxn, Transact, TransactionMut, XmlElementRef, XmlFragmentRef};

use crate::FIELD;
use crate::mend::{self, CellAt, Mend, SurveyedRow};
use crate::project::{project, to_body};
use crate::read::{self, Child, Content};
use crate::schema::{self, Place};
use crate::update::copy_of;

/// What a repair did.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Repair {
    /// How many nodes and texts were deleted, each with all it held.
    pub deleted: usize,
    /// How many cells of tables were put in, or had their spans written again, to mend
    /// the tables' grids (`mend.rs`): by the server alone, as no editor does. With
    /// [`Self::deleted`] 0, the document was a body already and is as it was.
    pub mended: usize,
    /// The deletions and what mended the tables, as a version 1 update for the editors
    /// that hold the document; `None` if nothing was changed.
    pub update: Option<Vec<u8>>,
    /// The body of the document as it now is; `None` for no body.
    pub body: Option<Body>,
}

/// Repairs a Y document: deletes from a copy of it what keeps it from being a body,
/// and gives the body that is left.
///
/// Two edits that are each valid can merge into a document that is not: two people each
/// delete one of the last two entries of a list, and a list without entries is left.
/// Every editor deletes shared content its schema refuses (as long as it is made
/// without its content check, with which it keeps the content and stops editing),
/// and so does this, with the
/// allowlist as the judge: a node that fails by itself is deleted with all it holds (a
/// kind nobody knows, an attribute that is not allowed, too few children, a child of a
/// kind it may not hold, a list entry that does not start with a paragraph), from the
/// bottom up, so that a parent is judged with the children it has left. A text is
/// deleted whole if a run of it fails. Deleting is the same whoever does it and however
/// often, so the editors and the server end with the same document.
///
/// A table is mended, not only deleted from: what the merge of two edits leaves in a
/// table that does not fill its grid (a row a cell short, a span past the last row) is
/// written here, as `mend.rs` says, and the editors are sent it. A cell that fails is
/// deleted as every editor deletes it, and an empty one is put in its place; a row that
/// holds anything but cells goes whole, as in an editor.
///
/// The allowlist refuses more than the editor's schema, which checks no value: a
/// heading of level 9 or a link to `javascript:` stays in an editor and is deleted here.
/// At the root an editor wraps a node the document may not hold (a list entry, a
/// mention) instead of deleting it; this deletes it, and a text at the root, on which
/// an editor's binding throws. And a node that holds letters or values where its
/// children belong, which an editor passes over, is deleted whole.
///
/// The deletions are made on a copy of the document, which comes back with what the
/// repair did; no copy if the document is a body already. `doc` is not changed. What the
/// repair writes is written as a client named by the document ([`writer_for`]): the
/// same document is repaired into the same changes. A server that opens a document
/// takes its logged updates again, each followed by the repair logged after it; the
/// update is repaired as it was, and the logged repair then adds nothing, where one
/// written as a new client would have added its cells to the first's.
///
/// # Errors
///
/// Nothing is deleted when an error is returned.
///
/// Returns [`BodyError::TooLarge`], [`BodyError::TooDeep`], [`BodyError::TooManyNodes`] or
/// [`BodyError::TableTooLarge`] if what would be left is past a limit, or a node that is
/// judged is past one by itself: deleting what fails is not meant to bring a document
/// under a limit. Returns [`BodyError::InvalidNode`] with an empty path if the root
/// itself holds letters or values (there is no node to delete in their place), if
/// more than a thousand separate stretches of children would have to be deleted (a
/// repair is for what a merge of edits leaves, and each stretch costs a walk through
/// its parent), or if the document cannot be copied, which nothing this crate takes
/// leaves it unable to be. Any other error means the document would still be refused
/// with the deletions.
pub(crate) fn repaired(doc: &Doc) -> Result<(Repair, Option<Doc>), BodyError> {
    repaired_by(doc, None)
}

/// The first client a repair writes as: past every client an editor picks, which are
/// below 2³².
const FIRST_WRITER: u64 = 1 << 32;

/// How many clients a repair may write as: those from [`FIRST_WRITER`] up to 2⁵³, past
/// which no client is (an editor in a browser reads one as a number).
const WRITERS: u64 = (1 << 53) - FIRST_WRITER;

/// The client a repair of `doc` writes as, named by what the document holds: where the
/// changes of each of its clients end. Two documents with the same changes are repaired
/// as one client into the same changes; once it has written, the document holds more,
/// and its next repair is another client's.
fn writer_for(doc: &Doc) -> u64 {
    let mut ends: Vec<(u64, u32)> = doc
        .transact()
        .state_vector()
        .iter()
        .map(|(client, end)| (client.get(), *end))
        .collect();
    ends.sort_unstable();
    // FNV-1a over 64 bits: the same in every build and on every machine, which the
    // standard library's hashers are not promised to be.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for (client, end) in ends {
        for byte in client.to_le_bytes().into_iter().chain(end.to_le_bytes()) {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    FIRST_WRITER + hash % WRITERS
}

/// [`repaired`], what it writes written as `writer`, or as the client the document names
/// ([`writer_for`]) if `None`: a test names one to have the same changes from any
/// document.
///
/// # Errors
///
/// As [`repaired`].
pub(crate) fn repaired_by(
    doc: &Doc,
    writer: Option<u64>,
) -> Result<(Repair, Option<Doc>), BodyError> {
    if let Ok(body) = to_body(doc) {
        let repair = Repair {
            deleted: 0,
            mended: 0,
            update: None,
            body,
        };
        return Ok((repair, None));
    }
    let (repair, copy) = deleting(doc, writer)?;
    Ok((repair, Some(copy)))
}

/// Repairs a copy of a document that is no body, and gives what the repair did with the
/// repaired copy. `doc` is not changed.
///
/// # Errors
///
/// As [`repaired`].
fn deleting(doc: &Doc, writer: Option<u64>) -> Result<(Repair, Doc), BodyError> {
    let writer = writer.unwrap_or_else(|| writer_for(doc));
    let copy = copy_of(doc, Some(writer)).map_err(|_| uncopied())?;
    let root = copy.get_or_insert_xml_fragment(FIELD);
    let mut txn = copy.transact_mut();
    let found = Survey { txn: &txn }.root(&root)?;
    // What would be left is judged whole, for the limits, before anything is deleted.
    let left = found.content.into_nodes();
    if !left.is_empty() {
        Body::parse(json!({"type": "doc", "content": left}))?;
    }
    let Changes { cuts, mends } = found.changes;
    if cuts.iter().map(Cuts::stretches).sum::<usize>() > MAX_STRETCHES {
        return Err(BodyError::InvalidNode {
            path: String::new(),
            problem: "more separate parts of the document fail than a repair deletes",
        });
    }
    let deleted = cuts.iter().map(|cuts| cuts.indexes.len()).sum();
    let mended = mends.iter().map(Mend::cells).sum();
    for cuts in &cuts {
        cuts.make(&mut txn);
    }
    // After the deletions, which leave every index a mend counts.
    for mend in &mends {
        mend.make(&mut txn);
    }
    let body = project(&txn, &root)?;
    let update = (deleted > 0 || mended > 0).then(|| txn.encode_update_v1());
    drop(txn);
    let repair = Repair {
        deleted,
        mended,
        update,
        body,
    };
    Ok((repair, copy))
}

/// The refusal of a document that cannot be copied to be repaired.
fn uncopied() -> BodyError {
    BodyError::InvalidNode {
        path: String::new(),
        problem: "the document cannot be read again to be repaired",
    }
}

/// How many stretches of neighbouring children a repair deletes at most. The library
/// deletes by index and walks to it from the front of the parent, over what was deleted
/// before as well, so a repair costs about this many walks through a parent. What two
/// edits leave when they merge is a stretch or two; a document that fails in a thousand
/// separate places was made to.
const MAX_STRETCHES: usize = 1000;

/// The children of one parent to delete: their indexes, in order.
#[derive(Debug)]
struct Cuts {
    parent: ArrayRef,
    indexes: Vec<u32>,
}

impl Cuts {
    /// The stretches of neighbouring children among them: where each begins and how
    /// many it is long.
    fn each_stretch(&self) -> impl Iterator<Item = (u32, u32)> + '_ {
        self.indexes
            .chunk_by(|one, next| one.checked_add(1) == Some(*next))
            .filter_map(|stretch| Some((*stretch.first()?, u32::try_from(stretch.len()).ok()?)))
    }

    fn stretches(&self) -> usize {
        self.each_stretch().count()
    }

    /// Deletes the children, each stretch of neighbours at once, from the last to the
    /// first, so that no deletion moves the place of another.
    fn make(&self, txn: &mut TransactionMut<'_>) {
        let stretches: Vec<(u32, u32)> = self.each_stretch().collect();
        for (start, length) in stretches.into_iter().rev() {
            // The indexes were read from this parent, so the stretch is there;
            // deleting past the end of a parent would panic in the library.
            if start.saturating_add(length) <= self.parent.len(txn) {
                self.parent.remove_range(txn, start, length);
            }
        }
    }
}

/// A walk over shared content that judges every node and notes which to delete,
/// without changing anything.
struct Survey<'txn, T> {
    txn: &'txn T,
}

/// What a survey found below one parent.
#[derive(Debug, Default)]
struct Found {
    /// The JSON of the children that stay.
    content: Content,
    /// What to change, here and below.
    changes: Changes,
}

/// What a repair changes below a node: the children to delete, parent by parent, and
/// the mends of tables, made after them.
#[derive(Debug, Default)]
struct Changes {
    cuts: Vec<Cuts>,
    mends: Vec<Mend>,
}

impl Changes {
    /// Notes the children of a parent that are to be deleted, if there are any.
    fn note(&mut self, parent: &ArrayRef, indexes: Vec<u32>) {
        if !indexes.is_empty() {
            self.cuts.push(Cuts {
                parent: parent.clone(),
                indexes,
            });
        }
    }

    /// Takes on the changes below a node that stays.
    fn take(&mut self, below: Self) {
        self.cuts.extend(below.cuts);
        self.mends.extend(below.mends);
    }

    /// How many changes of each kind there are, to go back to.
    fn mark(&self) -> (usize, usize) {
        (self.cuts.len(), self.mends.len())
    }

    /// Drops what was noted after a mark: the changes below a node that goes.
    fn rewind(&mut self, (cuts, mends): (usize, usize)) {
        self.cuts.truncate(cuts);
        self.mends.truncate(mends);
    }
}

impl<T: ReadTxn> Survey<'_, T> {
    /// Surveys the document. At the root a child must also be one the document may
    /// hold: there is no parent to delete in its place.
    fn root(&self, root: &XmlFragmentRef) -> Result<Found, BodyError> {
        let Some(children) = read::children(self.txn, root) else {
            return Err(BodyError::InvalidNode {
                path: String::new(),
                problem: "the root holds something that is neither a node nor a text",
            });
        };
        let mut found = Found::default();
        let mut own = Vec::new();
        for (index, child) in children.into_iter().enumerate() {
            // What is noted below this child from here on goes with it if it goes.
            let noted = found.changes.mark();
            // A text at the root goes whatever it holds: an editor's binding throws on one.
            let is_node = !matches!(child, Child::Text(_));
            let stays = match self.child(child, 2, &mut found.changes)? {
                // Each was within the limits as the node it is, so this gives no error.
                Some(nodes)
                    if is_node
                        && nodes
                            .iter()
                            .all(|node| passes(node, Place::Block) == Ok(true)) =>
                {
                    found.content.extend(nodes);
                    true
                }
                _ => false,
            };
            if !stays {
                found.changes.rewind(noted);
                own.push(index_of(index));
            }
        }
        found.changes.note(root.as_ref(), own);
        Ok(found)
    }

    /// Surveys the children of an element, which are at `depth`. `None` if the element
    /// holds something that is neither a node nor a text, and so goes whole.
    fn element(&self, element: &XmlElementRef, depth: usize) -> Result<Option<Found>, BodyError> {
        let Some(children) = read::children(self.txn, element) else {
            return Ok(None);
        };
        let mut found = Found::default();
        let mut own = Vec::new();
        for (index, child) in children.into_iter().enumerate() {
            match self.child(child, depth, &mut found.changes)? {
                Some(nodes) => found.content.extend(nodes),
                None => own.push(index_of(index)),
            }
        }
        found.changes.note(element.as_ref(), own);
        Ok(Some(found))
    }

    /// Judges a table at `depth`: its rows, each with what it holds, then its grid,
    /// mended. `None` if it is to be deleted.
    fn table(
        &self,
        table: &XmlElementRef,
        depth: usize,
        changes: &mut Changes,
    ) -> Result<Option<Vec<Value>>, BodyError> {
        let Some(children) = read::children(self.txn, table) else {
            return Ok(None);
        };
        let mut below = Changes::default();
        let mut content = Vec::new();
        let mut rows = Vec::new();
        let mut own = Vec::new();
        for (index, child) in children.into_iter().enumerate() {
            let stays = match child {
                Child::Element(row) if row.tag().as_ref() == "tableRow" => {
                    self.row(row, depth + 1, &mut below)?.map(|(json, row)| {
                        content.push(json);
                        rows.push(row);
                    })
                }
                other => self
                    .child(other, depth + 1, &mut below)?
                    .map(|nodes| content.extend(nodes)),
            };
            if stays.is_none() {
                own.push(index_of(index));
            }
        }
        // Anything but rows that stays makes the table go, as in an editor.
        if content.len() != rows.len() {
            return Ok(None);
        }
        below.note(table.as_ref(), own);
        let node = read::element_node(self.txn, table, content);
        let (node, mends) = mend::mend_grid(node, &rows)?;
        if !passes(&node, Place::Block)? {
            return Ok(None);
        }
        changes.take(below);
        changes.mends.extend(mends);
        Ok(Some(vec![node]))
    }

    /// Judges a row of a table at `depth`: its JSON, with an empty cell where a cell
    /// that fails was, and where its cells come from. `None` if it is to be deleted.
    fn row(
        &self,
        row: XmlElementRef,
        depth: usize,
        changes: &mut Changes,
    ) -> Result<Option<(Value, SurveyedRow)>, BodyError> {
        if depth > BODY_MAX_DEPTH {
            return Err(BodyError::TooDeep);
        }
        let Some(children) = read::children(self.txn, &row) else {
            return Ok(None);
        };
        let mut below = Changes::default();
        let mut content = Vec::new();
        let mut cells = Vec::new();
        let mut own = Vec::new();
        for (index, child) in children.into_iter().enumerate() {
            let cell = match &child {
                Child::Element(element) if is_cell(element.tag()) => Some(element.clone()),
                _ => None,
            };
            match (self.child(child, depth + 1, &mut below)?, cell) {
                (Some(nodes), Some(element)) => {
                    content.extend(nodes);
                    cells.push(CellAt::Kept(element));
                }
                // A text with nothing in it, which an editor passes over, goes: then
                // the row's children are its cells, and a cell put in counts among them.
                (Some(nodes), None) if nodes.is_empty() => own.push(index_of(index)),
                (Some(nodes), None) => content.extend(nodes),
                (None, Some(element)) => {
                    own.push(index_of(index));
                    content.push(read::element_node(
                        self.txn,
                        &element,
                        vec![json!({"type": "paragraph"})],
                    ));
                    cells.push(CellAt::Put);
                }
                (None, None) => own.push(index_of(index)),
            }
        }
        // Anything but cells that stays makes the row go, as in an editor.
        if content.len() != cells.len() {
            return Ok(None);
        }
        below.note(row.as_ref(), own);
        let node = read::element_node(self.txn, &row, content);
        if !passes(&node, Place::TableRow)? {
            return Ok(None);
        }
        changes.take(below);
        Ok(Some((
            node,
            SurveyedRow {
                element: row,
                cells,
            },
        )))
    }

    /// Judges one child at `depth`: its JSON if it stays (a text may be several nodes,
    /// or none), `None` if it is to be deleted. The deletions below a child that stays
    /// are added to `cuts`; those below one that goes are dropped, as they go with it.
    fn child(
        &self,
        child: Child,
        depth: usize,
        changes: &mut Changes,
    ) -> Result<Option<Vec<Value>>, BodyError> {
        match child {
            Child::Element(element) => {
                if depth > BODY_MAX_DEPTH {
                    return Err(BodyError::TooDeep);
                }
                if element.tag().as_ref() == "table" {
                    return self.table(&element, depth, changes);
                }
                let Some(below) = self.element(&element, depth + 1)? else {
                    return Ok(None);
                };
                let node = read::element_node(self.txn, &element, below.content.into_nodes());
                if passes(&node, schema::place(element.tag()))? {
                    changes.take(below.changes);
                    Ok(Some(vec![node]))
                } else {
                    Ok(None)
                }
            }
            Child::Text(text) => {
                let Some(nodes) = read::text_nodes(self.txn, &text) else {
                    return Ok(None);
                };
                if !nodes.is_empty() && depth > BODY_MAX_DEPTH {
                    return Err(BodyError::TooDeep);
                }
                for node in &nodes {
                    if !passes(node, Place::Line)? {
                        return Ok(None);
                    }
                }
                Ok(Some(nodes))
            }
            Child::Other => Ok(None),
        }
    }
}

/// The index of a child. A Y document has no more children under one parent than fit.
fn index_of(index: usize) -> u32 {
    u32::try_from(index).unwrap_or(u32::MAX)
}

/// Whether the allowlist takes a node by itself, with what it holds: the node is put
/// where a node of its kind stands, in a document of nothing else, and that document is
/// checked. A row or a cell is put in a table whose grid it fills: its cells' spans are
/// left out, and a row is given one more cell, so that only what it is and holds is
/// judged; where it falls in its own table is for the table's mending.
///
/// # Errors
///
/// Returns the error if the node alone is past a limit of size, depth or nodes: that is
/// no fault of the node's that deleting it would mend.
fn passes(node: &Value, place: Place) -> Result<bool, BodyError> {
    let around = |kind: &str, inner: Value| json!({"type": kind, "content": [inner]});
    let document = match place {
        Place::Document => node.clone(),
        Place::Block => around("doc", node.clone()),
        Place::Line => around("doc", around("paragraph", node.clone())),
        Place::ListEntry => around("doc", around("bulletList", node.clone())),
        Place::TaskEntry => around("doc", around("taskList", node.clone())),
        Place::TableRow => around("doc", around("table", filling_row(node))),
        Place::TableCell => around(
            "doc",
            around("table", around("tableRow", without_spans(node))),
        ),
    };
    match Body::parse(document) {
        Ok(_) => Ok(true),
        Err(BodyError::InvalidNode { .. } | BodyError::NotADocument) => Ok(false),
        Err(
            limit @ (BodyError::TooLarge
            | BodyError::TooDeep
            | BodyError::TooManyNodes
            | BodyError::TableTooLarge { .. }),
        ) => Err(limit),
    }
}

/// Whether an element of this name is a cell of a table.
fn is_cell(tag: &str) -> bool {
    matches!(tag, "tableCell" | "tableHeader")
}

/// A cell without its spans.
fn without_spans(cell: &Value) -> Value {
    let mut cell = cell.clone();
    if let Some(attrs) = cell.get_mut("attrs").and_then(Value::as_object_mut) {
        attrs.remove("colspan");
        attrs.remove("rowspan");
    }
    cell
}

/// A row whose cells are without their spans, given an empty cell if it has none: a row
/// that fills a table of its own, and no wider than it is.
fn filling_row(row: &Value) -> Value {
    let mut row = row.clone();
    if let Some(members) = row.as_object_mut() {
        let content = members
            .entry("content")
            .or_insert_with(|| Value::Array(Vec::new()));
        if let Some(children) = content.as_array_mut() {
            for child in children.iter_mut() {
                *child = without_spans(child);
            }
            if children.is_empty() {
                children.push(mend::empty_cell("tableCell"));
            }
        }
    }
    row
}
