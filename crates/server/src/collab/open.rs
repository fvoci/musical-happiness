//! Opening a card's document from what is stored: its state and the updates logged
//! after it, or, the first time, its card's body.

use std::sync::atomic::Ordering;

use fvoci_model::{Block, BlockType, Body};
use fvoci_store::{StoreError, StoredDoc};
use tokio::time::Instant;

use super::place::{OpenDoc, Opening};
use super::worker::CallError;
use super::{Collab, DocChange, given, refusals, upkeep};
use crate::app::App;
use crate::collab_worker::wire::{Answer, DocNumber, Request, Source};
use crate::error::AppError;

impl Collab {
    /// Opens a card's document in the worker that holds the fewest: from what is
    /// stored, or made from the card's body if nothing is. A document whose body the
    /// card holds but that is no longer the body the card has was left behind by a
    /// write of the body that did not go through it (an import, a copy, a restore):
    /// it is made again from the card's body.
    pub(super) async fn open_stored(&self, app: &App, card_id: &str) -> Result<OpenDoc, Opening> {
        let block = card(app, card_id).await?;
        let worker = self.least_loaded();
        let number = self.next_number();
        let mut open = match app.store().get_body_doc(card_id).await? {
            None => self.make(app, &block, worker, number).await?,
            Some(stored) => self.reopen(app, &block, worker, number, stored).await?,
        };
        given(&self.inner.loads[worker]);
        if open.first_unwritten.is_some() {
            self.schedule_write(app, card_id, &mut open);
        }
        Ok(open)
    }

    /// The first document of a card, made from its body and stored.
    async fn make(
        &self,
        app: &App,
        block: &Block,
        worker: usize,
        number: DocNumber,
    ) -> Result<OpenDoc, Opening> {
        let (generation, state, state_bytes) = self
            .open_from_body(worker, number, block.body.as_ref())
            .await?;
        let digest = upkeep::digest_of(block.body.as_ref());
        if let Err(error) = app
            .store()
            .create_body_doc(&block.id, &state, &digest)
            .await
        {
            self.close_number(worker, generation, number).await;
            return Err(error.into());
        }
        Ok(OpenDoc {
            worker,
            generation,
            number,
            state_seq: 0,
            log_end: 0,
            written_seq: 0,
            body_digest: digest,
            state_bytes,
            last_writer: block.modified_by.clone(),
            first_unwritten: None,
            last_update: Instant::now(),
            writing_scheduled: false,
        })
    }

    /// A stored document, with the updates logged after its state taken again.
    async fn reopen(
        &self,
        app: &App,
        block: &Block,
        worker: usize,
        number: DocNumber,
        stored: StoredDoc,
    ) -> Result<OpenDoc, Opening> {
        let opened = self
            .call(
                worker,
                None,
                &Request::Open {
                    doc: number,
                    source: Source::State(stored.state),
                },
            )
            .await?;
        let (
            generation,
            Answer::Opened {
                mut state_bytes, ..
            },
        ) = opened
        else {
            return Err(unexpected(&opened.1).into());
        };
        let mut last_writer = block.modified_by.clone();
        for update in stored.updates {
            let request = Request::Take {
                doc: number,
                update: update.bytes,
            };
            match self.call(worker, Some(generation), &request).await {
                Ok((
                    _,
                    Answer::Taken {
                        state_bytes: now, ..
                    },
                )) => state_bytes = now,
                Ok((_, other)) => return Err(unexpected(&other).into()),
                Err(Opening::Refused(error)) => {
                    // Taken once, it was refused now: what was stored is kept as it is
                    // and the update is passed over, as nothing would take it later.
                    tracing::error!(%error, card_id = block.id, seq = update.seq, "a logged update is not taken again");
                }
                Err(died) => return Err(died),
            }
            if !update.written_by.is_empty() {
                last_writer = update.written_by;
            }
        }
        let unwritten = stored.projected_seq < stored.log_end;
        let mut open = OpenDoc {
            worker,
            generation,
            number,
            state_seq: stored.state_seq,
            log_end: stored.log_end,
            written_seq: stored.projected_seq,
            body_digest: stored.body_digest,
            state_bytes,
            last_writer,
            first_unwritten: unwritten.then(Instant::now),
            last_update: Instant::now(),
            writing_scheduled: false,
        };
        let written_elsewhere = upkeep::digest_of(block.body.as_ref()) != open.body_digest;
        if !unwritten && written_elsewhere {
            self.remake_from(app, &block.id, &mut open, block.body.as_ref())
                .await?;
            self.tell(&block.id, DocChange::Remade);
        }
        Ok(open)
    }

    /// Opens a document made from a body in a worker: its generation, state and size.
    pub(super) async fn open_from_body(
        &self,
        worker: usize,
        number: DocNumber,
        body: Option<&Body>,
    ) -> Result<(u64, Vec<u8>, u64), Opening> {
        let json = body
            .map(|body| serde_json::to_vec(body.document()))
            .transpose()
            .map_err(AppError::internal)?;
        let request = Request::Open {
            doc: number,
            source: Source::Body(json),
        };
        match self.call(worker, None, &request).await? {
            (
                generation,
                Answer::Opened {
                    state: Some(state),
                    state_bytes,
                },
            ) => Ok((generation, state, state_bytes)),
            (_, other) => Err(unexpected(&other).into()),
        }
    }

    /// Asks a worker a request while a document is opened.
    async fn call(
        &self,
        worker: usize,
        generation: Option<u64>,
        request: &Request,
    ) -> Result<(u64, Answer), Opening> {
        match self.inner.workers[worker].call(generation, request).await {
            Ok(answered) => Ok(answered),
            Err(CallError::Refused(refused)) => Err(Opening::Refused(refusals::refused(
                &refused,
                &self.inner.limits,
            ))),
            Err(CallError::Died(death)) => {
                tracing::warn!(%death, "a collab worker ended while it opened a document");
                Err(Opening::Died)
            }
            Err(CallError::Lost | CallError::Unavailable(_)) => {
                Err(Opening::Refused(refusals::unavailable()))
            }
        }
    }

    /// Closes a number in a worker, if the worker of that generation still holds it.
    pub(super) async fn close_number(&self, worker: usize, generation: u64, number: DocNumber) {
        let _ = self.inner.workers[worker]
            .call(Some(generation), &Request::Close(number))
            .await;
    }

    /// The worker that was given the fewest documents.
    fn least_loaded(&self) -> usize {
        self.inner
            .loads
            .iter()
            .enumerate()
            .min_by_key(|(_, load)| load.load(Ordering::Relaxed))
            .map_or(0, |(index, _)| index)
    }

    /// A number no document was opened under.
    pub(super) fn next_number(&self) -> DocNumber {
        self.inner.numbers.fetch_add(1, Ordering::Relaxed)
    }
}

/// The card of that identifier as `app` reaches it.
async fn card(app: &App, card_id: &str) -> Result<Block, Opening> {
    match app.store().get_block(card_id).await {
        Ok(block) if block.block_type == BlockType::Card => Ok(block),
        Ok(_) | Err(StoreError::NotFound(_)) => Err(AppError::not_found("card").into()),
        Err(error) => Err(error.into()),
    }
}

/// The error of an answer that is not one to the request: a defect.
pub(super) fn unexpected(answer: &Answer) -> AppError {
    AppError::internal(format_args!("the worker answered with {answer:?}"))
}
