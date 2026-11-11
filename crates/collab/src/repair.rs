//! Repairing a Y document that the allowlist refuses.

use fvoci_model::{BODY_MAX_DEPTH, Body, BodyError};
use serde_json::{Value, json};
use yrs::updates::decoder::Decode;
use yrs::{
    Array, ArrayRef, Doc, ReadTxn, Transact, TransactionMut, Update, XmlElementRef, XmlFragmentRef,
};

use crate::FIELD;
use crate::project::project;
use crate::read::{self, Child, Content};
use crate::schema::{self, Place};
use crate::update::copy_of;

/// What a repair did.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Repair {
    /// How many nodes and texts were deleted, each with all it held. None: the document
    /// was a body already and is as it was.
    pub deleted: usize,
    /// The deletions as a version 1 update, for the editors that hold the document;
    /// `None` if nothing was deleted.
    pub update: Option<Vec<u8>>,
    /// The body of the document as it now is; `None` for no body.
    pub body: Option<Body>,
}

/// Deletes from a Y document what keeps it from being a body, and gives the body that
/// is left.
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
/// The allowlist refuses more than the editor's schema, which checks no value: a
/// heading of level 9 or a link to `javascript:` stays in an editor and is deleted here.
/// At the root an editor wraps a node the document may not hold (a list entry, a
/// mention) instead of deleting it; this deletes it, and a text at the root, on which
/// an editor's binding throws. And a node that holds letters or values where its
/// children belong, which an editor passes over, is deleted whole.
///
/// The deletions are made on a copy of the document first, and made to `doc` only when
/// the copy is a body with them: a repair that fails leaves `doc` as it was.
///
/// # Errors
///
/// Nothing is deleted when an error is returned.
///
/// Returns [`BodyError::TooLarge`], [`BodyError::TooDeep`] or
/// [`BodyError::TooManyNodes`] if what would be left is past a limit, or a node that is
/// judged is past one by itself: deleting what fails is not meant to bring a document
/// under a limit. Returns [`BodyError::InvalidNode`] with an empty path if the root
/// itself holds letters or values (there is no node to delete in their place), if
/// more than a thousand separate stretches of children would have to be deleted (a
/// repair is for what a merge of edits leaves, and each stretch costs a walk through
/// its parent), or if the document cannot be copied, which nothing this crate takes
/// leaves it unable to be. Any other error means the document would still be refused
/// with the deletions.
pub fn repair(doc: &Doc) -> Result<Repair, BodyError> {
    let (repair, copy) = repaired(doc)?;
    if let (Some(_), Some(deletions)) = (copy, &repair.update) {
        // The copy was made from the state of `doc`, so its deletions are deletions of
        // changes `doc` has, and leave it as they left the copy.
        let deletions = Update::decode_v1(deletions).map_err(|_| uncopied())?;
        doc.transact_mut()
            .apply_update(deletions)
            .map_err(|_| uncopied())?;
    }
    Ok(repair)
}

/// [`repair`] without changing `doc`: what the repair does, and the repaired copy of the
/// document. No copy if the document is a body already.
///
/// # Errors
///
/// As [`repair`].
pub(crate) fn repaired(doc: &Doc) -> Result<(Repair, Option<Doc>), BodyError> {
    let as_it_is = {
        let txn = doc.transact();
        txn.get_xml_fragment(FIELD)
            .map_or(Ok(None), |root| project(&txn, &root))
    };
    if let Ok(body) = as_it_is {
        let repair = Repair {
            deleted: 0,
            update: None,
            body,
        };
        return Ok((repair, None));
    }
    let copy = copy_of(doc).map_err(|_| uncopied())?;
    let root = copy.get_or_insert_xml_fragment(FIELD);
    let mut txn = copy.transact_mut();
    let found = Survey { txn: &txn }.root(&root)?;
    // What would be left is judged whole, for the limits, before anything is deleted.
    let left = found.content.into_nodes();
    if !left.is_empty() {
        Body::parse(json!({"type": "doc", "content": left}))?;
    }
    if found.cuts.iter().map(Cuts::stretches).sum::<usize>() > MAX_STRETCHES {
        return Err(BodyError::InvalidNode {
            path: String::new(),
            problem: "more separate parts of the document fail than a repair deletes",
        });
    }
    let deleted = found.cuts.iter().map(|cuts| cuts.indexes.len()).sum();
    for cuts in &found.cuts {
        cuts.make(&mut txn);
    }
    let body = project(&txn, &root)?;
    let update = (deleted > 0).then(|| txn.encode_update_v1());
    drop(txn);
    let repair = Repair {
        deleted,
        update,
        body,
    };
    Ok((repair, Some(copy)))
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
    /// The children to delete, here and below, parent by parent.
    cuts: Vec<Cuts>,
}

impl Found {
    /// Notes the children of a parent that are to be deleted, if there are any.
    fn note(&mut self, parent: &ArrayRef, indexes: Vec<u32>) {
        if !indexes.is_empty() {
            self.cuts.push(Cuts {
                parent: parent.clone(),
                indexes,
            });
        }
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
            let noted = found.cuts.len();
            // A text at the root goes whatever it holds: an editor's binding throws on one.
            let is_node = !matches!(child, Child::Text(_));
            let stays = match self.child(child, 2, &mut found.cuts)? {
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
                found.cuts.truncate(noted);
                own.push(index_of(index));
            }
        }
        found.note(root.as_ref(), own);
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
            match self.child(child, depth, &mut found.cuts)? {
                Some(nodes) => found.content.extend(nodes),
                None => own.push(index_of(index)),
            }
        }
        found.note(element.as_ref(), own);
        Ok(Some(found))
    }

    /// Judges one child at `depth`: its JSON if it stays (a text may be several nodes,
    /// or none), `None` if it is to be deleted. The deletions below a child that stays
    /// are added to `cuts`; those below one that goes are dropped, as they go with it.
    fn child(
        &self,
        child: Child,
        depth: usize,
        cuts: &mut Vec<Cuts>,
    ) -> Result<Option<Vec<Value>>, BodyError> {
        match child {
            Child::Element(element) => {
                if depth > BODY_MAX_DEPTH {
                    return Err(BodyError::TooDeep);
                }
                let Some(below) = self.element(&element, depth + 1)? else {
                    return Ok(None);
                };
                let node = read::element_node(self.txn, &element, below.content.into_nodes());
                if passes(&node, schema::place(element.tag()))? {
                    cuts.extend(below.cuts);
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
/// checked.
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
    };
    match Body::parse(document) {
        Ok(_) => Ok(true),
        Err(BodyError::InvalidNode { .. } | BodyError::NotADocument) => Ok(false),
        Err(limit @ (BodyError::TooLarge | BodyError::TooDeep | BodyError::TooManyNodes)) => {
            Err(limit)
        }
    }
}
