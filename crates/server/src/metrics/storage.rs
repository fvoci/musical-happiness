//! The metrics of the database and the files: the connection pool, the open uploads,
//! and the exports and imports of archives under way.

use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::body::{Body, Bytes, HttpBody};
use axum::extract::{MatchedPath, Request, State};
use axum::middleware::Next;
use axum::response::Response;
use http_body::{Frame, SizeHint};
use prometheus::{IntCounter, IntGauge, IntGaugeVec, Opts, Registry};

use super::{Metrics, PREFIX};
use crate::app::App;

/// How long a scrape waits for the database to count the open uploads. Shorter than
/// Prometheus's own timeout of a scrape (10 s by default), so that a pool that is
/// exhausted, which the pool's metrics are there to show, does not cost the scrape.
const UPLOADS_READ_TIMEOUT: Duration = Duration::from_secs(2);

/// The work on archives that is counted while it is under way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArchiveWork {
    /// An archive is written and sent: from the request until the last byte is sent
    /// or the client goes away.
    Export,
    /// An archive is read and its boards are made: for the whole request.
    Import,
}

impl ArchiveWork {
    /// The routes of the work: the template of a route of the API without the API's
    /// prefix, as the contract writes it.
    const ROUTES: [(&'static str, Self); 3] = [
        ("/boards/{boardId}/archive/export", Self::Export),
        ("/teams/{teamId}/archive/export", Self::Export),
        ("/teams/{teamId}/archive/import", Self::Import),
    ];

    /// The work a request of the route `template` does, if it is one of these. The
    /// template may have the base path before the API's prefix.
    pub(crate) fn of_route(template: &str) -> Option<Self> {
        Self::ROUTES.iter().find_map(|(route, work)| {
            template
                .strip_suffix(route)
                .is_some_and(|before| before.ends_with(crate::api::PREFIX))
                .then_some(*work)
        })
    }

    /// The value of the `work` label.
    fn as_str(self) -> &'static str {
        match self {
            Self::Export => "export",
            Self::Import => "import",
        }
    }
}

/// The metrics of the database and the files.
#[derive(Debug)]
pub(super) struct StorageMetrics {
    pool_connections: IntGauge,
    pool_idle: IntGauge,
    pool_max: IntGauge,
    uploads_open: IntGauge,
    uploads_open_bytes: IntGauge,
    uploads_read_failures: IntCounter,
    /// Held while a scrape counts the open uploads: one at a time, and a scrape that
    /// finds it held answers with the values counted before.
    reading_uploads: tokio::sync::Mutex<()>,
    archives: IntGaugeVec,
}

impl StorageMetrics {
    /// Makes the metrics and registers them in `registry`.
    pub(super) fn register(registry: &Registry) -> prometheus::Result<Self> {
        let gauge = |name: &str, help: &str| {
            let gauge = IntGauge::with_opts(Opts::new(name, help).namespace(PREFIX))?;
            registry.register(Box::new(gauge.clone()))?;
            Ok::<_, prometheus::Error>(gauge)
        };
        let pool_connections = gauge(
            "db_pool_connections",
            "How many connections to the database the pool holds, in use or idle.",
        )?;
        let pool_idle = gauge(
            "db_pool_idle_connections",
            "How many of the pool's connections to the database are idle.",
        )?;
        let pool_max = gauge(
            "db_pool_max_connections",
            "How many connections to the database the pool may hold.",
        )?;
        let uploads_open = gauge(
            "uploads_open",
            "How many uploads in parts are open: neither completed, abandoned nor ended.",
        )?;
        let uploads_open_bytes = gauge(
            "uploads_open_bytes",
            "The sizes of the open uploads together, in bytes: what they hold back of \
             max_pending_upload_bytes.",
        )?;
        let uploads_read_failures = IntCounter::with_opts(
            Opts::new(
                "uploads_read_failures_total",
                "How many scrapes could not count the open uploads (the database failed \
                 or took over two seconds) and gave the values counted before.",
            )
            .namespace(PREFIX),
        )?;
        registry.register(Box::new(uploads_read_failures.clone()))?;
        let archives = IntGaugeVec::new(
            Opts::new(
                "archives_in_progress",
                "How many archives are being exported (written and sent) or imported, \
                 by work: export, import.",
            )
            .namespace(PREFIX),
            &["work"],
        )?;
        registry.register(Box::new(archives.clone()))?;
        // Both values are there from the start, at zero.
        for work in [ArchiveWork::Export, ArchiveWork::Import] {
            archives.with_label_values(&[work.as_str()]).set(0);
        }
        Ok(Self {
            pool_connections,
            pool_idle,
            pool_max,
            uploads_open,
            uploads_open_bytes,
            uploads_read_failures,
            reading_uploads: tokio::sync::Mutex::new(()),
            archives,
        })
    }
}

/// Work on an archive under way, counted while it lives.
#[derive(Debug)]
pub(crate) struct ArchiveInProgress(Option<IntGauge>);

impl Drop for ArchiveInProgress {
    fn drop(&mut self) {
        if let Some(gauge) = &self.0 {
            gauge.dec();
        }
    }
}

impl ArchiveInProgress {
    /// The answer's body, holding this until it is sent or dropped.
    pub(crate) fn hold_until_sent(self, body: Body) -> Body {
        Body::new(HeldBody { body, _held: self })
    }
}

/// A body that holds a value until it is dropped.
struct HeldBody {
    body: Body,
    _held: ArchiveInProgress,
}

impl HttpBody for HeldBody {
    type Data = Bytes;
    type Error = axum::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, axum::Error>>> {
        Pin::new(&mut self.body).poll_frame(cx)
    }

    fn is_end_stream(&self) -> bool {
        self.body.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.body.size_hint()
    }
}

impl Metrics {
    /// Counts work on an archive as under way until the returned value is dropped.
    pub(crate) fn archive_started(&self, work: ArchiveWork) -> ArchiveInProgress {
        ArchiveInProgress(self.kept.as_ref().map(|kept| {
            let gauge = kept.storage.archives.with_label_values(&[work.as_str()]);
            gauge.inc();
            gauge
        }))
    }

    /// Reads what is measured by asking, for a scrape: the pool and the open uploads.
    /// The uploads stay as they were if the database cannot be read in time, which is
    /// counted, or another scrape is reading them.
    pub(super) async fn read_storage(&self, app: &App) {
        let Some(kept) = &self.kept else {
            return;
        };
        let storage = &kept.storage;
        let pool = app.store().pool_status();
        storage.pool_connections.set(i64::from(pool.connections));
        storage.pool_idle.set(i64::from(pool.idle));
        storage.pool_max.set(i64::from(pool.max_connections));
        let Ok(_reading) = storage.reading_uploads.try_lock() else {
            return;
        };
        let read = tokio::time::timeout(
            UPLOADS_READ_TIMEOUT,
            app.store().open_uploads(crate::auth::now_millis()),
        )
        .await;
        match read {
            Ok(Ok(open)) => {
                storage.uploads_open.set(open.sessions);
                storage.uploads_open_bytes.set(open.bytes);
            }
            Ok(Err(error)) => {
                storage.uploads_read_failures.inc();
                tracing::warn!(%error, "a scrape could not count the open uploads");
            }
            Err(_) => {
                storage.uploads_read_failures.inc();
                tracing::warn!("a scrape could not count the open uploads in time");
            }
        }
    }
}

/// Counts the export or import of an archive as under way while it is: an import for
/// its request, an export until its answer is sent. A layer of the API's routes inside
/// [`crate::api::run_to_completion`], so that the count lives in the task that does the
/// work, which goes on when the client goes away.
pub(crate) async fn count_archives(
    State(metrics): State<Metrics>,
    request: Request,
    next: Next,
) -> Response {
    let work = request
        .extensions()
        .get::<MatchedPath>()
        .and_then(|path| ArchiveWork::of_route(path.as_str()));
    let Some(work) = work else {
        return next.run(request).await;
    };
    let in_progress = metrics.archive_started(work);
    let response = next.run(request).await;
    if work == ArchiveWork::Export && response.status().is_success() {
        let (parts, body) = response.into_parts();
        Response::from_parts(parts, in_progress.hold_until_sent(body))
    } else {
        response
    }
}

#[cfg(test)]
mod tests {
    use axum::Router;
    use axum::http::StatusCode;
    use axum::middleware;
    use axum::routing::{get, post};
    use tower::ServiceExt as _;

    use super::*;

    /// Each route of [`ArchiveWork::ROUTES`] is a route of the contract with the
    /// method of its work, so that a route that is renamed is not silently never
    /// counted.
    #[test]
    fn the_archive_routes_are_routes_of_the_contract() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../contracts/openapi.json");
        let text = std::fs::read_to_string(&path).expect("the contract is read");
        let contract: serde_json::Value = serde_json::from_str(&text).expect("JSON");
        for (route, work) in ArchiveWork::ROUTES {
            let method = match work {
                ArchiveWork::Export => "get",
                ArchiveWork::Import => "post",
            };
            let path = format!("{}{route}", crate::api::PREFIX);
            assert!(
                contract["paths"][path.as_str()][method].is_object(),
                "{method} {path} is in the contract"
            );
        }
    }

    /// Runs the rest of the request as a task of its own, as `run_to_completion` does.
    async fn in_a_task(request: Request, next: Next) -> Response {
        tokio::spawn(next.run(request))
            .await
            .expect("the request's task ends")
    }

    fn archives(metrics: &Metrics, work: &str) -> String {
        let text = metrics
            .render()
            .expect("the metrics are kept")
            .expect("the metrics encode");
        let text = String::from_utf8(text).expect("UTF-8");
        let series = format!("fvoci_archives_in_progress{{work=\"{work}\"}} ");
        text.lines()
            .find_map(|line| line.strip_prefix(&series))
            .expect("the series is there")
            .to_owned()
    }

    fn router(metrics: &Metrics, started: tokio::sync::mpsc::Sender<()>) -> Router {
        async fn exports() -> &'static str {
            "an archive"
        }
        Router::new()
            .route("/api/v1/boards/{boardId}/archive/export", get(exports))
            .route(
                "/api/v1/teams/{teamId}/archive/import",
                post(move || async move {
                    let _ = started.send(()).await;
                    tokio::time::sleep(Duration::from_millis(300)).await;
                    StatusCode::NO_CONTENT
                }),
            )
            .route_layer(middleware::from_fn_with_state(
                metrics.clone(),
                count_archives,
            ))
            .route_layer(middleware::from_fn(in_a_task))
    }

    #[tokio::test]
    async fn an_export_is_under_way_until_its_answer_is_sent() {
        let metrics = Metrics::kept().expect("the metrics register");
        let (started, _) = tokio::sync::mpsc::channel(1);
        let request = Request::get("/api/v1/boards/b1/archive/export")
            .body(Body::empty())
            .expect("a request");
        let response = router(&metrics, started)
            .oneshot(request)
            .await
            .expect("an answer");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(archives(&metrics, "export"), "1", "being sent");
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .expect("the body is read");
        assert_eq!(body, "an archive");
        assert_eq!(archives(&metrics, "export"), "0", "sent");
    }

    #[tokio::test]
    async fn an_import_whose_client_went_away_is_under_way_until_it_is_done() {
        let metrics = Metrics::kept().expect("the metrics register");
        let (started, mut has_started) = tokio::sync::mpsc::channel(1);
        let request = Request::post("/api/v1/teams/t1/archive/import")
            .body(Body::empty())
            .expect("a request");
        let answering = tokio::spawn(router(&metrics, started).oneshot(request));
        has_started.recv().await.expect("the import starts");
        // The client goes away: the request is dropped, its task goes on.
        answering.abort();
        assert!(answering.await.is_err(), "the request was dropped");
        assert_eq!(archives(&metrics, "import"), "1", "still under way");
        tokio::time::sleep(Duration::from_millis(600)).await;
        assert_eq!(archives(&metrics, "import"), "0", "done");
    }

    #[test]
    fn the_archive_routes_are_known_with_and_without_a_base_path() {
        assert_eq!(
            ArchiveWork::of_route("/api/v1/boards/{boardId}/archive/export"),
            Some(ArchiveWork::Export)
        );
        assert_eq!(
            ArchiveWork::of_route("/boards/api/v1/teams/{teamId}/archive/import"),
            Some(ArchiveWork::Import)
        );
        for other in [
            "/api/v1/teams/{teamId}/archive/uploads",
            "/teams/{teamId}/archive/export",
            "/api/v2/teams/{teamId}/archive/export",
        ] {
            assert_eq!(ArchiveWork::of_route(other), None, "{other}");
        }
    }
}
