//! The metrics of the database and the files: the connection pool, the open uploads,
//! and the exports and imports of archives under way.

use std::pin::Pin;
use std::task::{Context, Poll};

use axum::body::{Body, Bytes, HttpBody};
use http_body::{Frame, SizeHint};
use prometheus::{IntGauge, IntGaugeVec, Opts, Registry};

use super::{Metrics, PREFIX};
use crate::app::App;

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
    /// The uploads stay as they were if the database cannot be read.
    pub(super) async fn read_storage(&self, app: &App) {
        let Some(kept) = &self.kept else {
            return;
        };
        let storage = &kept.storage;
        let pool = app.store().pool_status();
        storage.pool_connections.set(i64::from(pool.connections));
        storage.pool_idle.set(i64::from(pool.idle));
        storage.pool_max.set(i64::from(pool.max_connections));
        match app.store().open_uploads(crate::auth::now_millis()).await {
            Ok(open) => {
                storage.uploads_open.set(open.sessions);
                storage.uploads_open_bytes.set(open.bytes);
            }
            Err(error) => tracing::warn!(%error, "a scrape could not read the open uploads"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
