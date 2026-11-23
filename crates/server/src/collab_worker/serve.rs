//! The worker's loop: a request in, its answer out, until the server closes the pipe.

use std::collections::HashMap;
use std::io::{Read, Write};
#[cfg(feature = "test-util")]
use std::time::Duration;

use fvoci_collab::{
    BodyDoc, BodyTakeError, MAX_CLIENT_CHANGES, MAX_CLIENTS, PLACING_STEPS, READING_STEPS,
    SettleError, TakeError, UpdateError, UpdateLimits, same_state,
};
use fvoci_model::{Body, BodyError};

use super::frame::{FrameError, read_frame, write_frame};
use super::wire::{
    ASIDE_DOCUMENTS, Answer, DocNumber, Limits, Refusal, Refused, Request, Response, Share, Source,
    encode_response,
};

/// The largest frame before the limits are known: the one that brings them.
const CONFIGURE_FRAME_BYTES: u64 = 64;

/// How many accepted updates and repairs a held document may keep in its rollback log
/// (`Held::since`) before further `Request::Take` updates are refused until a fold
/// rebases it. Kept above 1,000 so direct worker bursts without folds still pass while
/// keeping rollback replay bounded.
const MAX_SINCE_ENTRIES: usize = 2048;

/// How many entries in `Held::since` cause the worker to ask the server for a fold
/// (`Answer::Taken::fold`), well before [`MAX_SINCE_ENTRIES`] is reached.
const FOLD_SINCE_ENTRIES: usize = 400;

/// The longest stall a request may ask for.
#[cfg(feature = "test-util")]
const LONGEST_STALL: Duration = Duration::from_mins(10);

/// Answers the requests of `input` on `output`, in order, until `input` ends.
///
/// # Errors
///
/// Returns why a frame could not be read or written, or a request read: the server
/// sent something no server of this build sends, or a pipe broke. The worker ends
/// then, and the server starts another.
pub fn serve(input: &mut impl Read, output: &mut impl Write) -> Result<(), FrameError> {
    let Some(first) = read_frame(input, CONFIGURE_FRAME_BYTES)? else {
        return Ok(());
    };
    let Ok(Request::Configure(limits)) = Request::decode(&first) else {
        return Err(malformed());
    };
    let mut worker = Worker::new(limits);
    let frame_bytes = limits.frame_bytes();
    write_frame(output, &encode_response(&Ok(Answer::Done)), frame_bytes)?;
    while let Some(frame) = read_frame(input, frame_bytes)? {
        let request = Request::decode(&frame).map_err(|_| malformed())?;
        let response = worker.answer(request)?;
        write_frame(output, &encode_response(&response), frame_bytes)?;
    }
    Ok(())
}

fn malformed() -> FrameError {
    FrameError::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        "the server sent no request",
    ))
}

/// The documents a worker holds, and the limits it holds them to.
#[derive(Debug)]
struct Worker {
    limits: Limits,
    update_limits: UpdateLimits,
    docs: HashMap<DocNumber, Held>,
    /// The bytes of the states and rollback logs (`since`) of all held documents.
    held_bytes: u64,
}

/// A document, its checkpoint (`base`) and accepted updates/repairs since it (`since`)
/// for exact rollback after a rejected in-place update, its latest state bytes and
/// change count, the share it is counted in, and whether it was left unsettled past
/// [`READING_STEPS`] and must be remade from its body before any further request other
/// than reading its body or closing it.
#[derive(Debug)]
struct Held {
    doc: BodyDoc,
    /// Checkpoint state as of opening, making, or the last rebase (`Request::State` or
    /// `Retake` rebase). Empty once `must_remake` is set.
    base: Vec<u8>,
    /// Accepted updates and repairs applied since `base`, replayed by
    /// [`BodyDoc::rebuild`] when an in-place update is refused after mutating `doc`.
    since: Vec<Vec<u8>>,
    /// Total bytes of every entry in `since`, counted in [`Worker::held_bytes`].
    since_bytes: u64,
    /// Latest state bytes when `since` is non-empty; `None` means `base` is the latest
    /// state (or `must_remake` is set).
    last_state: Option<Vec<u8>>,
    state_bytes: u64,
    changes: u64,
    share: Share,
    must_remake: bool,
    /// Whether the current state (`base` when `last_state` is `None`) has already been
    /// verified by a settle re-read (`same_state`). True on open/make and after
    /// `Request::State` settles; cleared by mutating `Take`/`Retake`.
    settled: bool,
    #[cfg(feature = "test-util")]
    fail_next_settle: bool,
}

impl Held {
    /// Total bytes accounted against [`Worker::held_bytes`] for this document.
    fn accounted_bytes(&self) -> u64 {
        self.state_bytes.saturating_add(self.since_bytes)
    }

    /// Promotes `last_state` into `base`, clears `since` (releasing its backing array),
    /// and returns the `since_bytes` freed for [`Worker::held_bytes`].
    fn rebase(&mut self) -> u64 {
        if let Some(last) = self.last_state.take() {
            self.base = last;
        }
        self.since = Vec::new();
        let freed = self.since_bytes;
        self.since_bytes = 0;
        freed
    }
}

impl Worker {
    fn new(limits: Limits) -> Self {
        let size = |bytes: u64| usize::try_from(bytes).unwrap_or(usize::MAX);
        Self {
            limits,
            update_limits: UpdateLimits {
                update_bytes: size(limits.update_bytes),
                state_bytes: size(limits.state_bytes),
                body_bytes: size(limits.body_bytes),
                placing_steps: Some(PLACING_STEPS),
                clients: Some(MAX_CLIENTS),
                client_changes: Some(MAX_CLIENT_CHANGES),
                reading_steps: Some(READING_STEPS),
            },
            docs: HashMap::new(),
            held_bytes: 0,
        }
    }

    /// Hard cap on `Held::since_bytes` for one document before `Take` is refused.
    fn max_since_bytes(&self) -> u64 {
        self.limits.state_bytes
    }

    /// Soft threshold on `Held::since_bytes` that asks the server to fold soon.
    fn fold_since_bytes(&self) -> u64 {
        (self.limits.state_bytes / 2).max(1)
    }

    fn answer(&mut self, request: Request) -> Result<Response, FrameError> {
        match request {
            Request::Configure(_) => Ok(Err(refused(
                Refusal::OutOfOrder,
                "the worker is configured once",
            ))),
            Request::Open { doc, source, share } => Ok(self.open(doc, source, share)),
            Request::Take { doc, update } => self.take(
                doc,
                &update,
                self.update_limits,
                take_refused,
                Refusal::OutOfOrder,
                TakeMode::Live,
            ),
            Request::Retake { doc, update } => self.take(
                doc,
                &update,
                self.logged_limits(),
                logged_refused,
                Refusal::Unappliable,
                TakeMode::Retake,
            ),
            Request::StateVector(doc) => Ok(self
                .usable_doc(doc, Refusal::OutOfOrder)
                .map(|body| Answer::Bytes(body.state_vector()))),
            Request::Diff { doc, state_vector } => {
                Ok(self.usable_doc(doc, Refusal::OutOfOrder).and_then(|body| {
                    body.diff(&state_vector)
                        .map(Answer::Bytes)
                        .map_err(|error| refused(Refusal::Unreadable, &error.to_string()))
                }))
            }
            Request::State(doc) => Ok(self.state_and_rebase(doc)),
            Request::Body(doc) => Ok(self.doc(doc).and_then(|body| {
                body.body()
                    .map(|body| Answer::Body(body.map(|body| json_of(&body))))
                    .map_err(|error| refused(Refusal::NoBody, &error.to_string()))
            })),
            Request::Close(doc) => {
                if let Some(held) = self.docs.remove(&doc) {
                    self.held_bytes = self.held_bytes.saturating_sub(held.accounted_bytes());
                }
                Ok(Ok(Answer::Done))
            }
            #[cfg(feature = "test-util")]
            Request::Stall(millis) => {
                std::thread::sleep(Duration::from_millis(millis).min(LONGEST_STALL));
                Ok(Ok(Answer::Done))
            }
            #[cfg(feature = "test-util")]
            Request::FailNextSettle(doc) => Ok(self.fail_next_settle(doc)),
        }
    }

    #[cfg(feature = "test-util")]
    fn fail_next_settle(&mut self, doc: DocNumber) -> Response {
        let held = self
            .docs
            .get_mut(&doc)
            .ok_or_else(|| refused(Refusal::UnknownDocument, "no document is open under it"))?;
        held.fail_next_settle = true;
        Ok(Answer::Done)
    }

    /// Limits used when settling an unsettled document on `Request::State` or during
    /// non-canonical `Dirty` rollback: allows logged/larger states within `held_bytes`
    /// while preserving `reading_steps` and strict origin checks.
    fn settle_limits(&self) -> UpdateLimits {
        UpdateLimits {
            reading_steps: self.update_limits.reading_steps,
            ..self.logged_limits()
        }
    }

    /// Transitions a document whose settle check failed into `must_remake` mode while
    /// keeping `doc_body` intact for `Request::Body` and reclaiming `since_bytes`.
    fn mark_unsettled_for_remake(
        &mut self,
        doc: DocNumber,
        doc_body: BodyDoc,
        old_state_bytes: u64,
        old_changes: u64,
        share: Share,
        base_held: u64,
    ) {
        self.held_bytes = base_held.saturating_add(old_state_bytes);
        self.docs.insert(
            doc,
            Held {
                doc: doc_body,
                base: Vec::new(),
                since: Vec::new(),
                since_bytes: 0,
                last_state: None,
                state_bytes: old_state_bytes,
                changes: old_changes,
                share,
                must_remake: true,
                settled: false,
                #[cfg(feature = "test-util")]
                fail_next_settle: false,
            },
        );
    }

    /// Settles (if unsettled) and returns the document's current canonical state bytes,
    /// rebasing its checkpoint and reclaiming `since_bytes` from [`Self::held_bytes`].
    ///
    /// If settling fails or would overflow `held_bytes`, keeps the live document intact
    /// in `must_remake` mode and returns [`Answer::Remake`] so the server projects its
    /// body and remakes it into a fresh 0-step document without losing updates.
    fn state_and_rebase(&mut self, doc: DocNumber) -> Response {
        let force_fail = {
            let held = self
                .docs
                .get_mut(&doc)
                .ok_or_else(|| refused(Refusal::UnknownDocument, "no document is open under it"))?;
            if held.must_remake {
                return Err(refused(
                    Refusal::OutOfOrder,
                    "the document must be made again from its body before another request",
                ));
            }
            #[cfg(feature = "test-util")]
            let force_fail = {
                let fail = held.fail_next_settle;
                held.fail_next_settle = false;
                fail
            };
            #[cfg(not(feature = "test-util"))]
            let force_fail = false;
            if held.settled && !force_fail {
                let freed = held.rebase();
                self.held_bytes = self.held_bytes.saturating_sub(freed);
                return Ok(Answer::Bytes(held.base.clone()));
            }
            force_fail
        };
        let held = self.docs.remove(&doc).expect("checked open above");
        let base_held = self.held_bytes.saturating_sub(held.accounted_bytes());
        let old_state_bytes = held.state_bytes;
        let old_changes = held.changes;
        let share = held.share;
        if force_fail {
            self.mark_unsettled_for_remake(
                doc,
                held.doc,
                old_state_bytes,
                old_changes,
                share,
                base_held,
            );
            return Ok(Answer::Remake);
        }
        let state_to_settle = held.last_state.unwrap_or(held.base);
        match held.doc.settle(state_to_settle, &self.settle_limits()) {
            Ok(settled) => {
                let new_state_bytes = count(settled.state_bytes);
                if base_held.saturating_add(new_state_bytes) > self.limits.held_bytes {
                    self.mark_unsettled_for_remake(
                        doc,
                        settled.doc,
                        old_state_bytes,
                        old_changes,
                        share,
                        base_held,
                    );
                    return Ok(Answer::Remake);
                }
                self.held_bytes = base_held.saturating_add(new_state_bytes);
                let reply = settled.state.clone();
                self.docs.insert(
                    doc,
                    Held {
                        doc: settled.doc,
                        base: settled.state,
                        since: Vec::new(),
                        since_bytes: 0,
                        last_state: None,
                        state_bytes: new_state_bytes,
                        changes: settled.changes,
                        share,
                        must_remake: false,
                        settled: true,
                        #[cfg(feature = "test-util")]
                        fail_next_settle: false,
                    },
                );
                Ok(Answer::Bytes(reply))
            }
            Err(SettleError { doc: intact, .. }) => {
                self.mark_unsettled_for_remake(
                    doc,
                    intact,
                    old_state_bytes,
                    old_changes,
                    share,
                    base_held,
                );
                Ok(Answer::Remake)
            }
        }
    }

    /// Takes an update into a document in place within `limits`; `refuse` maps a
    /// [`TakeError`] to a refusal, and `on_remake` is returned if the held document is
    /// waiting to be remade from its body. On post-mutation refusal (or memory/since-cap
    /// overflow), restores the document in O(1) from its checkpoint state (settling a
    /// non-canonical unsettled checkpoint if needed).
    fn take(
        &mut self,
        doc: DocNumber,
        update: &[u8],
        limits: UpdateLimits,
        refuse: fn(&TakeError) -> Refused,
        on_remake: Refusal,
        mode: TakeMode,
    ) -> Result<Response, FrameError> {
        {
            let Some(held) = self.docs.get(&doc) else {
                return Ok(Err(refused(
                    Refusal::UnknownDocument,
                    "no document is open under it",
                )));
            };
            if held.must_remake {
                return Ok(Err(refused(
                    on_remake,
                    "the document must be made again from its body before another request",
                )));
            }
        }
        let held = self.docs.remove(&doc).expect("checked open above");
        let Held {
            doc: body_doc,
            base,
            since,
            since_bytes,
            last_state,
            state_bytes: old_state_bytes,
            changes: old_changes,
            share,
            must_remake: _,
            settled,
            #[cfg(feature = "test-util")]
            fail_next_settle,
        } = held;
        let checkpoint = Checkpoint {
            base,
            since,
            since_bytes,
            last_state,
            state_bytes: old_state_bytes,
            changes: old_changes,
            share,
            settled,
            #[cfg(feature = "test-util")]
            fail_next_settle,
        };
        let prior_state = checkpoint.current_state();
        let changed = match body_doc.take_in_place(prior_state, old_changes, update, &limits) {
            Ok(changed) => changed,
            Err(BodyTakeError::Untouched { doc: intact, error }) => {
                self.docs.insert(doc, checkpoint.into_held(intact));
                return Ok(Err(refuse(&error)));
            }
            Err(BodyTakeError::Dirty(error)) => {
                self.restore_after_dirty(doc, checkpoint)?;
                return Ok(Err(refuse(&error)));
            }
        };
        self.commit_taken(doc, update, mode, checkpoint, changed)
    }

    /// Commits a successful in-place `TakenInPlace` into `self.docs`, checking `since` and
    /// `held_bytes` budgets and rolling back via [`Self::restore_after_dirty`] if either
    /// would be exceeded.
    fn commit_taken(
        &mut self,
        doc: DocNumber,
        update: &[u8],
        mode: TakeMode,
        mut checkpoint: Checkpoint,
        changed: fvoci_collab::TakenInPlace,
    ) -> Result<Response, FrameError> {
        let base_held = self.held_bytes.saturating_sub(checkpoint.accounted_bytes());
        if changed.remake {
            return self.commit_remake(doc, base_held, checkpoint, changed);
        }
        let state_bytes = count(changed.state_bytes);
        let prior_state = checkpoint.current_state();
        let unchanged = changed.repair.update.is_none()
            && (changed.state.as_slice() == prior_state
                || (changed.state.len() == prior_state.len()
                    && same_state(&changed.state, prior_state)));
        let mutated = !unchanged;
        let added_entries = if mutated {
            1 + usize::from(changed.repair.update.is_some())
        } else {
            0
        };
        let added_bytes = if mutated {
            size_of(update).saturating_add(changed.repair.update.as_deref().map_or(0, size_of))
        } else {
            0
        };
        let next_since_bytes = checkpoint.since_bytes.saturating_add(added_bytes);
        let next_since_len = checkpoint.since.len().saturating_add(added_entries);
        let since_over =
            next_since_bytes > self.max_since_bytes() || next_since_len > MAX_SINCE_ENTRIES;
        let budget_over = base_held
            .saturating_add(state_bytes)
            .saturating_add(next_since_bytes)
            > self.limits.held_bytes;
        if mode == TakeMode::Retake && (since_over || budget_over) {
            return self.commit_retake_rebase(doc, base_held, state_bytes, checkpoint, changed);
        }
        if since_over || budget_over {
            drop(changed);
            self.restore_after_dirty(doc, checkpoint)?;
            let message = if since_over {
                "the document holds as many un-folded updates as it may"
            } else {
                "the worker holds as many bytes of documents as it may"
            };
            return Ok(Err(refused(Refusal::TooManyDocuments, message)));
        }
        let (clients, files) = doc_meta(&changed);
        let repair = changed.repair.update;
        if mutated {
            checkpoint.since.push(update.to_vec());
            if let Some(repair_bytes) = &repair {
                checkpoint.since.push(repair_bytes.clone());
            }
            checkpoint.since_bytes = next_since_bytes;
            checkpoint.last_state = Some(changed.state);
            checkpoint.settled = false;
        }
        self.held_bytes = base_held
            .saturating_add(state_bytes)
            .saturating_add(checkpoint.since_bytes);
        let fold = checkpoint.since_bytes >= self.fold_since_bytes()
            || checkpoint.since.len() >= FOLD_SINCE_ENTRIES;
        checkpoint.state_bytes = state_bytes;
        checkpoint.changes = changed.changes;
        self.docs.insert(doc, checkpoint.into_held(changed.doc));
        Ok(Ok(Answer::Taken {
            repair,
            state_bytes,
            clients,
            client_changes: changed.client_changes,
            steps_ahead: changed.steps_ahead,
            files,
            remake: false,
            fold,
        }))
    }

    /// Rebases a replayed (`Retake`) document onto `changed.state` when its `since` log
    /// would overflow its cap or `held_bytes`, rolling back if `state_bytes` alone exceeds
    /// `held_bytes`.
    fn commit_retake_rebase(
        &mut self,
        doc: DocNumber,
        base_held: u64,
        state_bytes: u64,
        mut checkpoint: Checkpoint,
        changed: fvoci_collab::TakenInPlace,
    ) -> Result<Response, FrameError> {
        if base_held.saturating_add(state_bytes) > self.limits.held_bytes {
            drop(changed);
            self.restore_after_dirty(doc, checkpoint)?;
            return Ok(Err(too_many_bytes()));
        }
        let (clients, files) = doc_meta(&changed);
        let repair = changed.repair.update;
        checkpoint.base = changed.state;
        checkpoint.since = Vec::new();
        checkpoint.since_bytes = 0;
        checkpoint.last_state = None;
        checkpoint.state_bytes = state_bytes;
        checkpoint.changes = changed.changes;
        checkpoint.settled = false;
        self.held_bytes = base_held.saturating_add(state_bytes);
        self.docs.insert(doc, checkpoint.into_held(changed.doc));
        Ok(Ok(Answer::Taken {
            repair,
            state_bytes,
            clients,
            client_changes: changed.client_changes,
            steps_ahead: changed.steps_ahead,
            files,
            remake: false,
            fold: false,
        }))
    }

    /// Commits an unsettled `remake: true` update: checks `held_bytes`, drops `base` and
    /// `since` (the document only allows `Body` and `Close` until remade), and stores it.
    fn commit_remake(
        &mut self,
        doc: DocNumber,
        base_held: u64,
        checkpoint: Checkpoint,
        changed: fvoci_collab::TakenInPlace,
    ) -> Result<Response, FrameError> {
        let state_bytes = count(changed.state_bytes);
        if base_held.saturating_add(state_bytes) > self.limits.held_bytes {
            drop(changed);
            self.restore_after_dirty(doc, checkpoint)?;
            return Ok(Err(too_many_bytes()));
        }
        let (clients, files) = doc_meta(&changed);
        self.held_bytes = base_held.saturating_add(state_bytes);
        self.docs.insert(
            doc,
            Held {
                doc: changed.doc,
                base: Vec::new(),
                since: Vec::new(),
                since_bytes: 0,
                last_state: None,
                state_bytes,
                changes: changed.changes,
                share: checkpoint.share,
                must_remake: true,
                settled: false,
                #[cfg(feature = "test-util")]
                fail_next_settle: false,
            },
        );
        Ok(Ok(Answer::Taken {
            repair: changed.repair.update,
            state_bytes,
            clients,
            client_changes: changed.client_changes,
            steps_ahead: changed.steps_ahead,
            files,
            remake: true,
            fold: false,
        }))
    }

    /// Rebuilds a document from `checkpoint.current_state()` after a post-mutation
    /// refusal in O(1) decode work rather than replaying `since`, preventing
    /// `O(since.len())` timeout amplification on rejected updates.
    ///
    /// When `checkpoint` was unsettled and `yrs` squashes adjacent blocks on read-back
    /// (`!same_state` on pass 1), runs a single `BodyDoc::settle` pass to converge to its
    /// canonical settled state while staying within `held_bytes`. If restoration fails,
    /// still diverges, or overflows `held_bytes`, releases the accounted bytes and fails
    /// the worker closed (`Err(FrameError)`) so the server cleanly reopens from the store.
    fn restore_after_dirty(
        &mut self,
        doc: DocNumber,
        checkpoint: Checkpoint,
    ) -> Result<(), FrameError> {
        let old_accounted = checkpoint.accounted_bytes();
        let base_held = self.held_bytes.saturating_sub(old_accounted);
        let expected = checkpoint.current_state();
        let rebuilt = BodyDoc::rebuild::<&[u8]>(expected, &[]).map_err(|error| {
            self.held_bytes = base_held;
            FrameError::Io(std::io::Error::other(format!(
                "failed to restore document after refusal: {error}"
            )))
        })?;
        let rebuilt_state = rebuilt.state();
        if same_state(&rebuilt_state, expected) {
            self.docs.insert(doc, checkpoint.into_held(rebuilt));
            return Ok(());
        }
        if checkpoint.settled {
            self.held_bytes = base_held;
            return Err(FrameError::Io(std::io::Error::other(
                "restored document state diverged from settled checkpoint",
            )));
        }
        // Drop `rebuilt` before settling so two full `Doc` instances are not held at once
        // on a path that exits the worker on any error.
        drop(rebuilt);
        let settled =
            BodyDoc::from_state(&rebuilt_state, &self.settle_limits()).map_err(|error| {
                self.held_bytes = base_held;
                FrameError::Io(std::io::Error::other(format!(
                    "restored unsettled document state failed to settle: {error}"
                )))
            })?;
        if settled.repair.update.is_some() {
            self.held_bytes = base_held;
            return Err(FrameError::Io(std::io::Error::other(
                "restored unsettled document required repair during settle",
            )));
        }
        let new_state_bytes = count(settled.state_bytes);
        if base_held.saturating_add(new_state_bytes) > self.limits.held_bytes {
            self.held_bytes = base_held;
            return Err(FrameError::Io(std::io::Error::other(
                "restored document state exceeded worker held_bytes budget",
            )));
        }
        self.held_bytes = base_held.saturating_add(new_state_bytes);
        self.docs.insert(
            doc,
            Held {
                doc: settled.doc,
                base: settled.state,
                since: Vec::new(),
                since_bytes: 0,
                last_state: None,
                state_bytes: new_state_bytes,
                changes: settled.changes,
                share: checkpoint.share,
                must_remake: false,
                settled: true,
                #[cfg(feature = "test-util")]
                fail_next_settle: checkpoint.fail_next_settle,
            },
        );
        Ok(())
    }

    /// The limits of what the server stored and logged before: an update and a state
    /// as large as the worker's memory for documents, a body as large as a body may
    /// be, and its changes placed past as many as they are. Its reading is counted
    /// all the same, before the library reads it, and held to what an update may
    /// leave ([`READING_STEPS`]): a stored state past it is refused unread, while a
    /// logged update past it (repaired in place if needed) comes back with `remake` set
    /// so the server writes its body and makes the document again without losing it.
    fn logged_limits(&self) -> UpdateLimits {
        let held = usize::try_from(self.limits.held_bytes).unwrap_or(usize::MAX);
        UpdateLimits {
            update_bytes: held,
            state_bytes: held,
            body_bytes: self.update_limits.body_bytes,
            placing_steps: None,
            clients: None,
            client_changes: None,
            reading_steps: Some(READING_STEPS),
        }
    }

    fn doc(&self, doc: DocNumber) -> Result<&BodyDoc, Refused> {
        self.docs
            .get(&doc)
            .map(|held| &held.doc)
            .ok_or_else(|| refused(Refusal::UnknownDocument, "no document is open under it"))
    }

    /// Returns a held document only if it is not waiting to be remade from its body
    /// (`!held.must_remake`), refusing with `on_remake` otherwise so a document past
    /// [`READING_STEPS`] or with a failed settle check is not served for diffs or state
    /// vectors. Documents with `held.settled == false` remain usable for normal live
    /// requests until settled at fold/close.
    fn usable_doc(&self, doc: DocNumber, on_remake: Refusal) -> Result<&BodyDoc, Refused> {
        let held = self
            .docs
            .get(&doc)
            .ok_or_else(|| refused(Refusal::UnknownDocument, "no document is open under it"))?;
        if held.must_remake {
            return Err(refused(
                on_remake,
                "the document must be made again from its body before another request",
            ));
        }
        Ok(&held.doc)
    }

    /// Holds `held` under `doc`, in place of what was held there, if the states and
    /// rollback logs of all held documents stay within [`Limits::held_bytes`].
    fn hold(&mut self, doc: DocNumber, held: Held) -> Result<(), Refused> {
        let before = self.docs.get(&doc).map_or(0, Held::accounted_bytes);
        let after = self
            .held_bytes
            .saturating_sub(before)
            .saturating_add(held.accounted_bytes());
        if after > self.limits.held_bytes {
            return Err(refused(
                Refusal::TooManyDocuments,
                "the worker holds as many bytes of documents as it may",
            ));
        }
        self.held_bytes = after;
        self.docs.insert(doc, held);
        Ok(())
    }

    fn open(&mut self, doc: DocNumber, source: Source, share: Share) -> Response {
        if self.docs.contains_key(&doc) {
            return Err(refused(
                Refusal::UnknownDocument,
                "a document is open under it already",
            ));
        }
        let of_share = self
            .docs
            .values()
            .filter(|held| held.share == share)
            .count();
        let limit = match share {
            Share::Kept => self.limits.documents,
            Share::Aside => u64::try_from(ASIDE_DOCUMENTS).unwrap_or(u64::MAX),
        };
        if u64::try_from(of_share).unwrap_or(u64::MAX) >= limit {
            return Err(refused(
                Refusal::TooManyDocuments,
                "the worker holds as many documents of that share as it may",
            ));
        }
        let (opened, base, reply_state, counts) = match source {
            Source::State(stored) => {
                // What the server stored is opened whatever the limits are now.
                let changed = BodyDoc::from_state(&stored, &self.logged_limits())
                    .map_err(|error| logged_refused(&error))?;
                let repaired = changed
                    .repair
                    .update
                    .is_some()
                    .then(|| changed.state.clone());
                let counts = (changed.changes, changed.client_changes, changed.steps_ahead);
                (changed.doc, changed.state, repaired, counts)
            }
            Source::Body(json) => {
                let body = json
                    .map(|json| serde_json::from_slice(&json).map(Body::from_stored))
                    .transpose()
                    .map_err(|error| refused(Refusal::NoBody, &error.to_string()))?;
                let made = BodyDoc::from_body(body.as_ref());
                let state = made.state();
                let state_bytes = size_of(&state);
                if state_bytes > self.limits.state_bytes {
                    return Err(refused(
                        Refusal::DocumentTooLarge,
                        "the document of the body is larger than the limit",
                    ));
                }
                // A body's own document is read in no steps, and in few once another
                // client has written into it: counted in one pass, not taken for it.
                let counted = fvoci_collab::state_counts(&state)
                    .map_err(|error| take_refused(&TakeError::Update(error)))?;
                let base = state.clone();
                (
                    made,
                    base,
                    Some(state),
                    (counted.changes, counted.client_changes, counted.steps_ahead),
                )
            }
        };
        let (changes, client_changes, steps_ahead) = counts;
        let state_bytes = size_of(&base);
        let clients = count(opened.clients());
        self.hold(
            doc,
            Held {
                doc: opened,
                base,
                since: Vec::new(),
                since_bytes: 0,
                last_state: None,
                state_bytes,
                changes,
                share,
                must_remake: false,
                settled: true,
                #[cfg(feature = "test-util")]
                fail_next_settle: false,
            },
        )?;
        Ok(Answer::Opened {
            state: reply_state,
            state_bytes,
            clients,
            client_changes,
            steps_ahead,
        })
    }
}

/// Whether a `take` is a live client update or a replay of a logged update.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TakeMode {
    Live,
    Retake,
}

/// The saved fields of a [`Held`] document while its [`BodyDoc`] is taken in place.
struct Checkpoint {
    base: Vec<u8>,
    since: Vec<Vec<u8>>,
    since_bytes: u64,
    last_state: Option<Vec<u8>>,
    state_bytes: u64,
    changes: u64,
    share: Share,
    settled: bool,
    #[cfg(feature = "test-util")]
    fail_next_settle: bool,
}

impl Checkpoint {
    fn current_state(&self) -> &[u8] {
        self.last_state.as_deref().unwrap_or(&self.base)
    }

    fn accounted_bytes(&self) -> u64 {
        self.state_bytes.saturating_add(self.since_bytes)
    }

    fn into_held(self, doc: BodyDoc) -> Held {
        Held {
            doc,
            base: self.base,
            since: self.since,
            since_bytes: self.since_bytes,
            last_state: self.last_state,
            state_bytes: self.state_bytes,
            changes: self.changes,
            share: self.share,
            must_remake: false,
            settled: self.settled,
            #[cfg(feature = "test-util")]
            fail_next_settle: self.fail_next_settle,
        }
    }
}

fn doc_meta(changed: &fvoci_collab::TakenInPlace) -> (u64, Vec<String>) {
    let clients = count(changed.doc.clients());
    let files = changed.doc.file_ids();
    (clients, files)
}

fn too_many_bytes() -> Refused {
    refused(
        Refusal::TooManyDocuments,
        "the worker holds as many bytes of documents as it may",
    )
}

fn size_of(bytes: &[u8]) -> u64 {
    count(bytes.len())
}

fn count(number: usize) -> u64 {
    u64::try_from(number).unwrap_or(u64::MAX)
}

fn json_of(body: &Body) -> Vec<u8> {
    // A `Value` is always JSON.
    serde_json::to_vec(body.document()).unwrap_or_default()
}

fn refused(refusal: Refusal, message: &str) -> Refused {
    Refused {
        refusal,
        message: message.to_owned(),
    }
}

/// The refusal of an update or a state that the document does not take.
fn take_refused(error: &TakeError) -> Refused {
    let refusal = match error {
        TakeError::Update(UpdateError::TooLarge { .. }) => Refusal::UpdateTooLarge,
        // Deletions out of order are what no encoder writes.
        TakeError::Update(UpdateError::Unreadable(_) | UpdateError::DeletionsOutOfOrder { .. }) => {
            Refusal::Unreadable
        }
        TakeError::Update(UpdateError::MissingChanges) => Refusal::MissingChanges,
        // A document of too many clients, or of a client of too many changes, is one
        // too large to work on in time.
        TakeError::Update(
            UpdateError::DocumentTooLarge { .. }
            | UpdateError::TooManyClients { .. }
            | UpdateError::TooManyChanges { .. },
        ) => Refusal::DocumentTooLarge,
        TakeError::Update(UpdateError::TooManyReadingSteps { .. }) => Refusal::TooManySteps,
        TakeError::Body(
            BodyError::TooLarge
            | BodyError::TooManyNodes
            | BodyError::TooDeep
            | BodyError::TableTooLarge { .. },
        )
        | TakeError::BodyTooLarge { .. } => Refusal::BodyTooLarge,
        TakeError::Body(_) => Refusal::NoBody,
        // What cannot be applied, and what would be placed past too much of the
        // document (`UpdateError::PlacedPastTooMany`): the room says a refused update.
        _ => Refusal::Unappliable,
    };
    refused(refusal, &error.to_string())
}

/// The refusal of what the server stored or logged before and this worker does not
/// take again. A state, or a logged update, past the steps a document may take to read
/// ([`READING_STEPS`]) is one this worker does not apply, whenever it is asked: the
/// room makes the document again from its card.
fn logged_refused(error: &TakeError) -> Refused {
    match error {
        TakeError::Update(UpdateError::TooManyReadingSteps { .. }) => {
            refused(Refusal::Unappliable, &error.to_string())
        }
        other => take_refused(other),
    }
}

#[cfg(test)]
mod tests;
