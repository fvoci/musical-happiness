//! The `fvoci-server` executable.

use std::io::Write as _;
use std::path::PathBuf;

use anyhow::Context as _;
use clap::Parser;
use fvoci_server::config::{Configuration, ENV_PREFIX};
use fvoci_server::{BASE_PATH_ENV, Server, checked_base_path, listen_address};
use tracing_subscriber::EnvFilter;

/// Settings that are distinctive enough to recognize under a wrong prefix.
const DISTINCTIVE_SETTINGS: [&str; 7] = [
    "HTTP_SERVER_BASEPATH",
    "SERVERROOT",
    "DBCONFIG",
    "DBTYPE",
    "WEBPATH",
    "FILESPATH",
    "FILESDRIVER",
];

/// The fvoci board server.
#[derive(Debug, Parser)]
#[command(version, about)]
struct Cli {
    /// Location of the JSON config file.
    #[arg(long, default_value = "./config.json")]
    config: PathBuf,
    /// The port number; overrides the config file.
    #[arg(long)]
    port: Option<u16>,
    /// Database type (sqlite3 or postgres); overrides the config file.
    #[arg(long)]
    dbtype: Option<String>,
    /// Database config; overrides the config file.
    #[arg(long)]
    dbconfig: Option<String>,
    /// Print the contract of the API, an OpenAPI document, and exit. Nothing is
    /// configured, opened or served.
    #[arg(long)]
    print_openapi: bool,
    /// Ask the server this configuration describes, on this machine, whether it is
    /// ready (`GET /api/v1/ready`), and exit with success only if it is: the health
    /// check of a container, whose image has no HTTP client. Nothing is opened or served.
    #[arg(long)]
    check_ready: bool,
    /// Bring the database's schema up to date and exit, holding the lock of the server
    /// meanwhile: for the role that owns the tables (the configuration's dbconfig),
    /// when the server connects as one that does not. Nothing is served.
    #[arg(long, conflicts_with = "check_ready")]
    migrate: bool,
}

/// How long the check of readiness waits for the answer: the server answers `ready`
/// within two and a half seconds.
const READY_CHECK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

/// Where the server this configuration describes answers whether it is ready, on this
/// machine: the port, plain HTTP or TLS, and the base path (as the server checks it).
fn ready_url(config: &Configuration, base_path: &str) -> String {
    let scheme = if config.use_ssl { "https" } else { "http" };
    format!(
        "{scheme}://localhost:{}{base_path}{}/ready",
        config.port,
        fvoci_server::api::PREFIX
    )
}

/// Asks the server on this machine whether it is ready, and succeeds only on 200.
/// Nothing is sent but the request. The certificate of a server that serves TLS itself
/// is for its public name, not `localhost`, so it is not verified here.
async fn check_ready(config: &Configuration, base_path: &str) -> anyhow::Result<()> {
    let url = ready_url(config, base_path);
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .tls_danger_accept_invalid_certs(config.use_ssl)
        .timeout(READY_CHECK_TIMEOUT)
        .build()
        .context("unable to set up the check")?;
    let status = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("asking {url}"))?
        .status();
    anyhow::ensure!(status == reqwest::StatusCode::OK, "{url} answered {status}");
    Ok(())
}

/// Writes the contract of the API to the standard output: what `contracts/openapi.json`
/// is to be.
fn print_openapi() -> anyhow::Result<()> {
    let document = fvoci_server::api::contract::document()?;
    std::io::stdout()
        .write_all(document.as_bytes())
        .context("unable to write the contract")
}

fn init_logging() {
    let filter =
        EnvFilter::try_from_env("FVOCI_LOG_LEVEL").unwrap_or_else(|_| EnvFilter::new("info"));
    let builder = tracing_subscriber::fmt().with_env_filter(filter);
    if std::env::var("FVOCI_LOG_FORMAT").is_ok_and(|format| format == "json") {
        builder.json().init();
    } else {
        builder.init();
    }
}

/// Warns about environment variables that carry one of this server's settings under
/// another prefix. They have no effect, which is hard to notice otherwise.
fn warn_about_foreign_prefixes() {
    for (name, _) in std::env::vars_os() {
        let Some(name) = name.to_str() else { continue };
        if name.starts_with(ENV_PREFIX) {
            continue;
        }
        for setting in DISTINCTIVE_SETTINGS {
            let has_other_prefix = name
                .strip_suffix(setting)
                .is_some_and(|prefix| prefix.len() > 1 && prefix.ends_with('_'));
            if has_other_prefix {
                tracing::warn!(
                    "the environment variable {name} is ignored; this server reads {ENV_PREFIX}{setting}"
                );
            }
        }
    }
}

fn main() -> anyhow::Result<()> {
    // The worker of editing together is this executable too, started by the server
    // with one argument: it reads no configuration, starts no runtime and logs nothing.
    if let Some(ended) = fvoci_server::collab::run_as_worker() {
        return ended;
    }
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("unable to start the runtime")?
        .block_on(serve())
}

async fn serve() -> anyhow::Result<()> {
    let cli = Cli::parse();
    if cli.print_openapi {
        return print_openapi();
    }
    init_logging();
    warn_about_foreign_prefixes();

    let mut config = Configuration::read(&cli.config).context("unable to read the config file")?;
    if let Some(db_type) = cli.dbtype.filter(|db_type| !db_type.is_empty()) {
        tracing::info!(db_type, "DBType from commandline");
        config.db_type = db_type;
    }
    if let Some(db_config) = cli.dbconfig.filter(|db_config| !db_config.is_empty()) {
        // Not echoed: the connection string may hold a password.
        tracing::info!("DBConfigString overridden from commandline");
        config.db_config = db_config;
    }
    if let Some(port) = cli.port.filter(|port| *port > 0) {
        tracing::info!(port, "Port from commandline");
        config.port = port;
    }

    let base_path = std::env::var(BASE_PATH_ENV).unwrap_or_default();
    if cli.migrate {
        return fvoci_server::app::migrate_database(&config).await;
    }
    if cli.check_ready {
        return check_ready(&config, checked_base_path(&base_path)?).await;
    }

    let address = listen_address(&config);
    let port = config.port;
    let local_only = config.local_only;
    let server = Server::new(config, &base_path).await?;

    let listener = if local_only {
        tokio::net::TcpListener::bind(("localhost", port)).await
    } else {
        // All interfaces, IPv6 and IPv4 where the system maps one onto the other.
        match tokio::net::TcpListener::bind(("::", port)).await {
            Ok(listener) => Ok(listener),
            Err(_) => tokio::net::TcpListener::bind(("0.0.0.0", port)).await,
        }
    }
    .with_context(|| format!("listening on {address}"))?;

    server.serve(listener, shutdown_signal()).await
}

/// Completes when the process is asked to stop: on an interrupt, as the original, and on
/// a termination request, which is what service managers and containers send. The
/// original dies of the latter without closing anything.
async fn shutdown_signal() {
    let interrupt = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        match signal(SignalKind::terminate()) {
            Ok(mut terminate) => tokio::select! {
                () = interrupt => {}
                _ = terminate.recv() => {}
            },
            Err(error) => {
                tracing::warn!(%error, "unable to listen for the termination signal");
                interrupt.await;
            }
        }
    }
    #[cfg(not(unix))]
    interrupt.await;
    tracing::info!("shutting down");

    // The shutdown waits for requests and notifications under way. A second interrupt
    // ends the process at once.
    tokio::spawn(async {
        if tokio::signal::ctrl_c().await.is_ok() {
            tracing::warn!("interrupted again; exiting without waiting");
            std::process::exit(130);
        }
    });
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    use super::*;

    /// A server on this machine that answers every request with `status`.
    async fn answering(status: &'static str) -> u16 {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("a local port");
        let port = listener.local_addr().expect("its address").port();
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let mut request = [0; 1024];
                let _ = stream.read(&mut request).await;
                let answer =
                    format!("HTTP/1.1 {status}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n");
                let _ = stream.write_all(answer.as_bytes()).await;
            }
        });
        port
    }

    #[tokio::test]
    async fn the_check_succeeds_on_200_only() {
        for (status, ready) in [
            ("200 OK", true),
            ("503 Service Unavailable", false),
            ("204 No Content", false),
        ] {
            let config = Configuration {
                port: answering(status).await,
                ..Configuration::default()
            };
            assert_eq!(check_ready(&config, "").await.is_ok(), ready, "{status}");
        }
    }

    #[test]
    fn the_check_asks_where_the_server_listens() {
        let plain = Configuration {
            port: 8000,
            ..Configuration::default()
        };
        let tls = Configuration {
            port: 8443,
            use_ssl: true,
            ..Configuration::default()
        };
        let base = |raw| checked_base_path(raw).expect("a base path the server takes");
        assert_eq!(
            ready_url(&plain, base("")),
            "http://localhost:8000/api/v1/ready"
        );
        assert_eq!(
            ready_url(&plain, base("/boards/")),
            "http://localhost:8000/boards/api/v1/ready"
        );
        assert_eq!(
            ready_url(&tls, base("/")),
            "https://localhost:8443/api/v1/ready"
        );
    }
}
