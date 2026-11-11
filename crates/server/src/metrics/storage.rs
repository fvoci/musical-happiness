//! The metrics of the database and the files: the connection pool, the open uploads,
//! and the exports and imports of archives that hold their place.

use std::fmt::Display;
use std::time::Duration;

use prometheus::{IntCounter, IntGauge, IntGaugeVec, Opts, Registry};

use super::{Metrics, PREFIX};

/// How long a scrape waits for the database to count the open uploads. Shorter than
/// Prometheus's own timeout of a scrape (10 s by default), so that a pool that is
/// exhausted, which the pool's metrics are there to show, does not cost the scrape.
const UPLOADS_READ_TIMEOUT: Duration = Duration::from_secs(2);

/// The work on archives that is counted while it holds its place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArchiveWork {
    /// An archive is written and sent: its place is held until the download ends or is
    /// given up, or has stalled for `export_stall_seconds` (looked at every quarter of
    /// it), and then until its file is removed (`app/download.rs`).
    Export,
    /// An archive is read and its boards are made.
    Import,
}

impl ArchiveWork {
    /// The value of the `work` label.
    fn as_str(self) -> &'static str {
        match self {
            Self::Export => "export",
            Self::Import => "import",
        }
    }
}

/// The connection pool of the database, as a scrape finds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Pool {
    /// The connections the pool holds, in use or idle.
    pub(crate) connections: u32,
    /// The connections the pool holds that are idle.
    pub(crate) idle: u32,
    /// The most connections the pool may hold.
    pub(crate) max_connections: u32,
}

/// The uploads in parts that are open, as a scrape finds them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OpenUploads {
    /// How many there are.
    pub(crate) sessions: i64,
    /// The sum of their sizes, in bytes.
    pub(crate) bytes: i64,
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
                "How many exports and imports of archives hold their place, as \
                 max_concurrent_exports and max_concurrent_imports count them (an export \
                 until its download ends), by work: export, import.",
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

impl Metrics {
    /// Sets how many exports and how many imports of archives hold their place.
    pub(crate) fn set_archives_running(&self, exports: usize, imports: usize) {
        let Some(kept) = &self.kept else {
            return;
        };
        for (work, running) in [
            (ArchiveWork::Export, exports),
            (ArchiveWork::Import, imports),
        ] {
            kept.storage
                .archives
                .with_label_values(&[work.as_str()])
                .set(i64::try_from(running).unwrap_or(i64::MAX));
        }
    }

    /// Sets the connection pool of the database.
    pub(crate) fn set_pool(&self, pool: Pool) {
        let Some(kept) = &self.kept else {
            return;
        };
        let storage = &kept.storage;
        storage.pool_connections.set(i64::from(pool.connections));
        storage.pool_idle.set(i64::from(pool.idle));
        storage.pool_max.set(i64::from(pool.max_connections));
    }

    /// Sets the open uploads to what `read` answers, for a scrape. `read` is what asks
    /// the database; it is not polled if no metrics are kept or another scrape is
    /// reading. The uploads stay as they were then, and if `read` fails or does not
    /// answer in time, which is counted.
    pub(crate) async fn read_open_uploads<E: Display>(
        &self,
        read: impl Future<Output = Result<OpenUploads, E>>,
    ) {
        let Some(kept) = &self.kept else {
            return;
        };
        let storage = &kept.storage;
        let Ok(_reading) = storage.reading_uploads.try_lock() else {
            return;
        };
        match tokio::time::timeout(UPLOADS_READ_TIMEOUT, read).await {
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
