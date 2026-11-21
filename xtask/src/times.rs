//! The time file, `target/xtask-times.jsonl`: one line of JSON for every run of
//! `cargo xtask check` and `cargo xtask test`, with how long each step, each build and
//! each unit of tests took, and which tests failed.
//!
//! Lines are only ever added. A test that failed in one run and passed in a later run
//! of the same source stays failed in the first line and is named flaky in the second.
//! `jq -s . target/xtask-times.jsonl` reads the whole file; `cargo xtask test --part`
//! takes such a file for the weights of its parts.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::steps::{self, Outcome, Ran};

/// Where the time file is, under the workspace root.
pub(crate) const TIMES_FILE: &str = "target/xtask-times.jsonl";

/// One run of a task.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Record {
    /// `check` or `test`.
    pub(crate) task: String,
    /// When the run started, in seconds since 1970 (UTC).
    pub(crate) started_unix: u64,
    pub(crate) source: Source,
    /// The arguments the task was given.
    pub(crate) arguments: Vec<String>,
    /// What `FVOCI_TEST_CPUS` said, if it was set.
    pub(crate) test_cpus: Option<usize>,
    pub(crate) seconds: f64,
    pub(crate) verdict: Verdict,
    pub(crate) steps: Vec<StepTime>,
    /// The builds of the test binaries, one for each database the run covered.
    pub(crate) builds: Vec<BuildTime>,
    pub(crate) units: Vec<UnitTime>,
    /// Tests that failed in an earlier run of the same source and passed in this one.
    pub(crate) flaky: Vec<TestName>,
}

/// The code a run ran: the commit, and a digest of what was changed and not committed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Source {
    pub(crate) commit: String,
    /// Empty when nothing was changed.
    pub(crate) changes: String,
}

/// How a step, a unit or a run ended.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Verdict {
    #[default]
    Passed,
    Failed,
    Skipped,
    Interrupted,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct StepTime {
    pub(crate) name: String,
    pub(crate) verdict: Verdict,
    pub(crate) seconds: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct BuildTime {
    pub(crate) database: String,
    pub(crate) verdict: Verdict,
    pub(crate) seconds: f64,
}

/// One unit of tests: one `cargo test` of one database.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct UnitTime {
    pub(crate) database: String,
    pub(crate) unit: String,
    pub(crate) verdict: Verdict,
    pub(crate) seconds: f64,
    pub(crate) passed: u64,
    pub(crate) failed: u64,
    pub(crate) ignored: u64,
    pub(crate) failed_tests: Vec<String>,
}

/// A test of a unit of a database; with no name, the whole unit.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct TestName {
    pub(crate) database: String,
    pub(crate) unit: String,
    pub(crate) test: String,
}

/// When a run started, by the calendar and by the clock that measures it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Clock {
    wall: SystemTime,
    started: Instant,
}

impl Clock {
    pub(crate) fn start() -> Self {
        Self {
            wall: SystemTime::now(),
            started: Instant::now(),
        }
    }
}

impl Record {
    /// The record of a run of `task` in the checkout at `root`: its source, arguments
    /// and steps, and its verdict from the steps'.
    pub(crate) fn new(root: &Path, task: &str, clock: &Clock, ran: &Ran) -> Self {
        let steps: Vec<StepTime> = ran
            .results()
            .iter()
            .map(|step| StepTime {
                name: step.name.clone(),
                verdict: match step.outcome {
                    Outcome::Passed => Verdict::Passed,
                    Outcome::Failed(_) => Verdict::Failed,
                    Outcome::Skipped(_) => Verdict::Skipped,
                },
                seconds: step.elapsed.as_secs_f64(),
            })
            .collect();
        let verdict = if ran.interrupted() {
            Verdict::Interrupted
        } else if steps.iter().any(|step| step.verdict == Verdict::Failed) {
            Verdict::Failed
        } else {
            Verdict::Passed
        };
        Self {
            task: task.to_owned(),
            started_unix: unix(clock.wall),
            source: source(root),
            arguments: std::env::args().skip(1).collect(),
            test_cpus: crate::cpus::test_cpus().ok().flatten().map(usize::from),
            seconds: clock.started.elapsed().as_secs_f64(),
            verdict,
            steps,
            ..Self::default()
        }
    }
}

/// The seconds since 1970 (UTC) of `time`.
fn unix(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// The seconds since 1970 (UTC) of now.
pub(crate) fn now_unix() -> u64 {
    unix(SystemTime::now())
}

/// The path of the time file of the workspace at `root`.
pub(crate) fn path(root: &Path) -> PathBuf {
    root.join(TIMES_FILE)
}

/// Adds the record of a run to the time file, and says so. A record that cannot be
/// written is said and otherwise left: it changes nothing of the run's verdict.
pub(crate) fn append(root: &Path, record: &Record) {
    let file = path(root);
    match append_to(&file, record) {
        Ok(()) => steps::say(&format!("times: {TIMES_FILE}")),
        Err(error) => steps::say(&format!("times not written: {error:#}")),
    }
}

fn append_to(file: &Path, record: &Record) -> Result<()> {
    let mut line = serde_json::to_string(record).context("writing the record as JSON")?;
    line.push('\n');
    if let Some(directory) = file.parent() {
        std::fs::create_dir_all(directory)
            .with_context(|| format!("creating {}", directory.display()))?;
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)
        .and_then(|mut opened| opened.write_all(line.as_bytes()))
        .with_context(|| format!("appending to {}", file.display()))
}

/// The records of a time file, oldest first. A file that is not there has none; a line
/// that is no record (cut short, or of another shape) is passed over.
///
/// # Errors
///
/// Fails if the file is there and cannot be read.
pub(crate) fn read(file: &Path) -> Result<Vec<Record>> {
    let text = match std::fs::read_to_string(file) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error).with_context(|| format!("reading {}", file.display()));
        }
    };
    Ok(text
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect())
}

/// How long each unit of `database` took, from the latest test run that ran it.
pub(crate) fn unit_seconds(records: &[Record], database: &str) -> HashMap<String, f64> {
    let mut seconds = HashMap::new();
    for unit in records
        .iter()
        .filter(|record| record.task == "test")
        .flat_map(|record| &record.units)
        .filter(|unit| unit.database == database && unit.seconds.is_finite())
    {
        seconds.insert(unit.unit.clone(), unit.seconds);
    }
    seconds
}

/// The tests that failed in the test runs of `source`, each with when the latest of
/// those runs started. A unit that failed without naming a test is there as a test
/// with no name.
pub(crate) fn failures_of(records: &[Record], source: &Source) -> BTreeMap<TestName, u64> {
    let mut failures = BTreeMap::new();
    for record in records
        .iter()
        .filter(|record| record.task == "test" && record.source == *source)
    {
        for unit in &record.units {
            // A unit that failed without naming a test failed as a whole: its name is "".
            let whole = [String::new()];
            let tests = match unit.verdict {
                Verdict::Failed if unit.failed_tests.is_empty() => whole.as_slice(),
                _ => unit.failed_tests.as_slice(),
            };
            for test in tests {
                let name = TestName {
                    database: unit.database.clone(),
                    unit: unit.unit.clone(),
                    test: test.clone(),
                };
                failures.insert(name, record.started_unix);
            }
        }
    }
    failures
}

/// The source of the checkout at `root`. Without git, or outside a repository, the
/// commit is empty, and no two runs are known to be of one source.
pub(crate) fn source(root: &Path) -> Source {
    let root = root.to_string_lossy();
    let git = |arguments: &[&str]| {
        let mut all = vec!["-C", root.as_ref()];
        all.extend(arguments);
        steps::capture("git", &all)
            .ok()
            .filter(|captured| captured.succeeded)
            .map(|captured| captured.output)
    };
    let Some(commit) = git(&["rev-parse", "HEAD"]) else {
        return Source::default();
    };
    // What a run of the same commit may differ by: the changes to tracked files and the
    // files git does not track yet (a new test is one).
    let status = git(&["status", "--porcelain=v1", "--untracked-files=all"]).unwrap_or_default();
    let changes = if status.is_empty() {
        String::new()
    } else {
        let diff = git(&["diff", "HEAD", "--binary"]).unwrap_or_default();
        let digest = Sha256::new()
            .chain_update(status.as_bytes())
            .chain_update(diff.as_bytes())
            .finalize();
        digest.iter().fold(String::new(), |mut hexadecimal, byte| {
            let _ = write!(hexadecimal, "{byte:02x}");
            hexadecimal
        })
    };
    Source { commit, changes }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(database: &str, name: &str, seconds: f64, failed_tests: &[&str]) -> UnitTime {
        UnitTime {
            database: database.to_owned(),
            unit: name.to_owned(),
            seconds,
            failed_tests: failed_tests.iter().map(|&test| test.to_owned()).collect(),
            ..UnitTime::default()
        }
    }

    fn run(started_unix: u64, commit: &str, units: Vec<UnitTime>) -> Record {
        Record {
            task: "test".to_owned(),
            started_unix,
            source: Source {
                commit: commit.to_owned(),
                changes: String::new(),
            },
            units,
            ..Record::default()
        }
    }

    #[test]
    fn records_are_added_and_read_back_and_a_broken_line_is_passed_over() {
        let directory = crate::files::testing::scratch("times-append");
        let file = directory.join("times.jsonl");
        assert_eq!(read(&file).expect("no file"), [], "no file, no records");

        let first = run(1, "a", vec![unit("sqlite", "api", 3.5, &["api::x"])]);
        let second = run(2, "a", vec![]);
        append_to(&file, &first).expect("written");
        std::fs::OpenOptions::new()
            .append(true)
            .open(&file)
            .and_then(|mut opened| opened.write_all(b"{\"task\": \"te"))
            .expect("a line cut short");
        std::fs::OpenOptions::new()
            .append(true)
            .open(&file)
            .and_then(|mut opened| opened.write_all(b"\n"))
            .expect("its end");
        append_to(&file, &second).expect("written");
        assert_eq!(
            read(&file).expect("read"),
            [first, second],
            "both records, in order, without the broken line"
        );
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_unit_weighs_what_the_latest_run_of_its_database_took() {
        let records = [
            run(
                1,
                "a",
                vec![
                    unit("sqlite", "api", 10.0, &[]),
                    unit("sqlite", "web", 2.0, &[]),
                ],
            ),
            run(
                2,
                "b",
                vec![
                    unit("sqlite", "api", 12.0, &[]),
                    unit("postgres", "web", 9.0, &[]),
                ],
            ),
            Record {
                task: "check".to_owned(),
                units: vec![unit("sqlite", "web", 99.0, &[])],
                ..Record::default()
            },
        ];
        let seconds = unit_seconds(&records, "sqlite");
        assert_eq!(seconds.get("api"), Some(&12.0), "the latest run counts");
        assert_eq!(
            seconds.get("web"),
            Some(&2.0),
            "another database or task does not"
        );
        assert_eq!(seconds.len(), 2, "{seconds:?}");
    }

    #[test]
    fn failures_count_only_from_runs_of_the_same_source() {
        let crashed = UnitTime {
            verdict: Verdict::Failed,
            ..unit("sqlite", "web", 1.0, &[])
        };
        let records = [
            run(
                1,
                "a",
                vec![unit("sqlite", "api", 1.0, &["api::x"]), crashed],
            ),
            run(2, "b", vec![unit("sqlite", "api", 1.0, &["api::y"])]),
            run(
                3,
                "a",
                vec![
                    unit("postgres", "api", 1.0, &["api::x"]),
                    unit("sqlite", "doc", 1.0, &[]),
                ],
            ),
        ];
        let source = Source {
            commit: "a".to_owned(),
            changes: String::new(),
        };
        let failures: Vec<(String, u64)> = failures_of(&records, &source)
            .into_iter()
            .map(|(name, started)| {
                (
                    format!("{}/{}/{}", name.database, name.unit, name.test),
                    started,
                )
            })
            .collect();
        assert_eq!(
            failures,
            [
                ("postgres/api/api::x".to_owned(), 3),
                ("sqlite/api/api::x".to_owned(), 1),
                ("sqlite/web/".to_owned(), 1)
            ],
            "commit b's failure is of other code; a unit that failed unnamed is there whole, \
             one that passed is not"
        );
    }

    #[test]
    fn the_source_of_this_checkout_names_its_commit() {
        let source = source(&crate::workspace_root());
        assert_eq!(source.commit.len(), 40, "a full commit hash: {source:?}");
    }
}
