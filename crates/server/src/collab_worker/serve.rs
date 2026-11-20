//! The worker's loop: a request in, its answer out, until the server closes the pipe.

use std::collections::HashMap;
use std::io::{Read, Write};
#[cfg(feature = "test-util")]
use std::time::Duration;

use fvoci_collab::{
    BodyDoc, MAX_CLIENT_CHANGES, MAX_CLIENTS, PLACING_STEPS, READING_STEPS, TakeError, UpdateError,
    UpdateLimits,
};
use fvoci_model::{Body, BodyError};

use super::frame::{FrameError, read_frame, write_frame};
use super::wire::{
    ASIDE_DOCUMENTS, Answer, DocNumber, Limits, Refusal, Refused, Request, Response, Share, Source,
    encode_response,
};

/// The largest frame before the limits are known: the one that brings them.
const CONFIGURE_FRAME_BYTES: u64 = 64;

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
        let response = worker.answer(request);
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
    /// The bytes of the states of all of them.
    held_bytes: u64,
}

/// A document, the size of its state, and the share it is counted in.
#[derive(Debug)]
struct Held {
    doc: BodyDoc,
    state_bytes: u64,
    share: Share,
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

    fn answer(&mut self, request: Request) -> Response {
        match request {
            Request::Configure(_) => Err(refused(
                Refusal::OutOfOrder,
                "the worker is configured once",
            )),
            Request::Open { doc, source, share } => self.open(doc, source, share),
            Request::Take { doc, update } => {
                self.take(doc, &update, self.update_limits, take_refused)
            }
            Request::Retake { doc, update } => {
                self.take(doc, &update, self.logged_limits(), logged_refused)
            }
            Request::StateVector(doc) => Ok(Answer::Bytes(self.doc(doc)?.state_vector())),
            Request::Diff { doc, state_vector } => self
                .doc(doc)?
                .diff(&state_vector)
                .map(Answer::Bytes)
                .map_err(|error| refused(Refusal::Unreadable, &error.to_string())),
            Request::State(doc) => Ok(Answer::Bytes(self.doc(doc)?.state())),
            Request::Body(doc) => {
                let body = self
                    .doc(doc)?
                    .body()
                    .map_err(|error| refused(Refusal::NoBody, &error.to_string()))?;
                Ok(Answer::Body(body.map(|body| json_of(&body))))
            }
            Request::Close(doc) => {
                if let Some(held) = self.docs.remove(&doc) {
                    self.held_bytes = self.held_bytes.saturating_sub(held.state_bytes);
                }
                Ok(Answer::Done)
            }
            #[cfg(feature = "test-util")]
            Request::Stall(millis) => {
                std::thread::sleep(Duration::from_millis(millis).min(LONGEST_STALL));
                Ok(Answer::Done)
            }
        }
    }

    /// Takes an update into a document within `limits`; `refused` is what one that
    /// is not taken is answered with.
    fn take(
        &mut self,
        doc: DocNumber,
        update: &[u8],
        limits: UpdateLimits,
        refused: fn(&TakeError) -> Refused,
    ) -> Response {
        let changed = self
            .doc(doc)?
            .take(update, &limits)
            .map_err(|error| refused(&error))?;
        let state_bytes = u64::try_from(changed.state_bytes).unwrap_or(u64::MAX);
        let clients = count(changed.doc.clients());
        let client_changes = changed.client_changes;
        let reading_steps = changed.reading_steps;
        let files = changed
            .repair
            .body
            .as_ref()
            .map(|body| body.file_ids().into_iter().map(str::to_owned).collect())
            .unwrap_or_default();
        let share = self.docs.get(&doc).map_or(Share::Kept, |held| held.share);
        self.hold(doc, changed.doc, state_bytes, share)?;
        Ok(Answer::Taken {
            repair: changed.repair.update,
            state_bytes,
            clients,
            client_changes,
            reading_steps,
            files,
        })
    }

    /// The limits of what the server stored and logged before: an update and a state
    /// as large as the worker's memory for documents, a body as large as a body may
    /// be, and its changes placed past as many as they are. Its reading is counted
    /// all the same, before the library reads it, and held to what an update may
    /// leave ([`READING_STEPS`]): every update taken since readings are counted left
    /// a document within it, and a state stored before may take longer to read than
    /// this worker has.
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

    /// Holds `held` under `doc`, in place of what was held there, if the states of all
    /// the documents stay within [`Limits::held_bytes`].
    fn hold(
        &mut self,
        doc: DocNumber,
        held: BodyDoc,
        state_bytes: u64,
        share: Share,
    ) -> Result<(), Refused> {
        let before = self.docs.get(&doc).map_or(0, |held| held.state_bytes);
        let after = self
            .held_bytes
            .saturating_sub(before)
            .saturating_add(state_bytes);
        if after > self.limits.held_bytes {
            return Err(refused(
                Refusal::TooManyDocuments,
                "the worker holds as many bytes of documents as it may",
            ));
        }
        self.held_bytes = after;
        self.docs.insert(
            doc,
            Held {
                doc: held,
                state_bytes,
                share,
            },
        );
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
        let (opened, state) = match source {
            Source::State(stored) => {
                // What the server stored is opened whatever the limits are now.
                let changed = BodyDoc::from_state(&stored, &self.logged_limits())
                    .map_err(|error| logged_refused(&error))?;
                let repaired = (changed.repair.deleted > 0).then(|| changed.doc.state());
                let state_bytes = size_of(repaired.as_deref().unwrap_or(&stored));
                let counts = (changed.client_changes, changed.reading_steps);
                (changed.doc, (repaired, state_bytes, counts))
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
                let client_changes = fvoci_collab::client_changes(&state)
                    .map_err(|error| take_refused(&TakeError::Update(error)))?;
                // None, as a body's own document is made: counted, not taken for it.
                let reading_steps = fvoci_collab::reading_steps(&state, READING_STEPS)
                    .map_err(|error| take_refused(&TakeError::Update(error)))?;
                (
                    made,
                    (Some(state), state_bytes, (client_changes, reading_steps)),
                )
            }
        };
        let (state, state_bytes, (client_changes, reading_steps)) = state;
        let clients = count(opened.clients());
        self.hold(doc, opened, state_bytes, share)?;
        Ok(Answer::Opened {
            state,
            state_bytes,
            clients,
            client_changes,
            reading_steps,
        })
    }
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
        TakeError::Body(BodyError::TooLarge | BodyError::TooManyNodes | BodyError::TooDeep)
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
mod tests {
    use super::super::frame::read_frame;
    use super::super::wire::decode_response;
    use super::*;

    const LIMITS: Limits = Limits {
        update_bytes: 1024,
        state_bytes: 4096,
        body_bytes: 4096,
        documents: 1,
        held_bytes: 1 << 20,
    };

    /// The answers of a worker to `requests`, the configuration of [`LIMITS`] first.
    fn answers(requests: &[Request]) -> Vec<Response> {
        let mut input = Vec::new();
        write_frame(&mut input, &Request::Configure(LIMITS).encode(), 64).expect("written");
        for request in requests {
            write_frame(&mut input, &request.encode(), 1 << 20).expect("written");
        }
        let mut output = Vec::new();
        serve(&mut input.as_slice(), &mut output).expect("served");
        let mut output = output.as_slice();
        let configured = read_frame(&mut output, 1 << 20).expect("an answer");
        assert_eq!(
            configured.map(|frame| decode_response(&frame)),
            Some(Ok(Ok(Answer::Done))),
            "the configuration is answered"
        );
        let mut answers = Vec::new();
        while let Some(frame) = read_frame(&mut output, 1 << 20).expect("an answer") {
            answers.push(decode_response(&frame).expect("an answer"));
        }
        answers
    }

    #[test]
    fn a_document_opens_once_and_within_the_number_of_documents() {
        let body = br#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"x"}]}]}"#;
        let got = answers(&[
            Request::Open {
                doc: 1,
                source: Source::Body(Some(body.to_vec())),
                share: Share::Kept,
            },
            Request::Open {
                doc: 1,
                source: Source::Body(None),
                share: Share::Kept,
            },
            Request::Open {
                doc: 2,
                source: Source::Body(None),
                share: Share::Kept,
            },
            Request::Body(1),
            Request::Close(1),
            Request::Body(1),
        ]);
        assert!(matches!(got[0], Ok(Answer::Opened { state: Some(_), .. })));
        let refusal = |index: usize| got[index].as_ref().map_err(|refused| refused.refusal).err();
        assert_eq!(refusal(1), Some(Refusal::UnknownDocument), "open already");
        assert_eq!(refusal(2), Some(Refusal::TooManyDocuments), "one at a time");
        let Ok(Answer::Body(Some(json))) = &got[3] else {
            panic!("its body: {:?}", got[3]);
        };
        let json: serde_json::Value = serde_json::from_slice(json).expect("JSON");
        let sent: serde_json::Value = serde_json::from_slice(body).expect("JSON");
        assert_eq!(json, sent, "its body");
        assert_eq!(refusal(5), Some(Refusal::UnknownDocument), "closed");
    }

    #[test]
    fn a_worker_that_holds_as_many_documents_as_it_may_opens_some_aside() {
        let open = |doc: DocNumber, share: Share| Request::Open {
            doc,
            source: Source::Body(None),
            share,
        };
        let aside: Vec<DocNumber> = (10..).take(ASIDE_DOCUMENTS + 1).collect();
        let mut requests = vec![open(1, Share::Kept)];
        requests.extend(aside.iter().map(|doc| open(*doc, Share::Aside)));
        requests.extend([
            Request::Close(10),
            open(30, Share::Aside),
            open(2, Share::Kept),
        ]);
        let got = answers(&requests);
        let refused = |index: usize| got[index].as_ref().map_err(|refused| refused.refusal).err();
        let last = ASIDE_DOCUMENTS;
        assert!((0..=last).all(|index| refused(index).is_none()), "{got:?}");
        assert_eq!(
            refused(last + 1),
            Some(Refusal::TooManyDocuments),
            "past the share aside"
        );
        assert_eq!(refused(last + 3), None, "one closed aside makes room aside");
        assert_eq!(
            refused(last + 4),
            Some(Refusal::TooManyDocuments),
            "none kept past the limit"
        );
    }

    #[test]
    fn documents_aside_leave_the_kept_share_alone() {
        let open = |doc: DocNumber, share: Share| Request::Open {
            doc,
            source: Source::Body(None),
            share,
        };
        let mut requests: Vec<Request> = (10..)
            .take(ASIDE_DOCUMENTS)
            .map(|doc| open(doc, Share::Aside))
            .collect();
        requests.push(open(1, Share::Kept));
        let got = answers(&requests);
        assert!(got.iter().all(Result::is_ok), "{got:?}");
    }

    #[test]
    fn a_worker_that_is_sent_no_configuration_first_ends() {
        let mut input = Vec::new();
        write_frame(&mut input, &Request::State(1).encode(), 64).expect("written");
        assert!(serve(&mut input.as_slice(), &mut Vec::new()).is_err());
        assert!(
            serve(&mut [].as_slice(), &mut Vec::new()).is_ok(),
            "a pipe closed at once"
        );
    }
}
