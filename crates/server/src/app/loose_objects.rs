//! Removing the objects of the file storage that are doomed: their record went, or they
//! never had one (`fvoci_store::LooseObject`).
//!
//! The store lists an object as doomed in the transaction that removes its record. The
//! housekeeping removes it here, and takes it off the list only once it is gone: a
//! server that stops in between finds it listed again and removes it again, which for
//! an object that is gone already is nothing. A removal that fails is tried again
//! later, each time later still, up to [`RETRY_CAP_MILLIS`]; and a run takes the due
//! ones the longest due first, so that objects that keep failing do not hold back the
//! others. One at a place no storage takes is taken off the list with an error logged.

use fvoci_store::LooseObject;

use super::App;
use crate::error::AppError;
use crate::storage::StorageError;

/// How many doomed objects a run reads from the store at once.
const DOOMED_PER_BATCH: i64 = 200;

/// The most doomed objects a run tries to remove; the rest wait for the next run.
const DOOMED_PER_RUN: usize = 5_000;

/// How long after its first failure a removal is tried again; each further failure
/// doubles it, up to [`RETRY_CAP_MILLIS`].
const RETRY_BASE_MILLIS: i64 = 15 * 60 * 1000;

/// The longest a doomed object waits between two tries.
const RETRY_CAP_MILLIS: i64 = 24 * 60 * 60 * 1000;

/// What a run of the removal of doomed objects did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RemovedObjects {
    /// The objects removed, or found gone, and taken off the list.
    pub removed: usize,
    /// The objects that could not be removed, or taken off the list, and wait for
    /// another try.
    pub failed: usize,
}

impl App {
    /// Dooms the objects whose writing stopped long ago, then removes the doomed
    /// objects that are due at `now`, up to [`DOOMED_PER_RUN`]. What cannot be removed
    /// is noted with the kind of its error and tried again later.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the list cannot be read.
    pub async fn remove_doomed_objects(&self, now: i64) -> Result<RemovedObjects, AppError> {
        // What cannot be doomed now is doomed by a later run; the removals go on.
        match self.store().doom_stale_writing(now).await {
            Ok(0) => {}
            Ok(stale) => tracing::warn!(stale, "doomed objects whose writing stopped long ago"),
            Err(error) => {
                tracing::error!(%error, "unable to doom the objects whose writing stopped");
            }
        }
        let mut done = RemovedObjects::default();
        while done.removed + done.failed < DOOMED_PER_RUN {
            let due = self
                .store()
                .due_doomed_objects(now, DOOMED_PER_BATCH)
                .await?;
            let last_batch = i64::try_from(due.len()).unwrap_or(i64::MAX) < DOOMED_PER_BATCH;
            let mut removed = 0;
            for object in &due {
                if self.remove_doomed_object(object, now).await {
                    removed += 1;
                }
            }
            done.removed += removed;
            done.failed += due.len() - removed;
            // A batch of which nothing went says that the storage or the store is
            // failing: the rest waits for the next run rather than fail as well.
            if last_batch || removed == 0 {
                break;
            }
        }
        Ok(done)
    }

    /// Removes one doomed object and takes it off the list. Returns whether both went;
    /// if not, the failure is noted on its row, or the row is left as it is.
    async fn remove_doomed_object(&self, object: &LooseObject, now: i64) -> bool {
        let error_kind = match self.files().delete(&object.path).await {
            // Gone already is removed: a run cut off after the removal comes here.
            Ok(()) => match self.store().forget_doomed_object(&object.path).await {
                Ok(()) => return true,
                Err(error) => {
                    tracing::warn!(%error, "a removed object could not be taken off the list");
                    return false;
                }
            },
            // A place no storage takes is none to remove: tried again, it would fail
            // for ever.
            Err(error @ StorageError::InvalidPath(_)) => {
                tracing::error!(%error, "a doomed object has a place no storage takes; it is taken off the list");
                return match self.store().forget_doomed_object(&object.path).await {
                    Ok(()) => true,
                    Err(error) => {
                        tracing::warn!(%error, "a doomed object could not be taken off the list");
                        false
                    }
                };
            }
            Err(error) => {
                tracing::warn!(%error, attempts = object.attempts + 1, "a doomed object could not be removed");
                error_kind(&error)
            }
        };
        let retry_at = now.saturating_add(retry_delay(object.attempts + 1));
        if let Err(error) = self
            .store()
            .doomed_object_failed(&object.path, now, error_kind, retry_at)
            .await
        {
            tracing::warn!(%error, "a failed removal could not be noted");
        }
        false
    }
}

/// The kind of a storage error, as a failed removal notes it and the metrics count it:
/// one of a few words, never a path or a key.
pub(crate) fn error_kind(error: &StorageError) -> &'static str {
    match error {
        StorageError::NotFound(_) => "not_found",
        StorageError::InvalidPath(_) => "invalid_path",
        StorageError::Config(_) => "config",
        StorageError::Io(_) => "io",
        StorageError::ObjectStore(_) => "object_store",
    }
}

/// How long to wait before the next try of a removal that failed `attempts` times.
fn retry_delay(attempts: i64) -> i64 {
    let doublings = u32::try_from(attempts.saturating_sub(1).clamp(0, 30)).unwrap_or(30);
    RETRY_BASE_MILLIS
        .saturating_mul(1_i64 << doublings)
        .min(RETRY_CAP_MILLIS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_removal_is_tried_again_later_each_time_up_to_a_day() {
        assert_eq!(retry_delay(1), RETRY_BASE_MILLIS, "after the first failure");
        assert_eq!(retry_delay(2), 2 * RETRY_BASE_MILLIS, "then twice as long");
        assert_eq!(retry_delay(3), 4 * RETRY_BASE_MILLIS, "and so on");
        assert_eq!(retry_delay(8), RETRY_CAP_MILLIS, "up to a day");
        assert_eq!(retry_delay(i64::MAX), RETRY_CAP_MILLIS, "however many");
        assert_eq!(retry_delay(0), RETRY_BASE_MILLIS, "never sooner");
    }
}
