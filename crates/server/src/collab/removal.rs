//! Trash snapshots include pending edits, and no document work crosses deletion.

use std::future::Future;
use std::sync::Arc;

use fvoci_model::BlockType;
use fvoci_store::{BlockFilter, Previews, Tenant};
#[cfg(feature = "test-util")]
use tokio::sync::Notify;
use tokio::sync::{OwnedRwLockReadGuard, OwnedRwLockWriteGuard, RwLock};

use super::Collab;
use crate::app::App;
use crate::error::AppError;

/// What a deletion takes to the trash, read while the card tree cannot change.
#[derive(Debug)]
pub(crate) enum Deletion {
    /// A block and its descendants, at any depth.
    Block(String),
    /// Every card of these boards.
    Boards(Vec<String>),
}

/// Keeps a structural store mutation on one side of a document deletion.
#[derive(Debug)]
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
    pub(crate) async fn remove<W, F>(
        &self,
        app: &App,
        deletion: Deletion,
        remove: W,
    ) -> Result<(), AppError>
    where
        W: FnOnce(App) -> F + Send + 'static,
        F: Future<Output = Result<(), AppError>> + Send + 'static,
    {
        let (collab, scoped) = (self.clone(), app.clone());
        app.spawn_tracked(async move { collab.remove_held(&scoped, deletion, remove).await })
            .await
            .map_err(|error| {
                AppError::internal(format_args!("the document deletion ended: {error}"))
            })?
    }

    /// Pauses after deletion, before the detached rooms are told, for restore races.
    #[cfg(feature = "test-util")]
    #[doc(hidden)]
    #[must_use]
    pub fn pause_next_deletion(&self) -> (Arc<Notify>, Arc<Notify>) {
        let pause = (Arc::new(Notify::new()), Arc::new(Notify::new()));
        *self
            .inner
            .deletion_pause
            .lock()
            .expect("the deletion pause is not poisoned") = Some(pause.clone());
        pause
    }

    #[cfg(feature = "test-util")]
    async fn paused_after_deletion(&self) {
        let pause = self
            .inner
            .deletion_pause
            .lock()
            .expect("the deletion pause is not poisoned")
            .take();
        if let Some((waiting, go)) = pause {
            waiting.notify_one();
            go.notified().await;
        }
    }

    async fn remove_held<W, F>(
        &self,
        app: &App,
        deletion: Deletion,
        remove: W,
    ) -> Result<(), AppError>
    where
        W: FnOnce(App) -> F,
        F: Future<Output = Result<(), AppError>>,
    {
        let documents = self.writing(app.store().tenant()).await;
        let card_ids = deletion.cards(app).await?;
        for card_id in &card_ids {
            let mut slot = self.slot(app, card_id).await;
            let mut opened_here = false;
            let written = self
                .write_first(app, card_id, &mut slot, &mut opened_here)
                .await;
            if opened_here {
                self.forget(&mut slot).await;
            }
            self.retire(&mut slot);
            written?;
        }
        remove(app.clone()).await?;
        // Mark and detach the old rooms before new document work can begin. A restored
        // card may open a new room while the old connections are still being told.
        let rooms = self.detach_deleted_rooms(app, &card_ids);
        for card_id in &card_ids {
            let mut slot = self.slot(app, card_id).await;
            self.forget(&mut slot).await;
            self.retire(&mut slot);
        }
        drop(documents);
        #[cfg(feature = "test-util")]
        self.paused_after_deletion().await;
        self.finish_deleted_rooms(rooms).await;
        Ok(())
    }
}

impl Deletion {
    async fn cards(&self, app: &App) -> Result<Vec<String>, AppError> {
        let mut cards = Vec::new();
        match self {
            Self::Block(id) => {
                let mut blocks = app.store().get_blocks_below(id).await?;
                blocks.push(app.store().get_block(id).await?);
                cards.extend(
                    blocks
                        .into_iter()
                        .filter(|block| block.block_type == BlockType::Card)
                        .map(|block| block.id),
                );
            }
            Self::Boards(ids) => {
                for id in ids {
                    let blocks = app
                        .store()
                        .get_listed_blocks(
                            &BlockFilter::board(id).with_type(BlockType::Card),
                            Previews::Without,
                        )
                        .await?;
                    cards.extend(blocks.into_iter().map(|listed| listed.block.id));
                }
            }
        }
        cards.sort();
        cards.dedup();
        Ok(cards)
    }
}
