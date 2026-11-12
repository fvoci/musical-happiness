//! Editing a card's body together: the one place that holds the Y document of each
//! body being edited, in a worker process ([`worker`]), and stores it.

#[cfg(unix)]
mod child;
pub mod worker;

/// Runs the executable as the worker ([`crate::collab_worker`]) if its first argument
/// says it was started as one: `None` if it was not, else how the worker ended. The
/// executable does nothing else then: no configuration, runtime or log.
#[must_use]
pub fn run_as_worker() -> Option<anyhow::Result<()>> {
    crate::collab_worker::run_if_started_as_worker()
        .map(|ended| ended.map_err(|error| anyhow::anyhow!("the collab worker ended: {error}")))
}
