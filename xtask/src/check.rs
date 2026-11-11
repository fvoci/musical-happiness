//! `cargo xtask check`: everything that needs no external service.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context as _, Result, bail};

use crate::steps::{self, Step};

/// Whether the dependency check runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Licenses {
    Check,
    Skip,
}

/// The two names of the original product, which the port's own code never uses. Written
/// in two halves so that this file passes its own check.
const FORMER_NAMES: [&str; 2] = [concat!("focal", "board"), concat!("matter", "most")];

/// The directories whose files must not contain the former names.
const NAME_CHECKED_DIRECTORIES: [&str; 2] = ["crates", "xtask"];

/// Where `cargo install --root target/tools` puts the tools this workspace uses.
const LOCAL_TOOLS: &str = "target/tools/bin";

/// Runs the checks. Every check runs, also after one failed.
///
/// # Errors
///
/// Fails if a check failed.
pub(crate) fn run(root: &Path, licenses: Licenses) -> Result<()> {
    let cargo = steps::cargo();
    let mut checks = vec![
        Step::new("format", || {
            steps::run(root, &cargo, &["fmt", "--all", "--check"], &[])
        }),
        Step::new("clippy", || {
            steps::run(
                root,
                &cargo,
                &[
                    "clippy",
                    "--workspace",
                    "--all-targets",
                    "--all-features",
                    "--locked",
                    "--",
                    "-D",
                    "warnings",
                ],
                &[],
            )
        }),
        Step::new("unit tests", || {
            steps::run(
                root,
                &cargo,
                &["test", "--workspace", "--lib", "--bins", "--locked"],
                &[],
            )
        }),
        Step::new("documentation", || {
            steps::run(
                root,
                &cargo,
                &["doc", "--workspace", "--no-deps", "--locked"],
                &[("RUSTDOCFLAGS", "-D warnings")],
            )
        }),
        Step::new("names", || former_names_are_absent(root)),
        Step::new("layers", || crate::layers::run(root)),
        Step::new("lengths", || crate::lengths::run(root)),
        Step::new("contract", || {
            crate::openapi::run(root, crate::openapi::Mode::Check)
        }),
    ];
    checks.push(match licenses {
        Licenses::Check => Step::new("licenses and advisories", || {
            let cargo_deny = tool(root, "cargo-deny");
            // The hint is for a tool that is not there; a licence or an advisory that
            // fails the check is reported as what it is.
            if Command::new(&cargo_deny).arg("--version").output().is_err() {
                bail!(
                    "cargo-deny is needed; install it with \
                     `cargo install cargo-deny --locked --root target/tools`"
                );
            }
            steps::run(root, cargo_deny, &["check"], &[])
        }),
        Licenses::Skip => Step::skipped("licenses and advisories", "--skip-licenses"),
    });
    steps::run_all("check", checks)
}

/// The path of a tool: the copy under `target/tools` if there is one, else the name, for
/// the search path to resolve.
fn tool(root: &Path, name: &str) -> PathBuf {
    let local = root.join(LOCAL_TOOLS).join(name);
    if local.is_file() {
        local
    } else {
        PathBuf::from(name)
    }
}

fn former_names_are_absent(root: &Path) -> Result<()> {
    let mut hits = Vec::new();
    for directory in NAME_CHECKED_DIRECTORIES {
        collect_former_names(&root.join(directory), root, &mut hits)?;
    }
    if hits.is_empty() {
        steps::say(&format!(
            "    no former name in the code of {}",
            NAME_CHECKED_DIRECTORIES.join(", ")
        ));
        return Ok(());
    }
    for hit in &hits {
        steps::say(&format!("    {hit}"));
    }
    bail!("{} lines use a former name", hits.len())
}

/// Appends `path:line` for every line under `directory` that contains a former name, in
/// any case. Files that are not text are skipped.
fn collect_former_names(directory: &Path, root: &Path, hits: &mut Vec<String>) -> Result<()> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(directory)
        .with_context(|| format!("reading {}", directory.display()))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<_, _>>()
        .with_context(|| format!("reading {}", directory.display()))?;
    entries.sort();
    for path in entries {
        if path.is_dir() {
            collect_former_names(&path, root, hits)?;
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (index, line) in text.lines().enumerate() {
            if contains_former_name(line) {
                let shown = path.strip_prefix(root).unwrap_or(&path);
                hits.push(format!("{}:{}", shown.display(), index + 1));
            }
        }
    }
    Ok(())
}

fn contains_former_name(line: &str) -> bool {
    let line = line.to_lowercase();
    FORMER_NAMES.iter().any(|name| line.contains(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A directory for one test's files, under the workspace's `target/`.
    fn scratch(name: &str) -> PathBuf {
        crate::workspace_root()
            .join("target")
            .join("tmp")
            .join("xtask")
            .join(name)
    }

    #[test]
    fn former_names_are_found_in_any_case() {
        let upper = FORMER_NAMES[0].to_uppercase();
        assert!(
            contains_former_name(&format!("see the {upper} docs")),
            "an upper-case name counts"
        );
        assert!(
            contains_former_name(&format!("x{}y", FORMER_NAMES[1])),
            "a name inside a word counts"
        );
        assert!(
            !contains_former_name("the fvoci board server"),
            "the port's own name does not count"
        );
    }

    #[test]
    fn a_file_with_a_former_name_is_reported_by_path_and_line() {
        let root = scratch("names-check");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("crates/nested")).expect("the directory is created");
        std::fs::write(
            root.join("crates/nested/a.rs"),
            format!("fn main() {{}}\n// from {}\n", FORMER_NAMES[0]),
        )
        .expect("the file is written");
        std::fs::write(root.join("crates/clean.rs"), "fn main() {}\n")
            .expect("the file is written");

        let mut hits = Vec::new();
        collect_former_names(&root.join("crates"), &root, &mut hits).expect("the scan succeeds");
        assert_eq!(
            hits,
            ["crates/nested/a.rs:2"],
            "only the offending line is reported"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_tool_under_target_tools_is_preferred() {
        let root = scratch("tool-lookup");
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(
            tool(&root, "cargo-deny"),
            PathBuf::from("cargo-deny"),
            "without a local copy the search path decides"
        );
        std::fs::create_dir_all(root.join(LOCAL_TOOLS)).expect("the directory is created");
        std::fs::write(root.join(LOCAL_TOOLS).join("cargo-deny"), "").expect("the file is written");
        assert_eq!(
            tool(&root, "cargo-deny"),
            root.join(LOCAL_TOOLS).join("cargo-deny"),
            "a local copy wins"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
