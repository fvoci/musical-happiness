//! The metrics of HTTP requests on the API's listener: how many are being answered, and
//! how long each took, by method, route and status. What a request's method, route and
//! status are is for the middleware that measures (`request_metrics`) to say.

use std::time::Duration;

use prometheus::{HistogramOpts, HistogramVec, IntGauge, Opts, Registry};

use super::{Metrics, PREFIX};

/// The upper bounds of the buckets of the request durations, in seconds: from a
/// cached read to an import or an export of a large archive.
const DURATION_BUCKETS: [f64; 14] = [
    0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0,
];

/// The metrics of HTTP requests.
#[derive(Debug)]
pub(crate) struct RequestMetrics {
    /// How long requests took until their answer's head, by method, route and status.
    duration: HistogramVec,
    /// How many requests are being answered.
    in_flight: IntGauge,
}

impl RequestMetrics {
    /// Makes the metrics and registers them in `registry`.
    pub(super) fn register(registry: &Registry) -> prometheus::Result<Self> {
        let duration = HistogramVec::new(
            HistogramOpts::new(
                "http_request_duration_seconds",
                "How long requests to the API's listener took until the head of their \
                 answer, by method (or other), route template (or unmatched) and status (or \
                 abandoned, when the client went away first); its count is the number \
                 of requests.",
            )
            .namespace(PREFIX)
            .buckets(DURATION_BUCKETS.to_vec()),
            &["method", "route", "status"],
        )?;
        let in_flight = IntGauge::with_opts(
            Opts::new(
                "http_requests_in_flight",
                "How many requests to the API's listener are being answered, until the \
                 head of their answer.",
            )
            .namespace(PREFIX),
        )?;
        registry.register(Box::new(duration.clone()))?;
        registry.register(Box::new(in_flight.clone()))?;
        Ok(Self {
            duration,
            in_flight,
        })
    }

    /// Counts a request as being answered, until [`Self::ended`].
    pub(crate) fn started(&self) {
        self.in_flight.inc();
    }

    /// Counts a request as no longer being answered.
    pub(crate) fn ended(&self) {
        self.in_flight.dec();
    }

    /// Measures a request that took `duration` until the head of its answer. The
    /// labels are the caller's to keep to a fixed set: none is what a client sent.
    pub(crate) fn took(&self, method: &str, route: &str, status: &str, duration: Duration) {
        self.duration
            .with_label_values(&[method, route, status])
            .observe(duration.as_secs_f64());
    }
}

impl Metrics {
    /// The metrics of HTTP requests; `None` if no metrics are kept.
    pub(crate) fn requests(&self) -> Option<&RequestMetrics> {
        self.kept.as_ref().map(|kept| &kept.requests)
    }
}
