//! Reusable authorization reads at a committed store visibility revision.

use std::collections::HashMap;
use std::fmt;
use std::hash::Hash;
use std::sync::Mutex;

use fvoci_store::{Store, StoreError, VisibilityVersion};

/// Bounds memory even when a client probes arbitrary workspace identifiers.
const MAX_ENTRIES: usize = 4096;

pub(super) struct Cache<K, V> {
    state: Mutex<State<K, V>>,
}

struct State<K, V> {
    revision: Option<VisibilityVersion>,
    values: HashMap<K, V>,
}

impl<K, V> Default for Cache<K, V> {
    fn default() -> Self {
        Self {
            state: Mutex::new(State {
                revision: None,
                values: HashMap::new(),
            }),
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
                let mut state = self.state.lock().expect("audience cache lock");
                if state.revision != Some(revision) {
                    state.values.clear();
                    state.revision = Some(revision);
                }
                if let Some(value) = state.values.get(&key) {
                    return Ok(value.clone());
                }
            }
            let value = read().await?;
            if store.visibility_version().await != revision {
                continue;
            }
            let mut state = self.state.lock().expect("audience cache lock");
            if state.revision == Some(revision) {
                // Invalid workspace IDs must not grow a long-lived connection's cache forever.
                if state.values.len() >= MAX_ENTRIES {
                    state.values.clear();
                }
                state.values.insert(key.clone(), value.clone());
            }
            return Ok(value);
        }
    }
}
