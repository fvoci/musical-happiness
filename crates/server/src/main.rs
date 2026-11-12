//! The `fvoci-server` executable.

use std::io::Write as _;
use std::path::PathBuf;

use anyhow::Context as _;
use clap::Parser;
use fvoci_server::config::{Configuration, ENV_PREFIX};
use fvoci_server::{BASE_PATH_ENV, Server, listen_address};
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
