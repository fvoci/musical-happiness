//! Trash snapshots include pending edits, and no document work crosses deletion.

use std::future::Future;
use std::sync::Arc;

use fvoci_model::BlockType;
use fvoci_store::{BlockFilter, Previews, Tenant};
#[cfg(feature = "test-util")]
use tokio::sync::Notify;
use tokio::sync::{OwnedRwLockReadGuard, OwnedRwLockWriteGuard, RwLock};

use super::{Collab, place};
use crate::app::App;
use crate::error::AppError;

/// What a deletion takes to the trash, read while the tree of cards and pages cannot
/// change.
#[derive(Debug)]
pub(crate) enum Deletion {
    /// A block and its descendants, at any depth.
    Block(String),
    /// Every card of these boards, and every page of these wikis.
    Containers(Vec<String>),
}

/// Keeps a structural store mutation on one side of a document deletion.
#[derive(Debug)]
#[must_use = "the gate is let go when this is dropped"]
pub(crate) struct StructureChange {
    _held: OwnedRwLockReadGuard<()>,
}

impl Collab {
    /// Hold only around direct store writes, never around another collab operation:
    /// a deletion waiting for the gate would hold the second read back.
    pub(crate) async fn changing_structure(&self, app: &App) -> StructureChange {
        StructureChange {
            _held: self.reading(app.store().tenant()).await,
        }
    }

    /// The gate of an organization's documents: document work reads it, a deletion
    /// writes it. Organizations do not wait for each other's deletions.
    fn gate(&self, tenant: Tenant) -> Arc<RwLock<()>> {
        let mut gates = self.inner.gates.lock().expect("the gates are not poisoned");
        Arc::clone(gates.entry(tenant).or_default())
    }

    /// Taken before any slot of the organization, never while one is held.
    pub(super) async fn reading(&self, tenant: Tenant) -> OwnedRwLockReadGuard<()> {
        self.gate(tenant).read_owned().await
    }

    async fn writing(&self, tenant: Tenant) -> OwnedRwLockWriteGuard<()> {
        self.gate(tenant).write_owned().await
    }

    /// Writes pending bodies before the trash snapshot, then closes the deleted rooms.
    /// The task owns the entire operation even when its HTTP caller goes away.
    pub(crate) async fn remove<T, W, F>(
        &self,
        app: &App,
        deletion: Deletion,
        remove: W,
    ) -> Result<T, AppError>
    where
        T: Send + 'static,
        W: FnOnce(App) -> F + Send + 'static,
        F: Future<Output = Result<T, AppError>> + Send + 'static,
    {
        let (collab, scoped) = (self.clone(), app.clone());
        app.spawn_tracked(async move { collab.remove_held(&scoped, deletion, remove).await })
            .await
            .map_err(|error| {
                AppError::internal(format_args!("the document deletion ended: {error}"))
            })?
    }

    /// Pauses the next deletion at `stage` until the second is notified; the first is
    /// notified when it gets there. For the tests of what happens meanwhile.
    #[cfg(feature = "test-util")]
    #[doc(hidden)]
    #[must_use]
    pub fn pause_next_deletion(&self, stage: DeletionStage) -> (Arc<Notify>, Arc<Notify>) {
        let (waiting, go) = (Arc::new(Notify::new()), Arc::new(Notify::new()));
        *self
            .inner
            .deletion_pause
            .lock()
            .expect("the deletion pause is not poisoned") =
            Some((stage, Arc::clone(&waiting), Arc::clone(&go)));
        (waiting, go)
    }

    #[cfg(feature = "test-util")]
    async fn paused_at(&self, stage: DeletionStage) {
        let pause = {
            let mut pause = self
                .inner
                .deletion_pause
                .lock()
                .expect("the deletion pause is not poisoned");
            pause
                .take_if(|(at, _, _)| *at == stage)
                .map(|(_, waiting, go)| (waiting, go))
        };
        if let Some((waiting, go)) = pause {
            waiting.notify_one();
            go.notified().await;
        }
    }

    async fn remove_held<T, W, F>(
        &self,
        app: &App,
        deletion: Deletion,
        remove: W,
    ) -> Result<T, AppError>
    where
        W: FnOnce(App) -> F,
        F: Future<Output = Result<T, AppError>>,
    {
        // A block that holds no card (a comment, a view) takes no document along: the
        // organization's documents do not wait for it.
        if matches!(deletion, Deletion::Block(_)) && deletion.cards(app).await?.is_empty() {
            return remove(app.clone()).await;
        }
        let documents = self.writing(app.store().tenant()).await;
        let card_ids = deletion.cards(app).await?;
        self.write_deleted(app, &card_ids).await;
        #[cfg(feature = "test-util")]
        self.paused_at(DeletionStage::BeforeRemoving).await;
        let result = remove(app.clone()).await?;
        // Mark and detach the old rooms before new document work can begin. A restored
        // card may open a new room while the old connections are still being told.
        let rooms = self.detach_deleted_rooms(app, &card_ids);
        for card_id in &card_ids {
            if let Some(mut slot) = self.existing_slot(app, card_id).await {
                self.forget(&mut slot).await;
                self.retire(&mut slot);
            }
        }
        drop(documents);
        #[cfg(feature = "test-util")]
        self.paused_at(DeletionStage::AfterRemoving).await;
        self.finish_deleted_rooms(rooms).await;
        Ok(result)
    }

    /// Writes to the cards about to be deleted what their documents hold and they do
    /// not. What cannot be written stays in the log, and the card restored opens with
    /// it; so does all of it if the store cannot say which logs are not written.
    async fn write_deleted(&self, app: &App, card_ids: &[String]) {
        let open = self.open_among(app, card_ids);
        let pending = match app.store().with_unwritten_bodies(card_ids).await {
            Ok(unwritten) => merged(open, unwritten),
            Err(error) => {
                tracing::error!(%error, "cannot find the deleted cards' unwritten logs; those closed keep them");
                open
            }
        };
        for card_id in &pending {
            let mut slot = self.slot(app, card_id).await;
            if let Err(error) = self
                .write_pending(app, card_id, &mut slot, super::whole::DueVersion::Keep)
                .await
            {
                tracing::error!(%error, card_id, "cannot write a deleted card's body; its log keeps it");
            }
            self.retire(&mut slot);
        }
    }

    /// Those of `card_ids` whose document may hold what the card does not: open here,
    /// or with a log not written to the card, found in one query. The open ones are
    /// read first: a document that closes between the two without writing its body
    /// leaves its log for the query.
    async fn pending_among(&self, app: &App, card_ids: &[String]) -> Result<Vec<String>, AppError> {
        let open = self.open_among(app, card_ids);
        let unwritten = app.store().with_unwritten_bodies(card_ids).await?;
        Ok(merged(open, unwritten))
    }

    /// Those of `card_ids` that have a slot here: a document open, or closed for a while.
    fn open_among(&self, app: &App, card_ids: &[String]) -> Vec<String> {
        let tenant = app.store().tenant();
        let slots = self.inner.slots.lock().expect("the slots are not poisoned");
        card_ids
            .iter()
            .filter(|id| slots.contains_key(&place::key_of(tenant, id)))
            .cloned()
            .collect()
    }

    /// [`Self::write_body`] for many cards: of those that may hold something unwritten
    /// only, found at once, so a board of cards nobody edits costs one query.
    ///
    /// # Errors
    ///
    /// The errors of [`Self::write_body`], and the store's.
    pub(crate) async fn write_bodies(
        &self,
        app: &App,
        card_ids: &[String],
    ) -> Result<(), AppError> {
        for card_id in self.pending_among(app, card_ids).await? {
            self.write_body(app, &card_id).await?;
        }
        Ok(())
    }
}

/// The cards of both lists, each once.
fn merged(open: Vec<String>, unwritten: Vec<String>) -> Vec<String> {
    let mut pending: Vec<String> = open.into_iter().chain(unwritten).collect();
    pending.sort();
    pending.dedup();
    pending
}

/// Where a deletion is paused for a test ([`Collab::pause_next_deletion`]).
#[cfg(feature = "test-util")]
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeletionStage {
    /// The bodies are written, the gate is held, the store has not deleted yet.
    BeforeRemoving,
    /// The store deleted and the rooms are detached; the gate is let go and the
    /// rooms' members are not told yet.
    AfterRemoving,
}

/// A deletion paused at a stage: told when it gets there, and waiting to go on.
#[cfg(feature = "test-util")]
pub(super) type DeletionPause = (DeletionStage, Arc<Notify>, Arc<Notify>);

impl Deletion {
    async fn cards(&self, app: &App) -> Result<Vec<String>, AppError> {
        let mut cards = Vec::new();
        match self {
            Self::Block(id) => {
                // Deleted meanwhile: deleting it again takes nothing along.
                let root = match app.store().get_block(id).await {
                    Ok(root) => root,
                    Err(error) if error.is_not_found() => return Ok(cards),
                    Err(error) => return Err(error.into()),
                };
                let mut blocks = app.store().get_blocks_below(id).await?;
                blocks.push(root);
                cards.extend(
                    blocks
                        .into_iter()
                        .filter(|block| block.block_type.has_room())
                        .map(|block| block.id),
                );
            }
            Self::Containers(ids) => {
                // A board has cards and a wiki pages: each is asked for both, as its
                // kind is not read here.
                for id in ids {
                    for with_room in BlockType::WITH_ROOM {
                        let blocks = app
                            .store()
                            .get_listed_blocks(
                                &BlockFilter::container(id).with_type(with_room),
                                Previews::Without,
                            )
                            .await?;
                        cards.extend(blocks.into_iter().map(|listed| listed.block.id));
                    }
                }
            }
        }
        cards.sort();
        cards.dedup();
        Ok(cards)
    }
}
