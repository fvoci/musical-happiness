//! Working off what the search server is still to be told (`Store::due_search_changes`).
//!
//! A row of the list says that a block, or everything of a board, changed. The worker
//! reads what is stored of it now, with the reach of the housekeeping (the list is of
//! every organization), sends that, and takes the row off unless it was noted again
//! meanwhile. A document the search server would not take leaves its row on the list,
//! to be tried again later and later; a search server that does not answer at all
//! leaves every row as it is, and the upkeep pauses.

use std::collections::HashMap;

use fvoci_store::{SearchChange, SearchDocument, SearchIndexMark, SearchTarget, Store};

use super::client::{Client, ClientError, IndexFound, Operation};
use super::index;
use super::query::Scope;
use crate::auth::now_millis;
use crate::error::AppError;
use crate::metrics::{Metrics, SearchSent};

/// How many rows of the list one round reads.
const ROUND: i64 = 200;
/// How many rounds one run of the upkeep works off at most: the rest is the next
/// run's, a second later.
const ROUNDS_A_RUN: usize = 5;
/// How many documents are read, and sent, at a time: a body may be a megabyte.
const DOCUMENTS_AT_ONCE: usize = 50;
/// [`DOCUMENTS_AT_ONCE`], as the limit of a query.
const PAGE: i64 = 50;
/// How many refused documents of one container are named in the log, each by a line.
const REFUSALS_NAMED: usize = 5;
/// How long a row whose document was refused waits for its next try, at first and at
/// most; doubled with every refusal in a row.
const FIRST_RETRY_MILLIS: i64 = 5_000;
/// See [`FIRST_RETRY_MILLIS`].
const LONGEST_RETRY_MILLIS: i64 = 10 * 60 * 1000;

/// Why a run stopped before the list was worked off.
#[derive(Debug)]
pub(super) enum Stopped {
    /// The search server did not take a request: nothing it was asked is done.
    Search(ClientError),
    /// The index is not there, or is not the one the list is kept for: nothing was
    /// written to it.
    IndexLost,
    /// The database could not be read or written.
    Store(AppError),
}

impl From<ClientError> for Stopped {
    fn from(error: ClientError) -> Self {
        Self::Search(error)
    }
}

impl From<fvoci_store::StoreError> for Stopped {
    fn from(error: fvoci_store::StoreError) -> Self {
        Self::Store(error.into())
    }
}

/// When a row that failed for the `attempts`-th time in a row is tried again.
fn retry_at(now: i64, attempts: i64) -> i64 {
    let doublings = u32::try_from(attempts.clamp(0, 16)).unwrap_or(16);
    let wait = FIRST_RETRY_MILLIS
        .saturating_mul(2_i64.saturating_pow(doublings))
        .min(LONGEST_RETRY_MILLIS);
    now.saturating_add(wait)
}

/// Whether the index on the search server is the one the list is kept for. Asked
/// before anything is written: both search servers make an index that is not there
/// when a document is put into it, with no analyser of ours and under no wall, and
/// answer as if nothing were amiss.
async fn index_is_kept(store: &Store, client: &Client) -> Result<bool, Stopped> {
    let IndexFound::Ours { version, uuid } = client.describe_index().await? else {
        return Ok(false);
    };
    let found = SearchIndexMark {
        name: client.index().to_owned(),
        uuid,
        version,
    };
    Ok(version == index::VERSION && store.search_index_mark().await? == Some(found))
}

/// Works off the rows that are due, a few rounds of them. `store` reaches every
/// organization. Returns how many rows it looked at.
///
/// # Errors
///
/// Returns why it stopped: the index is not the one the list is kept for, the search
/// server did not take a request, or the database could not be asked. The rows it
/// had not settled are as they were.
pub(super) async fn work_off(
    store: &Store,
    client: &Client,
    metrics: &Metrics,
) -> Result<usize, Stopped> {
    let mut looked_at = 0;
    for _ in 0..ROUNDS_A_RUN {
        let changes = store.due_search_changes(now_millis(), ROUND).await?;
        if changes.is_empty() {
            break;
        }
        // Before every round: a run goes on for a while when everything is sent again.
        if !index_is_kept(store, client).await? {
            return Err(Stopped::IndexLost);
        }
        looked_at += changes.len();
        let (boards, blocks): (Vec<SearchChange>, Vec<SearchChange>) = changes
            .into_iter()
            .partition(|change| change.target == SearchTarget::Board);
        for board in &boards {
            send_board(store, client, metrics, board).await?;
        }
        for blocks in blocks.chunks(DOCUMENTS_AT_ONCE) {
            send_blocks(store, client, metrics, blocks).await?;
        }
    }
    Ok(looked_at)
}

/// Sends what is stored of the blocks of `changes` now: the document of each that
/// has one, and the removal of the document of each that has none.
async fn send_blocks(
    store: &Store,
    client: &Client,
    metrics: &Metrics,
    changes: &[SearchChange],
) -> Result<(), Stopped> {
    let ids: Vec<&str> = changes.iter().map(|change| change.id.as_str()).collect();
    let mut documents: HashMap<String, SearchDocument> = store
        .search_documents(&ids)
        .await?
        .into_iter()
        .map(|document| (document.id.clone(), document))
        .collect();
    let operations: Vec<Operation> = changes
        .iter()
        .map(|change| match documents.remove(&change.id) {
            Some(document) => Operation::Put(document),
            None => Operation::Remove(change.id.clone()),
        })
        .collect();
    let refusals = client.write(&operations).await?;
    for (change, refusal) in changes.iter().zip(refusals) {
        settle(store, metrics, change, refusal.as_deref()).await?;
    }
    Ok(())
}

/// Sends everything of a board as it is stored now: removes every document the
/// search server holds of it, those of cards and pages that are gone among them, and
/// puts the documents it has now. A board or a wiki that is gone, or is a template,
/// has none.
async fn send_board(
    store: &Store,
    client: &Client,
    metrics: &Metrics,
    change: &SearchChange,
) -> Result<(), Stopped> {
    // What was put a moment ago is found by the removal only once the index was
    // refreshed.
    client.refresh().await?;
    client
        .remove_documents(&Scope::Container {
            org_id: &change.org_id,
            container_id: &change.id,
        })
        .await?;
    // Every page is put, also after a document was refused: what is refused is one
    // document, and the others of the board are not left out for it.
    let mut after: Option<String> = None;
    let mut refused: Option<String> = None;
    let mut refused_count = 0_usize;
    loop {
        let page = store
            .search_documents_of_container(&change.id, after.as_deref(), PAGE)
            .await?;
        let Some(last) = page.last() else {
            break;
        };
        after = Some(last.id.clone());
        let operations: Vec<Operation> = page.into_iter().map(Operation::Put).collect();
        let refusals = client.write(&operations).await?;
        for (operation, kind) in operations.iter().zip(refusals) {
            let (Operation::Put(document), Some(kind)) = (operation, kind) else {
                continue;
            };
            // Which documents, so that somebody can look at them: the first few of a
            // container, and how many in all below.
            if refused_count < REFUSALS_NAMED {
                tracing::warn!(
                    kind,
                    id = %document.id,
                    container_id = %change.id,
                    "the search server refused a document of a container sent whole"
                );
            }
            refused_count += 1;
            refused.get_or_insert(kind);
        }
    }
    if refused_count > REFUSALS_NAMED {
        tracing::warn!(
            container_id = %change.id,
            refused = refused_count,
            "the search server refused more documents of the container than were named"
        );
    }
    settle(store, metrics, change, refused.as_deref()).await
}

/// Takes a row off the list once what it is about was sent, or notes when to try
/// again what the search server refused (`refusal`, the name of its error).
async fn settle(
    store: &Store,
    metrics: &Metrics,
    change: &SearchChange,
    refusal: Option<&str>,
) -> Result<(), Stopped> {
    match refusal {
        None => {
            // A row noted again meanwhile stays, and is counted when it is sent again.
            if store.settle_search_change(change).await? {
                metrics.search_sent(SearchSent::Done);
            }
        }
        Some(kind) => {
            tracing::warn!(
                kind,
                id = %change.id,
                attempts = change.attempts.saturating_add(1),
                "the search server refused a document; it is tried again later"
            );
            store
                .search_change_failed(change, retry_at(now_millis(), change.attempts))
                .await?;
            metrics.search_sent(SearchSent::Refused);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refused_row_waits_twice_as_long_each_time_up_to_ten_minutes() {
        assert_eq!(retry_at(1000, 0), 6000);
        assert_eq!(retry_at(1000, 1), 11_000);
        assert_eq!(retry_at(1000, 3), 41_000);
        assert_eq!(retry_at(0, 20), LONGEST_RETRY_MILLIS);
        assert_eq!(retry_at(0, i64::MAX), LONGEST_RETRY_MILLIS);
        assert_eq!(retry_at(0, -1), FIRST_RETRY_MILLIS);
    }
}
