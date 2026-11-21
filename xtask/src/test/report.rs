//! The end of `cargo xtask test`'s summary: the time of every unit, the tests that
//! failed with the command that runs them again, and the tests that were flaky.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::time::Duration;

use super::harness::UnitRun;
use crate::times::{Scope, TestName, UnitTime, Verdict};

/// One unit after its turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UnitResult {
    /// The database's name as `--database` takes it.
    pub(crate) database: &'static str,
    pub(crate) unit: String,
    pub(crate) scope: Scope,
    pub(crate) run: UnitRun,
}

impl UnitResult {
    /// Whether it failed: its command failed, its harness named a failure, a test binary
    /// of it never reported, or the names of a rerun matched no test.
    pub(crate) fn failed(&self) -> bool {
        let report = &self.run.report;
        !self.run.succeeded
            || !report.failed_tests.is_empty()
            || !report.every_binary_finished()
            || self.matched_nothing()
    }

    /// Whether it ran the tests arguments chose and those were none, which proves
    /// nothing: a name that moved or was misspelled.
    fn matched_nothing(&self) -> bool {
        let report = &self.run.report;
        self.scope == Scope::Chosen && report.passed + report.failed == 0
    }

    /// Whether its failed tests, run again by name, are every failure of it.
    fn reruns_by_name(&self) -> bool {
        !self.run.report.failed_tests.is_empty() && self.run.report.names_every_failure()
    }

    pub(crate) fn time(&self) -> UnitTime {
        let report = &self.run.report;
        UnitTime {
            database: self.database.to_owned(),
            unit: self.unit.clone(),
            scope: self.scope,
            verdict: if self.failed() {
                Verdict::Failed
            } else {
                Verdict::Passed
            },
            seconds: self.run.elapsed.as_secs_f64(),
            passed: report.passed,
            failed: report.failed,
            ignored: report.ignored,
            failed_tests: report.failed_tests.clone(),
            every_failure_named: report.names_every_failure() && !self.matched_nothing(),
        }
    }
}

/// The commands that run the failures of `results` again: for each database, one for
/// the tests named (a name runs in every unit given, and matches nothing where it is
/// not), and one for the units whose failures were not all named, which run whole.
pub(crate) fn reruns(results: &[UnitResult]) -> Vec<String> {
    let mut databases: Vec<&str> = Vec::new();
    for result in results.iter().filter(|result| result.failed()) {
        if !databases.contains(&result.database) {
            databases.push(result.database);
        }
    }
    let mut commands = Vec::new();
    for database in databases {
        let failed = results
            .iter()
            .filter(|result| result.database == database && result.failed());
        let (named, whole): (Vec<&UnitResult>, Vec<&UnitResult>) =
            failed.partition(|result| result.reruns_by_name());
        let command = |units: &[&UnitResult]| {
            let mut command = format!("cargo xtask test --database {database}");
            for result in units {
                command.push_str(" --unit ");
                command.push_str(&result.unit);
            }
            command
        };
        if !named.is_empty() {
            let mut line = command(&named);
            line.push_str(" -- --exact");
            let mut given: Vec<&str> = Vec::new();
            for name in named
                .iter()
                .flat_map(|result| &result.run.report.failed_tests)
            {
                if !given.contains(&name.as_str()) {
                    given.push(name);
                    line.push(' ');
                    line.push_str(&shell_word(name));
                }
            }
            commands.push(line);
        }
        if !whole.is_empty() {
            commands.push(command(&whole));
        }
    }
    commands
}

/// The tests that passed in `results` and failed before, in `earlier`, with when the
/// run they failed in started; and the units that failed before without naming every
/// failure and passed whole now.
pub(crate) fn flaky(
    results: &[UnitResult],
    earlier: &BTreeMap<TestName, u64>,
) -> Vec<(TestName, u64)> {
    let mut flaky = Vec::new();
    for result in results {
        let name = |test: Option<&String>| TestName {
            database: result.database.to_owned(),
            unit: result.unit.clone(),
            test: test.cloned(),
        };
        let whole = name(None);
        if result.scope == Scope::Whole
            && !result.failed()
            && let Some(&started) = earlier.get(&whole)
        {
            flaky.push((whole, started));
        }
        for test in &result.run.report.passed_tests {
            let named = name(Some(test));
            if let Some(&started) = earlier.get(&named) {
                flaky.push((named, started));
            }
        }
    }
    flaky
}

/// The lines after the steps' summary.
pub(crate) fn lines(results: &[UnitResult], flaky: &[(TestName, u64)], now_unix: u64) -> String {
    let mut text = String::new();
    if !results.is_empty() {
        text.push_str("\ntime by unit, slowest first\n");
        let mut slowest: Vec<&UnitResult> = results.iter().collect();
        slowest.sort_by_key(|result| std::cmp::Reverse(result.run.elapsed));
        for result in slowest {
            let report = &result.run.report;
            let _ = writeln!(
                text,
                "  {:>9}  {}  {:<8}  {}  ({} passed, {} failed, {} ignored)",
                clock(result.run.elapsed),
                if result.failed() { "FAIL" } else { "PASS" },
                result.database,
                result.unit,
                report.passed,
                report.failed,
                report.ignored
            );
        }
    }
    let failed: Vec<&UnitResult> = results.iter().filter(|result| result.failed()).collect();
    if !failed.is_empty() {
        text.push_str("\nfailed tests\n");
        for result in failed {
            let _ = writeln!(text, "  {} {}:", result.database, result.unit);
            for name in &result.run.report.failed_tests {
                let _ = writeln!(text, "    {name}");
            }
            if result.matched_nothing() {
                text.push_str("    (the names given matched no test: it runs again whole)\n");
            } else if !result.reruns_by_name() {
                text.push_str(
                    "    (a failure has no name, or a test binary never reported: see the \
                     output above; it runs again whole)\n",
                );
            }
        }
        text.push_str("run them again:\n");
        for command in reruns(results) {
            let _ = writeln!(text, "  {command}");
        }
    }
    if !flaky.is_empty() {
        text.push_str(
            "\nflaky: failed in an earlier run of this same source, passed in this one \
             (the earlier run stays failed)\n",
        );
        for (name, started) in flaky {
            let _ = writeln!(
                text,
                "  {} {}: {}  (failed {} ago)",
                name.database,
                name.unit,
                name.test.as_deref().unwrap_or("(the whole unit)"),
                clock(Duration::from_secs(now_unix.saturating_sub(*started)))
            );
        }
    }
    text
}

/// A span as `1h02m`, `3m07.2s` or `4.1s`.
fn clock(span: Duration) -> String {
    let seconds = span.as_secs();
    if seconds >= 3600 {
        format!("{}h{:02}m", seconds / 3600, seconds % 3600 / 60)
    } else if seconds >= 60 {
        format!("{}m{:04.1}s", seconds / 60, span.as_secs_f64() % 60.0)
    } else {
        format!("{:.1}s", span.as_secs_f64())
    }
}

/// `word` as one word of a POSIX shell's command line.
fn shell_word(word: &str) -> Cow<'_, str> {
    let plain = !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-:./=+,@%".contains(c));
    if plain {
        Cow::Borrowed(word)
    } else {
        Cow::Owned(format!("'{}'", word.replace('\'', r"'\''")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::harness::Report;

    /// How the command of a unit exited.
    #[derive(Clone, Copy)]
    enum Exit {
        Success,
        Failure,
    }

    fn result(database: &'static str, unit: &str, exit: Exit, report: Report) -> UnitResult {
        UnitResult {
            database,
            unit: unit.to_owned(),
            scope: Scope::Whole,
            run: UnitRun {
                succeeded: matches!(exit, Exit::Success),
                report,
                elapsed: Duration::from_millis(83_400),
            },
        }
    }

    /// A report of one binary that ran to its end.
    fn finished(passed: &[&str], failed: &[&str]) -> Report {
        let count = |names: &[&str]| u64::try_from(names.len()).expect("a few names");
        Report {
            started: 1,
            finished: 1,
            passed: count(passed),
            failed: count(failed),
            failed_tests: failed.iter().map(|&name| name.to_owned()).collect(),
            passed_tests: passed.iter().map(|&name| name.to_owned()).collect(),
            ..Report::default()
        }
    }

    fn test_name(database: &str, unit: &str, test: Option<&str>) -> TestName {
        TestName {
            database: database.to_owned(),
            unit: unit.to_owned(),
            test: test.map(str::to_owned),
        }
    }

    #[test]
    fn one_command_per_database_runs_every_named_failure_again() {
        let results = [
            result(
                "postgres",
                "rooms",
                Exit::Failure,
                finished(&[], &["rooms::a", "rooms::b"]),
            ),
            result(
                "postgres",
                "web",
                Exit::Failure,
                finished(&["web::c"], &["web::d"]),
            ),
            result("postgres", "api", Exit::Success, finished(&["api::e"], &[])),
            result(
                "sqlite",
                "doc",
                Exit::Failure,
                finished(&[], &["src/lib.rs - f (line 3)"]),
            ),
        ];
        assert_eq!(
            reruns(&results),
            [
                "cargo xtask test --database postgres --unit rooms --unit web -- --exact \
                 rooms::a rooms::b web::d",
                "cargo xtask test --database sqlite --unit doc -- --exact 'src/lib.rs - f (line 3)'",
            ],
            "a name with spaces is quoted; a unit that passed is left out"
        );
    }

    #[test]
    fn a_unit_whose_failures_are_not_all_named_runs_again_whole() {
        let crashed = result("sqlite", "api", Exit::Failure, Report::default());
        let short = result(
            "sqlite",
            "web",
            Exit::Failure,
            Report {
                failed: 2,
                ..finished(&[], &["web::a"])
            },
        );
        let died = result(
            "sqlite",
            "libs",
            Exit::Failure,
            Report {
                started: 2,
                ..finished(&[], &["libs::a"])
            },
        );
        let named = result("sqlite", "doc", Exit::Failure, finished(&[], &["doc::x"]));
        let results = [crashed, short, died, named];
        assert_eq!(
            reruns(&results),
            [
                "cargo xtask test --database sqlite --unit doc -- --exact doc::x",
                "cargo xtask test --database sqlite --unit api --unit web --unit libs",
            ]
        );
        let text = lines(&results, &[], 0);
        assert_eq!(text.matches("it runs again whole").count(), 3, "{text}");
    }

    #[test]
    fn a_unit_fails_on_a_named_failure_a_binary_that_never_reported_or_a_rerun_of_nothing() {
        assert!(result("sqlite", "x", Exit::Success, finished(&[], &["x::a"])).failed());
        let silent = Report {
            started: 1,
            ..Report::default()
        };
        assert!(
            result("sqlite", "x", Exit::Success, silent).failed(),
            "a test ended the process before its binary reported"
        );
        assert!(!result("sqlite", "x", Exit::Success, finished(&["x::a"], &[])).failed());

        let nothing = UnitResult {
            scope: Scope::Chosen,
            ..result("sqlite", "x", Exit::Success, finished(&[], &[]))
        };
        assert!(nothing.failed(), "names that matched no test prove nothing");
        assert!(!nothing.time().every_failure_named);
        assert_eq!(
            reruns(&[nothing]),
            ["cargo xtask test --database sqlite --unit x"]
        );
    }

    #[test]
    fn the_summary_lists_units_slowest_first_then_failures_then_flaky_tests() {
        let mut fast = result("sqlite", "libs", Exit::Success, finished(&[], &[]));
        fast.run.elapsed = Duration::from_secs(2);
        let slow = result("sqlite", "api", Exit::Failure, finished(&[], &["api::x"]));
        let flaky = [
            (test_name("sqlite", "web", Some("web::y")), 1000),
            (test_name("sqlite", "doc", None), 1535),
        ];
        let text = lines(&[fast, slow], &flaky, 1600);
        let api = text.find("FAIL  sqlite    api").expect(&text);
        let libs = text.find("PASS  sqlite    libs").expect(&text);
        assert!(api < libs, "slowest first: {text}");
        assert!(text.contains("  1m23.4s  FAIL"), "{text}");
        assert!(
            text.contains("\n  cargo xtask test --database sqlite --unit api -- --exact api::x\n"),
            "{text}"
        );
        assert!(
            text.contains("  sqlite web: web::y  (failed 10m00.0s ago)"),
            "{text}"
        );
        assert!(
            text.contains("  sqlite doc: (the whole unit)  (failed 1m05.0s ago)"),
            "{text}"
        );
    }

    #[test]
    fn a_test_that_passes_now_and_failed_before_is_flaky() {
        let results = [result(
            "postgres",
            "api",
            Exit::Success,
            finished(&["api::x", "api::y"], &[]),
        )];
        let earlier = BTreeMap::from([
            (test_name("postgres", "api", Some("api::y")), 7),
            (test_name("sqlite", "api", Some("api::x")), 8),
        ]);
        let flaky = flaky(&results, &earlier);
        assert_eq!(
            flaky,
            [(test_name("postgres", "api", Some("api::y")), 7)],
            "another database's failure is another test"
        );
    }

    #[test]
    fn a_unit_that_failed_unnamed_is_flaky_only_when_it_passes_whole() {
        let earlier = BTreeMap::from([(test_name("sqlite", "api", None), 5)]);
        let whole = result("sqlite", "api", Exit::Success, finished(&["api::x"], &[]));
        assert_eq!(
            flaky(std::slice::from_ref(&whole), &earlier),
            [(test_name("sqlite", "api", None), 5)]
        );
        let chosen = UnitResult {
            scope: Scope::Chosen,
            ..whole
        };
        assert_eq!(
            flaky(&[chosen], &earlier),
            [],
            "some of its tests passing says nothing of the whole"
        );
    }
}
