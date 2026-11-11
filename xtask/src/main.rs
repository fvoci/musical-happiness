//! Development tasks of the fvoci workspace, run as `cargo xtask <command>`.
//!
//! * `cargo xtask check` — formatting, lints, unit tests, documentation, names and
//!   licenses. Starts no external service.
//! * `cargo xtask test` — every test on SQLite and on PostgreSQL, and the S3 storage
//!   test. Prepares the containers the tests need.
//! * `cargo xtask openapi` — fails if `contracts/openapi.json` is not the document the
//!   server prints; `--write` rewrites the file.

mod check;
mod containers;
mod openapi;
mod steps;
mod test;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};

/// Development tasks of the fvoci workspace.
#[derive(Debug, Parser)]
#[command(name = "cargo xtask", bin_name = "cargo xtask")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Formatting, lints, unit tests, documentation, names and licenses. Starts no
    /// external service.
    Check {
        /// Leaves out the dependency check (`cargo deny`), which reads the advisory
        /// database from the network.
        #[arg(long)]
        skip_licenses: bool,
    },
    /// Runs every test on SQLite and on PostgreSQL, and the S3 storage test. Starts the
    /// test containers when they are not running and leaves them running.
    Test {
        /// The databases to run the tests on.
        #[arg(long, value_enum, default_value_t = Databases::All)]
        database: Databases,
    },
    /// Fails if `contracts/openapi.json` is not the OpenAPI document the server prints
    /// from its route definitions.
    Openapi {
        /// Rewrites the file instead of failing.
        #[arg(long)]
        write: bool,
    },
}

/// Which databases `cargo xtask test` covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Databases {
    /// SQLite, PostgreSQL and the S3 storage test.
    All,
    /// SQLite only; needs no container.
    Sqlite,
    /// PostgreSQL only.
    Postgres,
}

/// The workspace root: the directory above this crate's manifest.
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the xtask crate sits in the workspace root")
        .to_owned()
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let root = workspace_root();
    if let Err(error) = steps::catch_interrupts() {
        steps::say(&format!("error: {error:#}"));
        return ExitCode::FAILURE;
    }
    let result = match cli.command {
        Command::Check { skip_licenses } => {
            let licenses = if skip_licenses {
                check::Licenses::Skip
            } else {
                check::Licenses::Check
            };
            check::run(&root, licenses)
        }
        Command::Test { database } => test::run(&root, database),
        Command::Openapi { write } => {
            let mode = if write {
                openapi::Mode::Write
            } else {
                openapi::Mode::Check
            };
            openapi::run(&root, mode)
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            steps::say(&format!("error: {error:#}"));
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory as _;

    use super::*;

    #[test]
    fn the_command_line_definition_is_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn the_workspace_root_holds_the_workspace_manifest() {
        let manifest = workspace_root().join("Cargo.toml");
        let text = std::fs::read_to_string(&manifest).expect("the workspace manifest is readable");
        assert!(
            text.contains("[workspace]"),
            "{} is the workspace manifest",
            manifest.display()
        );
    }
}
