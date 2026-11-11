//! Repairing a Y document that the allowlist refuses.

use fvoci_model::{BODY_MAX_DEPTH, BODY_MAX_NODES, Body, BodyError};
use serde_json::{Value, json};
use yrs::{Array, ArrayRef, Doc, ReadTxn, Transact, TransactionMut, XmlElementRef, XmlFragmentRef};

use crate::FIELD;
use crate::project::project;
use crate::read::{self, Child, Content, Pushed};
use crate::schema::{self, Place};

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
/// Every editor deletes shared content its schema refuses, and so does this, with the
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
/// mention) instead of deleting it; this deletes it.
///
/// # Errors
///
/// Returns [`BodyError::TooLarge`], [`BodyError::TooDeep`] or
/// [`BodyError::TooManyNodes`] if the document is past a limit: deleting what fails
/// does not bring a document under a limit, and nothing is deleted then. Any other
/// error means the repaired document was still refused, which is a defect of this
/// crate; the deletions were made.
pub fn repair(doc: &Doc) -> Result<Repair, BodyError> {
    let root = doc.get_or_insert_xml_fragment(FIELD);
    let mut txn = doc.transact_mut();
    if let Ok(body) = project(&txn, &root) {
        return Ok(Repair {
            deleted: 0,
            update: None,
            body,
        });
    }
    let cuts = Survey::new(&txn).root(&root)?;
    let deleted = cuts.len();
    // From the last to the first, so that no deletion moves the place of another.
    for cut in cuts.iter().rev() {
        cut.make(&mut txn);
    }
    let body = project(&txn, &root)?;
    let update = (deleted > 0).then(|| txn.encode_update_v1());
    Ok(Repair {
        deleted,
        update,
        body,
    })
}

/// A child to delete: where it is.
#[derive(Debug)]
struct Cut {
    parent: ArrayRef,
    index: u32,
}

impl Cut {
    /// Deletes the child, if the parent has one at the index. (It has, as the index
    /// was read from it; deleting past the end of a parent would panic in the library.)
    fn make(&self, txn: &mut TransactionMut<'_>) {
        if self.index < self.parent.len(txn) {
            self.parent.remove_range(txn, self.index, 1);
        }
    }
}

/// A walk over shared content that judges every node and notes which to delete,
/// without changing anything.
struct Survey<'txn, T> {
    txn: &'txn T,
    /// The nodes that stay, the document included.
    nodes: usize,
}

/// What a survey found below one parent.
#[derive(Debug, Default)]
struct Found {
    /// The JSON of the children that stay.
    content: Content,
    /// The children to delete, here and below, in the order of the document.
    cuts: Vec<Cut>,
}

impl<'txn, T: ReadTxn> Survey<'txn, T> {
    fn new(txn: &'txn T) -> Self {
        Self { txn, nodes: 1 }
    }

    /// Surveys the document. At the root a child must also be one the document may
    /// hold: there is no parent to delete in its place.
    fn root(mut self, root: &XmlFragmentRef) -> Result<Vec<Cut>, BodyError> {
        let parent: &ArrayRef = root.as_ref();
        let mut found = Found::default();
        for (index, child) in read::children(self.txn, root).into_iter().enumerate() {
            // What is noted below this child from here on goes with it if it goes.
            let (noted, counted) = (found.cuts.len(), self.nodes);
            let stays = match self.child(child, 2, &mut found.cuts)? {
                // Each was within the limits as the node it is, so this gives no error.
                Some(nodes)
                    if nodes
                        .iter()
                        .all(|node| passes(node, Place::Block) == Ok(true)) =>
                {
                    self.keep(nodes, &mut found.content)?;
                    true
                }
                _ => false,
            };
            if !stays {
                self.nodes = counted;
                found.cuts.truncate(noted);
                found.cuts.push(Cut {
                    parent: parent.clone(),
                    index: index_of(index),
                });
            }
        }
        Ok(found.cuts)
    }

    /// Surveys the children of an element, which are at `depth`.
    fn element(&mut self, element: &XmlElementRef, depth: usize) -> Result<Found, BodyError> {
        let parent: &ArrayRef = element.as_ref();
        let mut found = Found::default();
        for (index, child) in read::children(self.txn, element).into_iter().enumerate() {
            match self.child(child, depth, &mut found.cuts)? {
                Some(nodes) => self.keep(nodes, &mut found.content)?,
                None => found.cuts.push(Cut {
                    parent: parent.clone(),
                    index: index_of(index),
                }),
            }
        }
        Ok(found)
    }

    /// Judges one child at `depth`: its JSON if it stays (a text may be several nodes),
    /// `None` if it is to be deleted. The deletions below a child that stays are added
    /// to `cuts`; those below one that goes are dropped, as they go with it.
    fn child(
        &mut self,
        child: Child,
        depth: usize,
        cuts: &mut Vec<Cut>,
    ) -> Result<Option<Vec<Value>>, BodyError> {
        if depth > BODY_MAX_DEPTH {
            return Err(BodyError::TooDeep);
        }
        match child {
            Child::Element(element) => {
                let counted = self.nodes;
                let below = self.element(&element, depth + 1)?;
                let node = read::element_node(self.txn, &element, below.content.into_nodes());
                if passes(&node, schema::place(element.tag()))? {
                    cuts.extend(below.cuts);
                    Ok(Some(vec![node]))
                } else {
                    // What it held does not count: it goes with it.
                    self.nodes = counted;
                    Ok(None)
                }
            }
            Child::Text(text) => {
                let Some(nodes) = read::text_nodes(self.txn, &text) else {
                    return Ok(None);
                };
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

    /// Adds nodes that stay to their parent's content, and counts them.
    fn keep(&mut self, nodes: Vec<Value>, content: &mut Content) -> Result<(), BodyError> {
        for node in nodes {
            if content.push(node) == Pushed::New {
                self.nodes += 1;
                if self.nodes > BODY_MAX_NODES {
                    return Err(BodyError::TooManyNodes);
                }
            }
        }
        Ok(())
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
