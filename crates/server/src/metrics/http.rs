//! The metrics of HTTP requests on the API's listener: how many are being answered, and
//! how long each took, by method, route and status.

use std::sync::Arc;
use std::time::Instant;

use axum::extract::{MatchedPath, Request, State};
use axum::http::Method;
use axum::middleware::Next;
use axum::response::Response;
use prometheus::{HistogramOpts, HistogramVec, IntGauge, Opts, Registry};

use super::{Metrics, PREFIX};

/// The route of a request that matched none: a page of the web app, a path that is no
/// route, under the API or elsewhere.
pub(crate) const UNMATCHED: &str = "unmatched";

/// The method of a request that is none of the methods the server has routes for.
const OTHER_METHOD: &str = "other";

/// The upper bounds of the buckets of the request durations, in seconds: from a
/// cached read to an import or an export of a large archive.
const DURATION_BUCKETS: [f64; 14] = [
    0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0,
];

/// The metrics of HTTP requests.
#[derive(Debug)]
pub(super) struct HttpMetrics {
    /// How long requests took until their answer's head, by method, route and status.
    duration: HistogramVec,
    /// How many requests are being answered.
    in_flight: IntGauge,
}

impl HttpMetrics {
    /// Makes the metrics and registers them in `registry`.
    pub(super) fn register(registry: &Registry) -> prometheus::Result<Self> {
        let duration = HistogramVec::new(
            HistogramOpts::new(
                "http_request_duration_seconds",
                "How long requests to the API's listener took until the head of their \
                 answer, by method, route template (or unmatched) and status; its count \
                 is the number of requests.",
            )
            .namespace(PREFIX)
            .buckets(DURATION_BUCKETS.to_vec()),
            &["method", "route", "status"],
        )?;
        let in_flight = IntGauge::with_opts(
            Opts::new(
                "http_requests_in_flight",
                "How many requests to the API's listener are being answered.",
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
}

/// The template of the route a request was routed to, as the router knows it: carried
/// out on the answer from where routing is done to where the request is measured.
#[derive(Debug, Clone)]
struct RouteTemplate(Arc<str>);

/// Notes on the answer the template of the route that answered, for
/// [`measure_requests`]. A layer of the routes only: requests that match no route do
/// not pass here, and are measured as [`UNMATCHED`].
pub(crate) async fn note_route(request: Request, next: Next) -> Response {
    let template = request
        .extensions()
        .get::<MatchedPath>()
        .map(|path| RouteTemplate(Arc::from(path.as_str())));
    let mut response = next.run(request).await;
    if let Some(template) = template {
        response.extensions_mut().insert(template);
    }
    response
}

/// What [`measure_requests`] needs: the metrics, and the base path the routes are
/// nested under, which is left out of the route label.
#[derive(Debug, Clone)]
pub(crate) struct Measured {
    pub(crate) metrics: Metrics,
    pub(crate) base_path: Arc<str>,
}

/// Counts the request as being answered until its answer's head is ready, and then its
/// duration by method, route template and status. The outermost layer of the API's
/// listener, so that what the inner layers answer, a contained panic among them, is
/// measured too.
pub(crate) async fn measure_requests(
    State(measured): State<Measured>,
    request: Request,
    next: Next,
) -> Response {
    let Some(kept) = measured.metrics.kept.as_ref() else {
        return next.run(request).await;
    };
    let http = &kept.http;
    let method = method_label(request.method());
    let started = Instant::now();
    // Dropped also if the request is given up halfway, when the client goes away.
    let _answering = Answering::start(&http.in_flight);
    let response = next.run(request).await;
    let route = response
        .extensions()
        .get::<RouteTemplate>()
        .map_or(UNMATCHED, |RouteTemplate(template)| {
            route_label(template, &measured.base_path)
        });
    http.duration
        .with_label_values(&[method, route, response.status().as_str()])
        .observe(started.elapsed().as_secs_f64());
    response
}

/// The method as a label: one of the methods with routes, or [`OTHER_METHOD`], so that
/// a client cannot make a label of its own.
fn method_label(method: &Method) -> &'static str {
    match *method {
        Method::GET => "GET",
        Method::HEAD => "HEAD",
        Method::POST => "POST",
        Method::PUT => "PUT",
        Method::PATCH => "PATCH",
        Method::DELETE => "DELETE",
        Method::OPTIONS => "OPTIONS",
        _ => OTHER_METHOD,
    }
}

/// The route template as a label: as the router has it, without the base path.
fn route_label<'a>(template: &'a str, base_path: &str) -> &'a str {
    match template.strip_prefix(base_path) {
        Some("") => "/",
        Some(rest) if rest.starts_with('/') => rest,
        _ => template,
    }
}

/// A request being answered: counted in the gauge while it lives.
struct Answering<'a>(&'a IntGauge);

impl<'a> Answering<'a> {
    fn start(gauge: &'a IntGauge) -> Self {
        gauge.inc();
        Self(gauge)
    }
}

impl Drop for Answering<'_> {
    fn drop(&mut self) {
        self.0.dec();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_method_without_routes_is_one_label() {
        assert_eq!(method_label(&Method::PATCH), "PATCH");
        for name in ["TRACE", "CONNECT", "PROPFIND", "X-ANYTHING"] {
            let method = Method::from_bytes(name.as_bytes()).expect("a method");
            assert_eq!(method_label(&method), OTHER_METHOD, "{name}");
        }
    }

    #[test]
    fn the_base_path_is_left_out_of_the_route() {
        assert_eq!(
            route_label("/api/v1/boards/{boardId}", ""),
            "/api/v1/boards/{boardId}"
        );
        assert_eq!(
            route_label("/boards/api/v1/hello", "/boards"),
            "/api/v1/hello"
        );
        assert_eq!(route_label("/boards/", "/boards"), "/");
        assert_eq!(route_label("/boards", "/boards"), "/");
    }
}
