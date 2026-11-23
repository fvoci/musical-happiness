//! Taking an update from a client.

use thiserror::Error;
use yrs::updates::decoder::Decode;
use yrs::{ClientID, Doc, Out, ReadTxn, StateVector, Transact, Update};

use crate::FIELD;
use crate::build::doc_of_client;
use crate::placing::{self, Held, Sent};
use crate::reading;
use crate::scan::{self, Refusal};

/// The sizes an update may have and leave.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpdateLimits {
    /// The largest update, in bytes as it arrives.
    pub update_bytes: usize,
    /// The largest Y document an update may leave, in bytes as it is stored (a version
    /// 1 update of the whole document). The documents of this crate keep what was
    /// deleted (they are made without garbage collection), so a document grows with
    /// every edit even where the body does not.
    pub state_bytes: usize,
    /// The largest body an update may leave, in bytes of its stored JSON. Every body is
    /// held to [`fvoci_model::BODY_MAX_BYTES`] as well, so a larger value is that.
    pub body_bytes: usize,
    /// How many steps the Y library may have to take to place the changes of one
    /// update: changes of the document it walks past, and letters of changes it cuts,
    /// 256 a step ([`PLACING_STEPS`] for what a client sends); `None` for no count, for
    /// what was taken before and is read again. With a count, the deletions of each
    /// client must also come in order, without overlapping, as an editor writes them.
    pub placing_steps: Option<u64>,
    /// How many clients the document an update leaves may name ([`MAX_CLIENTS`] for
    /// what a client sends); `None` for no count, for what was taken before.
    pub clients: Option<usize>,
    /// How many changes one client may have in the document an update leaves, deleted
    /// ones too ([`MAX_CLIENT_CHANGES`] for what a client sends); `None` for no count,
    /// for what was taken before.
    pub client_changes: Option<u64>,
    /// How many steps the Y library may take to read the document an update leaves
    /// from its state, as it does for every update after it and every time the
    /// document is opened ([`READING_STEPS`]); `None` for no count. The steps are
    /// counted from the state before the library reads it.
    pub reading_steps: Option<u64>,
}

/// How many clients a document an update a client sends leaves may name
/// ([`UpdateLimits::clients`]). The Y library reads the changes of a state client by
/// client and looks for each among the others, so that reading a state takes the
/// square of the clients it names: a letter typed into a document of 10,000 clients is
/// taken in 0.1 s, of 20,000 in 0.9 s and of 40,000 in 4.9 s (release build). An
/// editor takes a client for each time it opens the document, so a document grows to
/// it only in years; what holds one makes it again from its body, with one client,
/// well before.
pub const MAX_CLIENTS: usize = 16 * 1024;

/// How many changes one client may have in a document an update a client sends leaves,
/// deleted ones too ([`UpdateLimits::client_changes`]). The Y library keeps the changes
/// of a client in one array, and moves every later one of them each time it cuts one in
/// two: 17,500 cuts of one letter each, as many as an update of 64 KiB holds, took 3 s
/// where the client cut has 250,000 changes, 6.9 s at 520,000 and 26 s at 1,170,000
/// (release build, an idle machine), and 1.1 s where as many were spread over a hundred
/// clients. A body within its limits makes at most about 323,000, all of one client (of
/// the bodies measured: letters of five marks in turn with line breaks), 0.82 of the
/// three quarters at which what holds a document makes it again from its body, well
/// before an editor brings a client to this.
pub const MAX_CLIENT_CHANGES: u64 = 512 * 1024;

/// How many steps the Y library may take to place the changes of one update a client
/// sends ([`UpdateLimits::placing_steps`]). A step past a change takes 50 to 110 ns
/// where the update is read once, and is taken again every time the library reads
/// the document again, where the change is of a client below the others of its list:
/// as the update is taken, as what it leaves is read again, and as it is repaired. An
/// update of 3 million steps of those, repaired, was taken in 1.9 to 2.0 s, and as many cuts
/// of a text of a million letters of three bytes in 0.3 s (release build, an idle
/// machine): within a fifth of the time a worker has for its answer. Steps that are
/// taken again at every reading are counted for the document as well
/// ([`READING_STEPS`]): an update of more of them than a document may have is refused
/// once it is applied, in under a second. An editor's update is a handful of steps. Past it are updates of a hundred changes that each go into a
/// list of tens of thousands, or of hundreds of cuts of a text of a million letters.
pub const PLACING_STEPS: u64 = 3 * 1024 * 1024;

/// How many steps the Y library may take to read a document from its state: the
/// document an update a client sends leaves, and one that was stored
/// ([`UpdateLimits::reading_steps`]). The library places a change by walking past the
/// changes between the neighbours it names, and walks so again every time it reads the
/// document from its state: two or three times for every update, five where the update
/// is repaired, and twice as the document is opened. A step takes 0.13 to 0.24 µs in a
/// list of 50,000 (release build, a machine that does other work besides, at which
/// all the times here were measured; a busier one took twice as long), so each
/// reading of such a document at this many steps takes 0.15 to 0.25 s more than the
/// document's size asks: a paragraph typed
/// into a list of 50,000 at these steps was taken in 0.31 s where it took 0.06 s,
/// repaired in 0.74 s where it took 0.23 s, and the document was opened in 0.30 s
/// where it took 0.10 s. With the slowest update of the largest body (2.3 s, repaired)
/// that is within half the time a worker has for its answer (10 s). A step takes
/// longer the more changes a document has that lie far from their neighbours, 0.5 µs
/// in a list of 200,000 such and 0.7 to 0.9 µs in one of 700,000
/// (`tests/cost.rs`, `scattered`): such a document is slow by its size before its
/// steps are counted, 1.5 s a reading, which the limits of size and of a client's
/// changes are for, not this one.
///
/// What editors write is placed in a step or two where two people wrote at one place:
/// the largest bodies, edited, are under 5,000 steps (the changes of the body's
/// client, looked at once each while an edit waits for the one it names), and a
/// body's own document is none. Past it are documents with changes made to walk: 62
/// changes of 383 bytes that name the last of 50,000 children to their right and
/// nothing to their left are three million steps.
///
/// And past it are documents that thousands of sessions wrote one after the other,
/// with no change made for it. Each session is a client, whose changes wait for those
/// of the session it wrote after, and the library looks through all that wait each
/// time one does: 400 sessions of a paragraph are 289 steps, 400 of 50 changes
/// 15,000, and from a few thousand sessions on a session whose client is read before
/// the others brings the steps past the limit with its first paragraph, whatever
/// they were (4,000 sessions of 20 changes, 2,500 of 100: `tests/reading.rs`).
///
/// Before it comes to that, what holds a document goes by the steps the changes it
/// has may take once one more client is read first ([`crate::steps_ahead`]: each
/// change looked at once for each client), and makes the document again from its
/// body, which has no steps and one client: at an eighth of this ahead as the
/// document is closed with nobody holding it, and at three quarters under its
/// editors. What an update adds is not in that count (a change it writes, or cuts in
/// two, is a step for every 128 clients), so an update can still leave a document
/// past this limit: when the copy with the update is already a valid body (needs no
/// repair), [`crate::take_update`] still takes it without reading the slow state
/// again and marks [`crate::Taken::remake`] so what holds it writes its body and
/// makes it again from that without losing the update; only one that also needs a
/// repair is refused with [`UpdateError::TooManyReadingSteps`].
///
/// A stored state past it is refused before the library reads it, in the time of the
/// count, which stops a little past the limit whatever the state holds: one that was
/// stored before the steps were counted, as every update taken since leaves a
/// document within it (or is made again from its body before its state is stored).
pub const READING_STEPS: u64 = 1024 * 1024;

/// Why an update is refused.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum UpdateError {
    /// The update is larger than [`UpdateLimits::update_bytes`].
    #[error("the update is {size} bytes, more than the {limit} allowed")]
    TooLarge {
        /// The size of the update.
        size: usize,
        /// The limit.
        limit: usize,
    },
    /// The bytes are no update.
    #[error("the update cannot be read: {0}")]
    Unreadable(String),
    /// The update cannot be applied to the document.
    #[error("the update cannot be applied: {0}")]
    Unappliable(String),
    /// The deletions of a client in the update go back or overlap, which Yjs never
    /// writes: the library would walk the client's changes again for each.
    #[error("the deletions of client {client} go back to clock {start} from {end}")]
    DeletionsOutOfOrder {
        /// The client.
        client: u64,
        /// Where the range that goes back starts.
        start: u32,
        /// Where the range before it ends.
        end: u32,
    },
    /// The update builds on changes the document does not have: a client's changes
    /// begin later than where the document has them, or part of the update waits for
    /// changes of another client instead of taking effect. A client sends what it has
    /// in order; one that leaves a gap is not in step with the document.
    #[error("the update builds on changes the document does not have")]
    MissingChanges,
    /// The update writes outside the fragment that holds the body.
    #[error("the update writes to the root {0:?}, which is not the body's")]
    UnexpectedRoot(String),
    /// The changes of the update would take the library more steps to place than
    /// [`UpdateLimits::placing_steps`]: each change that names a neighbour the document
    /// has to its left, or none, is counted what it may walk past, and each change it
    /// cuts its length.
    #[error(
        "the changes of the update would take {steps} steps to place in the document, \
         more than the {limit} allowed"
    )]
    PlacedPastTooMany {
        /// The steps its changes may take to place, counted no further than is needed
        /// to know that they are more than the limit.
        steps: u64,
        /// The limit.
        limit: u64,
    },
    /// With the update the document would name more clients than
    /// [`UpdateLimits::clients`].
    #[error(
        "with the update the document would name {clients} clients, more than the {limit} allowed"
    )]
    TooManyClients {
        /// The clients it would name.
        clients: usize,
        /// The limit.
        limit: usize,
    },
    /// With the update a client of the document would have more changes than
    /// [`UpdateLimits::client_changes`].
    #[error(
        "with the update a client of the document would have {changes} changes, more than \
         the {limit} allowed"
    )]
    TooManyChanges {
        /// The most changes a client would have, deleted ones too.
        changes: u64,
        /// The limit.
        limit: u64,
    },
    /// With the update the library would take more steps to read the document from its
    /// state than [`UpdateLimits::reading_steps`].
    #[error(
        "with the update the document would take {steps} steps to read, more than the \
         {limit} allowed"
    )]
    TooManyReadingSteps {
        /// The steps it would take, counted no further than is needed to know that
        /// they are more than the limit.
        steps: u64,
        /// The limit.
        limit: u64,
    },
    /// With the update the document would be larger than [`UpdateLimits::state_bytes`].
    #[error("with the update the document would be {size} bytes, more than the {limit} allowed")]
    DocumentTooLarge {
        /// The size the document would have.
        size: usize,
        /// The limit.
        limit: usize,
    },
    /// With the update the document could not be read again from what is stored of it:
    /// the library took something that it writes in another form than it read it.
    #[error("with the update the document could not be read again: {0}")]
    LeavesUnreadable(String),
    /// The document the update was given for cannot be read again from its own state,
    /// so that no update can be taken for it. Nothing this crate takes leaves one so.
    #[error("the document cannot be read again from its own state: {0}")]
    DocumentUnreadable(String),
}

/// The whole of a Y document as a version 1 update: what is stored, and what brings an
/// empty document to the same state.
#[must_use]
pub fn state_of(doc: &Doc) -> Vec<u8> {
    doc.transact()
        .encode_state_as_update_v1(&StateVector::default())
}

/// A copy of a Y document that shares nothing with it, which writes as `writer`, or as
/// a client new to it if `None`. (Cloning a `Doc` gives a second handle to the same
/// document.)
///
/// # Errors
///
/// Returns [`UpdateError::DocumentUnreadable`] if the document's own state cannot be
/// read again as any update is read.
pub(crate) fn copy_of(doc: &Doc, writer: Option<u64>) -> Result<Doc, UpdateError> {
    doc_of_state(&state_of(doc), writer).map_err(UpdateError::DocumentUnreadable)
}

/// The Y document whose state is `state`, read as any update is read: past the walk
/// (`scan`), with every change it builds on, and with nothing left waiting. It writes as
/// `writer`, or as a client new to it if `None`.
///
/// # Errors
///
/// Returns why the state is no whole document.
fn doc_of_state(state: &[u8], writer: Option<u64>) -> Result<Doc, String> {
    let walked = scan::check(state).map_err(|refusal| refusal.reason().to_owned())?;
    read_walked(state, &walked, writer)
}

/// [`doc_of_state`], with what the walk found of the state: how many changes it holds,
/// and the most of one client.
fn read_state(state: &[u8]) -> Result<(Doc, scan::Walked), String> {
    let walked = scan::check(state).map_err(|refusal| refusal.reason().to_owned())?;
    read_walked(state, &walked, None).map(|doc| (doc, walked))
}

/// The Y document of a state the walk has taken, which writes as `writer`, or as a
/// client new to it if `None`.
fn read_walked(state: &[u8], walked: &scan::Walked, writer: Option<u64>) -> Result<Doc, String> {
    if walked.begins.iter().any(|(_, clock)| *clock > 0) {
        return Err(UpdateError::MissingChanges.to_string());
    }
    let update = Update::decode_v1(state).map_err(|error| error.to_string())?;
    let doc = doc_of_client(writer);
    {
        let mut txn = doc.transact_mut();
        txn.apply_update(update)
            .map_err(|error| error.to_string())?;
        if txn.has_missing_updates() {
            return Err(UpdateError::MissingChanges.to_string());
        }
    }
    Ok(doc)
}

/// A document read from a state an update left, with what was counted of the state
/// before the library read it.
struct Counted {
    doc: Doc,
    /// The most changes of one client.
    client_changes: u64,
    /// The steps the reading took; 0 where they are not counted.
    reading_steps: u64,
    /// The most steps it may take once one more client has written; 0 likewise.
    steps_ahead: u64,
}

/// What [`read_counted`] did with a state.
enum ReadOutcome {
    /// Within [`UpdateLimits::reading_steps`] (or uncounted) and read back into a `Doc`.
    Read(Counted),
    /// Past [`UpdateLimits::reading_steps`]: the library did not read it.
    PastLimit {
        client_changes: u64,
        steps: u64,
        steps_ahead: u64,
        limit: u64,
    },
}

/// Counts a state an update left against [`UpdateLimits::reading_steps`] and reads it
/// back if it is within the limit; a state past the limit does not reach the library.
fn read_counted(state: &[u8], limits: &UpdateLimits) -> Result<ReadOutcome, UpdateError> {
    let unreadable = |refusal: Refusal| UpdateError::LeavesUnreadable(refusal.reason().to_owned());
    let Some(limit) = limits.reading_steps else {
        let (doc, walked) = read_state(state).map_err(UpdateError::LeavesUnreadable)?;
        return Ok(ReadOutcome::Read(Counted {
            doc,
            client_changes: walked.most,
            reading_steps: 0,
            steps_ahead: 0,
        }));
    };
    let read = reading::count(state, limit).map_err(unreadable)?;
    if read.steps > limit {
        return Ok(ReadOutcome::PastLimit {
            client_changes: read.walked.most,
            steps: read.steps,
            steps_ahead: read.ahead,
            limit,
        });
    }
    let doc = read_walked(state, &read.walked, None).map_err(UpdateError::LeavesUnreadable)?;
    // Lengths counted otherwise than the library counts them would have found the
    // neighbours of the changes in other places than the library does.
    let ends = doc.transact().state_vector();
    let as_the_library = ends.len() == read.ends.len()
        && read
            .ends
            .iter()
            .all(|(client, end)| u64::from(ends.get(&ClientID::new(*client))) == *end);
    if !as_the_library {
        return Err(unreadable(Refusal::Lengths));
    }
    Ok(ReadOutcome::Read(Counted {
        doc,
        client_changes: read.walked.most,
        reading_steps: read.steps,
        steps_ahead: read.ahead,
    }))
}

/// Refuses a stored state whose reading takes more steps than
/// [`UpdateLimits::reading_steps`], before the library reads it.
pub(crate) fn within_reading_steps(state: &[u8], limits: &UpdateLimits) -> Result<(), UpdateError> {
    let Some(limit) = limits.reading_steps else {
        return Ok(());
    };
    match reading::count(state, limit) {
        Ok(read) if read.steps > limit => Err(UpdateError::TooManyReadingSteps {
            steps: read.steps,
            limit,
        }),
        Ok(_) => Ok(()),
        Err(Refusal::Gap) => Err(UpdateError::MissingChanges),
        Err(refusal) => Err(UpdateError::Unreadable(refusal.reason().to_owned())),
    }
}

/// Reads the bytes of an update for a document, after the walk that keeps from the
/// library what it does not survive (`scan`), with how many clients the document names
/// with it.
pub(crate) fn read(doc: &Doc, update: &[u8]) -> Result<(Update, usize), UpdateError> {
    let begins = scan::check(update)
        .map_err(|refusal| UpdateError::Unreadable(refusal.reason().to_owned()))?
        .begins;
    // A client's changes go on from where the document has them: the library would
    // take changes that begin later and leave a hole before them.
    let known = doc.transact().state_vector();
    let mut clients = known.len();
    for (client, clock) in begins {
        let client = ClientID::new(client);
        if clock > u64::from(known.get(&client)) {
            return Err(UpdateError::MissingChanges);
        }
        if !known.contains_client(&client) {
            clients += 1;
        }
    }
    let update =
        Update::decode_v1(update).map_err(|error| UpdateError::Unreadable(error.to_string()))?;
    Ok((update, clients))
}

/// The Y document as it would be with a client's update: a copy of `doc` with the
/// update applied, read again from its state. `doc` itself is not changed.
///
/// What comes back has still to be projected and, where that fails, repaired: this only
/// refuses what cannot be taken at all. [`crate::take_update`] does all of it, and says
/// what a server that calls it keeps in mind about time.
///
/// Not everything an update may write is read by the projection: attributes the schema
/// does not have, attributes of a text. No editor reads them
/// either. They stay in the document, and [`UpdateLimits::state_bytes`] is what bounds
/// them.
///
/// # Errors
///
/// Returns an [`UpdateError`] if the update is too large, is no update, cannot be
/// applied, leaves a part of itself waiting for changes the document does not have,
/// writes to another root than the body's, leaves the document too large or in a
/// state that cannot be read again, or if `doc` itself cannot be read again.
#[cfg(any(test, feature = "test-util"))]
pub(crate) fn with_update(
    doc: &Doc,
    update: &[u8],
    limits: &UpdateLimits,
) -> Result<Doc, UpdateError> {
    with_update_sized(doc, update, limits).map(|settled| settled.doc)
}

/// [`with_update`], with the size of the state of what comes back.
pub(crate) fn with_update_sized(
    doc: &Doc,
    update: &[u8],
    limits: &UpdateLimits,
) -> Result<Settled, UpdateError> {
    if update.len() > limits.update_bytes {
        return Err(UpdateError::TooLarge {
            size: update.len(),
            limit: limits.update_bytes,
        });
    }
    let (sent, clients) = read(doc, update)?;
    // Before the document is read, which takes the square of its clients.
    if let Some(limit) = limits.clients
        && clients > limit
    {
        return Err(UpdateError::TooManyClients { clients, limit });
    }
    let state = state_of(doc);
    let (copy, walked) = read_state(&state).map_err(UpdateError::DocumentUnreadable)?;
    if let Some(limit) = limits.placing_steps {
        deletions_in_order(&sent)?;
        let steps = charged(&copy, (&state, walked.changes), (update, &sent), limit)?;
        if steps > limit {
            return Err(UpdateError::PlacedPastTooMany { steps, limit });
        }
    }
    drop(state);
    let update = sent;
    {
        let mut txn = copy.transact_mut();
        txn.apply_update(update)
            .map_err(|error| UpdateError::Unappliable(error.to_string()))?;
    }
    {
        let txn = copy.transact();
        if txn.has_missing_updates() {
            return Err(UpdateError::MissingChanges);
        }
        for (name, root) in txn.root_refs() {
            if name != FIELD || !matches!(root, Out::YXmlFragment(_)) {
                return Err(UpdateError::UnexpectedRoot(name.to_owned()));
            }
        }
    }
    settled_sized(copy, limits)
}

/// Refuses an update whose deletions of a client go back or overlap. The library walks
/// the blocks of a client from the start of each range to its end, deleted or not, in
/// the order the ranges are written: one range written 16,000 times over a client of
/// 75,000 blocks took 7 to 13 s. Yjs writes the ranges of a client in order and merged,
/// so an editor's update always passes.
fn deletions_in_order(update: &Update) -> Result<(), UpdateError> {
    for (client, ranges) in update.delete_set().iter() {
        let mut end = 0;
        for range in ranges.iter() {
            if range.start < end {
                return Err(UpdateError::DeletionsOutOfOrder {
                    client: client.get(),
                    start: range.start,
                    end,
                });
            }
            end = range.end;
        }
    }
    Ok(())
}

/// What the changes of an update are charged for the document of a state
/// ([`placing::charge`]): `copy` is the document as the library read it from `state`,
/// which holds `changes`, and `sent` the update as the library read it from `update`.
pub(crate) fn charged(
    copy: &Doc,
    (state, changes): (&[u8], u64),
    (update, sent): (&[u8], &Update),
    limit: u64,
) -> Result<u64, UpdateError> {
    let held = Held {
        state,
        changes,
        ends: &copy.transact().state_vector(),
    };
    let sent = Sent {
        update,
        covered: &sent.insertions(true),
        deleted: sent.delete_set(),
    };
    placing::charge(&held, &sent, limit)
        .map_err(|refusal| UpdateError::Unreadable(refusal.reason().to_owned()))
}

/// What the changes of `update` are charged for `doc`, counted list by list whatever
/// the update holds.
#[cfg(any(test, feature = "test-util"))]
pub(crate) fn placing_steps(doc: &Doc, update: &[u8]) -> Result<u64, UpdateError> {
    let (sent, _) = read(doc, update)?;
    let state = state_of(doc);
    let (copy, walked) = read_state(&state).map_err(UpdateError::DocumentUnreadable)?;
    // Past a limit of nothing every update is counted list by list.
    charged(&copy, (&state, walked.changes), (update, &sent), 0)
}

/// Whether [`settled_sized`] read the document back from its state or stopped because
/// reading its state would take more steps than [`UpdateLimits::reading_steps`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// Read back from its state within the steps allowed.
    Settled,
    /// Not read back: reading its state would take `steps` steps, past `limit`.
    /// Must be made again from its body before its state is stored or another update
    /// is taken, and refused with [`UpdateError::TooManyReadingSteps`] if a repair
    /// is needed.
    Remake { steps: u64, limit: u64 },
}

/// A document as a server that stored it reads it ([`settled_sized`]), or the
/// unsettled copy when reading its state would take more steps than allowed
/// ([`Outcome::Remake`]), with the size of its state, the most changes of one of its
/// clients, the steps the library takes to read it from its state, and the most it
/// may take once one more client has written into it (0 where they are not counted).
#[derive(Debug)]
pub(crate) struct Settled {
    pub(crate) doc: Doc,
    pub(crate) state_bytes: usize,
    pub(crate) client_changes: u64,
    pub(crate) reading_steps: u64,
    pub(crate) steps_ahead: u64,
    pub(crate) outcome: Outcome,
}

/// Refuses a state whose client with the most changes is past
/// [`UpdateLimits::client_changes`].
fn check_client_changes(changes: u64, limits: &UpdateLimits) -> Result<(), UpdateError> {
    if let Some(limit) = limits.client_changes
        && changes > limit
    {
        return Err(UpdateError::TooManyChanges { changes, limit });
    }
    Ok(())
}

/// A document as a server that stored it reads it: its state within the limit of
/// size, read again within the steps a reading may take, and reading again to the same
/// changes and deletions at the latest the second time ([`same_state`]). Where reading
/// its state would take more steps than [`UpdateLimits::reading_steps`], returns `doc`
/// unsettled with [`Outcome::Remake`] instead of handing the slow state to the library.
///
/// What the library took it may write in a form it does not read: the walk keeps out
/// what is known of that, and this is for what is not.
///
/// # Errors
///
/// Returns [`UpdateError::DocumentTooLarge`] past [`UpdateLimits::state_bytes`],
/// [`UpdateError::TooManyChanges`] past [`UpdateLimits::client_changes`], and
/// [`UpdateError::LeavesUnreadable`] for a state that does not read again to itself.
pub(crate) fn settled_sized(doc: Doc, limits: &UpdateLimits) -> Result<Settled, UpdateError> {
    let state = state_of(&doc);
    if state.len() > limits.state_bytes {
        return Err(UpdateError::DocumentTooLarge {
            size: state.len(),
            limit: limits.state_bytes,
        });
    }
    let again = match read_counted(&state, limits)? {
        ReadOutcome::Read(again) => {
            check_client_changes(again.client_changes, limits)?;
            again
        }
        ReadOutcome::PastLimit {
            client_changes,
            steps,
            steps_ahead,
            limit,
        } => {
            check_client_changes(client_changes, limits)?;
            return Ok(Settled {
                doc,
                state_bytes: state.len(),
                client_changes,
                reading_steps: steps,
                steps_ahead,
                outcome: Outcome::Remake { steps, limit },
            });
        }
    };
    let state_again = state_of(&again.doc);
    if same_state(&state_again, &state) {
        return Ok(Settled {
            doc: again.doc,
            state_bytes: state_again.len(),
            client_changes: again.client_changes,
            reading_steps: again.reading_steps,
            steps_ahead: again.steps_ahead,
            outcome: Outcome::Settled,
        });
    }
    // A first reading may write the same document otherwise (neighbouring changes it
    // joins); a second must not. What it writes is what is stored and read from then
    // on, and a change it joined is cut again by every reading: counted as well.
    let read_twice = match read_counted(&state_again, limits)? {
        ReadOutcome::Read(read_twice) => read_twice,
        ReadOutcome::PastLimit {
            client_changes,
            steps,
            steps_ahead,
            limit,
        } => {
            if state_again.len() > limits.state_bytes {
                return Err(UpdateError::DocumentTooLarge {
                    size: state_again.len(),
                    limit: limits.state_bytes,
                });
            }
            check_client_changes(client_changes, limits)?;
            return Ok(Settled {
                doc: again.doc,
                state_bytes: state_again.len(),
                client_changes: again.client_changes,
                reading_steps: steps,
                steps_ahead,
                outcome: Outcome::Remake { steps, limit },
            });
        }
    };
    let state_settled = state_of(&read_twice.doc);
    if !same_state(&state_settled, &state_again) {
        return Err(UpdateError::LeavesUnreadable(
            "its state reads to a document whose state reads to another".to_owned(),
        ));
    }
    Ok(Settled {
        doc: read_twice.doc,
        state_bytes: state_settled.len(),
        client_changes: again.client_changes,
        reading_steps: read_twice.reading_steps,
        steps_ahead: read_twice.steps_ahead,
        outcome: Outcome::Settled,
    })
}

/// Whether two states are the same as far as can be told the same way every time: the
/// same bytes, or the same changes by their identifiers and the same deletions, in any
/// order (the library writes each client's deletions in the order of a table). Not
/// their content: with changes no editor makes (origins that do not lie where the
/// change does), the library reads one state to documents that differ from one reading
/// to the next, so that comparing content would refuse such an update only now and
/// then. Refusing those origins in the walk is what would close that.
pub(crate) fn same_state(one: &[u8], other: &[u8]) -> bool {
    one == other
        || matches!(
            (Update::decode_v1(one), Update::decode_v1(other)),
            (Ok(one), Ok(other)) if one == other
        )
}

#[cfg(test)]
mod tests {
    use yrs::{Any, Number, Xml, XmlElementPrelim, XmlFragment};

    use super::*;
    use crate::build::new_doc;

    const LIMITS: UpdateLimits = UpdateLimits {
        update_bytes: 1024,
        state_bytes: 1024,
        body_bytes: 1024,
        placing_steps: Some(PLACING_STEPS),
        clients: Some(MAX_CLIENTS),
        client_changes: Some(MAX_CLIENT_CHANGES),
        reading_steps: Some(READING_STEPS),
    };

    /// A document that holds what the library writes in a form the walk refuses: a
    /// whole number past 53 bits, put in here instead of by an update, as a part of the
    /// library that the walk does not know of could.
    fn unreadable() -> Doc {
        let doc = new_doc();
        let root = doc.get_or_insert_xml_fragment(FIELD);
        let mut txn = doc.transact_mut();
        let element = root.push_back(&mut txn, XmlElementPrelim::empty("paragraph"));
        element.insert_attribute(&mut txn, "n", Any::Number(Number::Int((1 << 53) + 1)));
        drop(txn);
        doc
    }

    #[test]
    fn a_document_that_cannot_be_read_again_is_refused_and_does_not_panic() {
        let doc = unreadable();
        assert!(
            doc_of_state(&state_of(&doc), None).is_err(),
            "its state is not read"
        );
        assert!(
            matches!(copy_of(&doc, None), Err(UpdateError::DocumentUnreadable(_))),
            "no copy"
        );
        let empty = [0, 0];
        assert!(
            matches!(
                with_update(&doc, &empty, &LIMITS),
                Err(UpdateError::DocumentUnreadable(_))
            ),
            "no update is taken for it"
        );
    }

    #[test]
    fn a_state_is_read_again_only_whole() {
        let doc = new_doc();
        assert!(
            doc_of_state(&state_of(&doc), None).is_ok(),
            "the empty document"
        );
        // A change of client 5 that begins at clock 3: the changes before it are missing.
        let mut later = vec![1, 1, 5, 3, 4, 1, 7];
        later.extend_from_slice(b"default");
        later.extend_from_slice(&[1, b'a', 0]);
        assert!(doc_of_state(&later, None).is_err(), "a state with a hole");
    }
}
