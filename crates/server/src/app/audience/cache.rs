//! Reusable authorization reads at a committed store visibility revision.

use std::collections::HashMap;
use std::fmt;
use std::hash::Hash;
use std::sync::Mutex;

use fvoci_store::{Store, StoreError, VisibilityScope, VisibilityVersion};

/// Bounds memory even when a client probes arbitrary workspace identifiers.
const MAX_ENTRIES: usize = 4096;

pub(super) struct Cache<K, V> {
    state: Mutex<HashMap<VisibilityScope, Slot<K, V>>>,
}

/// What was read in one scope (an organization, or everything) at one revision.
struct Slot<K, V> {
    revision: VisibilityVersion,
    values: HashMap<K, V>,
}

impl<K, V> Default for Cache<K, V> {
    fn default() -> Self {
        Self {
            state: Mutex::new(HashMap::new()),
        }
    }
}

impl<K, V> fmt::Debug for Cache<K, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Some keys contain read tokens, which must never appear in diagnostics.
        f.debug_struct("Cache").finish_non_exhaustive()
    }
}

impl<K: Eq + Hash + Clone, V: Clone> Cache<K, V> {
    pub(super) async fn read<F: Future<Output = Result<V, StoreError>> + Send>(
        &self,
        store: &Store,
        key: K,
        read: impl Fn() -> F + Send,
    ) -> Result<V, StoreError> {
        loop {
            let revision = store.visibility_version().await;
            if !revision.cacheable() {
                return read().await;
            }
            {
                let mut slots = self.state.lock().expect("audience cache lock");
                let slot = slots.entry(revision.scope()).or_insert_with(|| Slot {
                    revision,
                    values: HashMap::new(),
                });
                if slot.revision != revision {
                    slot.values.clear();
                    slot.revision = revision;
                }
                if let Some(value) = slot.values.get(&key) {
                    return Ok(value.clone());
                }
            }
            let value = read().await?;
            if store.visibility_version().await != revision {
                continue;
            }
            let mut slots = self.state.lock().expect("audience cache lock");
            // Invalid workspace IDs must not grow a long-lived connection's cache
            // forever, across all organizations together.
            if slots.values().map(|slot| slot.values.len()).sum::<usize>() >= MAX_ENTRIES {
                slots.clear();
            }
            let slot = slots.entry(revision.scope()).or_insert_with(|| Slot {
                revision,
                values: HashMap::new(),
            });
            if slot.revision == revision {
                slot.values.insert(key.clone(), value.clone());
            }
            return Ok(value);
        }
    }
}
