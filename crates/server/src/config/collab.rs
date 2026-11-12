//! The settings of editing a body together: the worker processes and their limits
//! (`crate::collab`, `crate::collab_worker`).

use anyhow::bail;
use serde::Deserialize;

/// The variables of the environment of these keys ([`super::ENV_KEYS`]).
pub(super) const ENV_KEYS: [(&str, super::EnvKind); 6] = [
    ("collab_workers", super::EnvKind::Number),
    ("collab_documents_per_worker", super::EnvKind::Number),
    ("collab_state_bytes", super::EnvKind::Number),
    ("collab_update_bytes", super::EnvKind::Number),
    ("collab_worker_timeout_ms", super::EnvKind::Number),
    ("collab_worker_memory_bytes", super::EnvKind::Number),
];

/// The keys `collab_*` of the configuration, at its top level.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(serde::Serialize))]
#[serde(default)]
pub struct CollabSettings {
    /// How many worker processes hold the documents. 1 to 64.
    pub collab_workers: u32,
    /// How many documents one worker holds at once. At least 1. A worker also holds no
    /// more of their states, together, than an eighth of `collab_worker_memory_bytes`.
    pub collab_documents_per_worker: u32,
    /// The largest document, in bytes of its stored state. 2 MiB to 1 GiB.
    pub collab_state_bytes: u64,
    /// The largest update a client sends, in bytes. At least 1, at most
    /// `collab_state_bytes`.
    pub collab_update_bytes: u64,
    /// How long, in milliseconds, the server waits for a worker's answer before it
    /// kills the worker and starts another. At least 1.
    pub collab_worker_timeout_ms: u64,
    /// The address space a worker may take, in bytes, where the system limits it
    /// (Unix). At least 256 MiB.
    pub collab_worker_memory_bytes: u64,
    /// How a worker is started; `None` for this executable. Not a configuration file
    /// key: the tests, whose executable is no server, set it.
    #[serde(skip)]
    pub worker_command: Option<crate::collab::worker::WorkerCommand>,
}

/// The default of `collab_state_bytes`: 8 MiB.
pub(super) const DEFAULT_STATE_BYTES: u64 = 8 * 1024 * 1024;

/// The default of `collab_update_bytes`: 64 KiB. An update made to place many changes
/// where many others claim the same place costs the worker the square of its size.
pub(super) const DEFAULT_UPDATE_BYTES: u64 = 64 * 1024;

/// The most workers.
pub(super) const MOST_WORKERS: u32 = 64;

/// The least `collab_state_bytes`: a body may be 1 MiB of JSON, and its document is
/// larger than that.
pub(super) const LEAST_STATE_BYTES: u64 = 2 * 1024 * 1024;

/// The most `collab_state_bytes`: a frame between the server and a worker counts its
/// length in four bytes.
pub(super) const MOST_STATE_BYTES: u64 = 1024 * 1024 * 1024;

/// The least `collab_worker_memory_bytes`: the executable and a document at its limit,
/// with the copies an update makes of it.
pub(super) const LEAST_WORKER_MEMORY_BYTES: u64 = 256 * 1024 * 1024;

/// How many bytes of a worker's memory every byte of the states of its documents
/// takes: a document in memory is larger than its state, and an update makes a copy of
/// it and reads its state again. A worker holds states of `collab_worker_memory_bytes`
/// divided by this, together.
pub(super) const MEMORY_PER_STATE_BYTE: u64 = 8;

impl Default for CollabSettings {
    fn default() -> Self {
        Self {
            collab_workers: 1,
            collab_documents_per_worker: 500,
            collab_state_bytes: DEFAULT_STATE_BYTES,
            collab_update_bytes: DEFAULT_UPDATE_BYTES,
            collab_worker_timeout_ms: 10_000,
            collab_worker_memory_bytes: 1024 * 1024 * 1024,
            worker_command: None,
        }
    }
}

impl CollabSettings {
    /// The bytes of the states of all its documents a worker may hold together.
    #[must_use]
    pub fn held_bytes(&self) -> u64 {
        self.collab_worker_memory_bytes / MEMORY_PER_STATE_BYTE
    }

    /// Checks the settings against their ranges.
    ///
    /// # Errors
    ///
    /// Returns an error naming the first setting out of its range.
    pub fn check(&self) -> anyhow::Result<()> {
        if self.collab_workers == 0 || self.collab_workers > MOST_WORKERS {
            bail!(
                "collab_workers must be 1 to {MOST_WORKERS}, not {}",
                self.collab_workers
            );
        }
        if self.collab_documents_per_worker == 0 {
            bail!("collab_documents_per_worker must be at least 1");
        }
        if !(LEAST_STATE_BYTES..=MOST_STATE_BYTES).contains(&self.collab_state_bytes) {
            bail!(
                "collab_state_bytes must be {LEAST_STATE_BYTES} to {MOST_STATE_BYTES}, not {}",
                self.collab_state_bytes
            );
        }
        if self.collab_update_bytes == 0 || self.collab_update_bytes > self.collab_state_bytes {
            bail!(
                "collab_update_bytes must be 1 to collab_state_bytes ({}), not {}",
                self.collab_state_bytes,
                self.collab_update_bytes
            );
        }
        if self.collab_worker_timeout_ms == 0 {
            bail!("collab_worker_timeout_ms must be at least 1");
        }
        if self.collab_worker_memory_bytes / MEMORY_PER_STATE_BYTE < self.collab_state_bytes {
            bail!(
                "collab_worker_memory_bytes must be at least {MEMORY_PER_STATE_BYTE} times \
                 collab_state_bytes ({}), not {}",
                self.collab_state_bytes,
                self.collab_worker_memory_bytes
            );
        }
        if self.collab_worker_memory_bytes < LEAST_WORKER_MEMORY_BYTES {
            bail!(
                "collab_worker_memory_bytes must be at least {LEAST_WORKER_MEMORY_BYTES}, not {}",
                self.collab_worker_memory_bytes
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_in_range_and_each_bound_is_checked() {
        CollabSettings::default().check().expect("the defaults");
        let refused = |change: fn(&mut CollabSettings), key: &str| {
            let mut settings = CollabSettings::default();
            change(&mut settings);
            let error = settings.check().expect_err(key).to_string();
            assert!(error.contains(key), "{error}");
        };
        refused(|s| s.collab_workers = 0, "collab_workers");
        refused(|s| s.collab_workers = MOST_WORKERS + 1, "collab_workers");
        refused(
            |s| s.collab_documents_per_worker = 0,
            "collab_documents_per_worker",
        );
        refused(
            |s| s.collab_state_bytes = LEAST_STATE_BYTES - 1,
            "collab_state_bytes",
        );
        refused(
            |s| s.collab_state_bytes = MOST_STATE_BYTES + 1,
            "collab_state_bytes",
        );
        refused(|s| s.collab_update_bytes = 0, "collab_update_bytes");
        refused(
            |s| s.collab_update_bytes = s.collab_state_bytes + 1,
            "collab_update_bytes",
        );
        refused(
            |s| s.collab_worker_timeout_ms = 0,
            "collab_worker_timeout_ms",
        );
        refused(
            |s| s.collab_worker_memory_bytes = LEAST_WORKER_MEMORY_BYTES - 1,
            "collab_worker_memory_bytes",
        );
    }
}
