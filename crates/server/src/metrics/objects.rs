//! The metrics of the objects of the file storage that may have no record: how many are
//! listed, being written or doomed, and how many doomed ones keep failing to go. Set
//! after each run of the housekeeping that removes them.

use prometheus::{IntGaugeVec, Opts, Registry};

use super::{Metrics, PREFIX};

/// The states of a listed object, the values of the `state` label.
const STATES: [&str; 2] = ["writing", "doomed"];

/// The kinds of the error of a failed removal, the values of the `error` label; a
/// kind the server does not know is counted as `other`.
const ERRORS: [&str; 6] = [
    "io",
    "object_store",
    "config",
    "not_found",
    "invalid_path",
    "other",
];

/// The listed objects, as the housekeeping counted them after a run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct LooseObjects {
    /// Being written.
    pub(crate) writing: i64,
    /// Doomed.
    pub(crate) doomed: i64,
    /// The doomed ones whose removal failed at least once, by the kind of the error of
    /// the last failure.
    pub(crate) failing: Vec<(String, i64)>,
}

/// The metrics of the listed objects.
#[derive(Debug)]
pub(super) struct ObjectMetrics {
    listed: IntGaugeVec,
    failing: IntGaugeVec,
}

impl ObjectMetrics {
    /// Makes the metrics and registers them in `registry`, every label value at zero.
    pub(super) fn register(registry: &Registry) -> prometheus::Result<Self> {
        let listed = IntGaugeVec::new(
            Opts::new(
                "loose_objects",
                "The objects of the file storage that may have no record, by state: \
                 writing (its record is not written yet), doomed (to be removed). Counted \
                 after each run of the housekeeping that removes them.",
            )
            .namespace(PREFIX),
            &["state"],
        )?;
        registry.register(Box::new(listed.clone()))?;
        let failing = IntGaugeVec::new(
            Opts::new(
                "loose_object_removals_failing",
                "The doomed objects whose removal failed and was tried again at least \
                 once, by the kind of the error of the last try: io, object_store, \
                 config, not_found, invalid_path, other. Counted with loose_objects.",
            )
            .namespace(PREFIX),
            &["error"],
        )?;
        registry.register(Box::new(failing.clone()))?;
        for state in STATES {
            listed.with_label_values(&[state]).set(0);
        }
        for error in ERRORS {
            failing.with_label_values(&[error]).set(0);
        }
        Ok(Self { listed, failing })
    }
}

impl Metrics {
    /// Sets the counts of the listed objects.
    pub(crate) fn loose_objects_counted(&self, counted: &LooseObjects) {
        let Some(kept) = &self.kept else {
            return;
        };
        let objects = &kept.objects;
        objects
            .listed
            .with_label_values(&["writing"])
            .set(counted.writing);
        objects
            .listed
            .with_label_values(&["doomed"])
            .set(counted.doomed);
        let mut by_error = [0_i64; ERRORS.len()];
        for (error, count) in &counted.failing {
            let index = ERRORS
                .iter()
                .position(|known| known == error)
                .unwrap_or(ERRORS.len() - 1);
            by_error[index] = by_error[index].saturating_add(*count);
        }
        for (error, count) in ERRORS.iter().zip(by_error) {
            objects.failing.with_label_values(&[error]).set(count);
        }
    }

    /// The count of listed objects in `state`, and of failing removals of `error`, for
    /// tests.
    #[cfg(test)]
    fn loose_object_counts(&self, state: &str, error: &str) -> (i64, i64) {
        self.kept.as_ref().map_or((0, 0), |kept| {
            (
                kept.objects.listed.with_label_values(&[state]).get(),
                kept.objects.failing.with_label_values(&[error]).get(),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kind_of_error_the_server_does_not_know_is_other() {
        let metrics = Metrics::kept().expect("the metrics register");
        metrics.loose_objects_counted(&LooseObjects {
            writing: 2,
            doomed: 5,
            failing: vec![
                ("io".to_owned(), 3),
                ("a path/that/is no kind".to_owned(), 1),
            ],
        });
        assert_eq!(metrics.loose_object_counts("writing", "io"), (2, 3));
        assert_eq!(metrics.loose_object_counts("doomed", "other"), (5, 1));
        assert_eq!(
            metrics.loose_object_counts("doomed", "object_store"),
            (5, 0),
            "a kind that did not fail is zero"
        );
        let text = String::from_utf8(
            metrics
                .render()
                .expect("kept")
                .expect("the metrics are written"),
        )
        .expect("text");
        assert!(
            !text.contains("a path"),
            "no label carries what the store said: {text}"
        );
    }
}
