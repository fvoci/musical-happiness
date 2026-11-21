//! The end of `cargo xtask test`'s summary: the time of every unit, the tests that
//! failed with the command that runs them again, and the tests that were flaky.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::time::Duration;

use super::harness::{Ran, Report};
use crate::times::{TestName, UnitTime, Verdict};

/// One unit after its turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UnitResult {
    /// The database's name as `--database` takes it.
    pub(crate) database: &'static str,
    pub(crate) unit: String,
    pub(crate) scope: Scope,
    pub(crate) ran: Ran,
}

/// How much of a unit ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scope {
    /// The unit as a run takes it.
    Whole,
    /// The tests that arguments for its harness chose: the names of a rerun.
    Chosen,
}

impl UnitResult {
    pub(crate) fn failed(&self) -> bool {
        !self.ran.succeeded || !self.ran.report.failed_tests.is_empty()
    }

    /// The command that runs this unit's failures again: its failed tests by name if
    /// its harness named every failure, else the whole unit.
    pub(crate) fn rerun(&self) -> String {
        let mut command = format!(
            "cargo xtask test --database {} --unit {}",
            self.database, self.unit
        );
        let report = &self.ran.report;
        if !report.failed_tests.is_empty() && report.names_every_failure() {
            command.push_str(" -- --exact");
            for name in &report.failed_tests {
                command.push(' ');
                command.push_str(&shell_word(name));
            }
        }
        command
    }

    pub(crate) fn time(&self) -> UnitTime {
        let Report {
            passed,
            failed,
            ignored,
            failed_tests,
            ..
        } = &self.ran.report;
        UnitTime {
            database: self.database.to_owned(),
            unit: self.unit.clone(),
            verdict: if self.failed() {
                Verdict::Failed
            } else {
                Verdict::Passed
            },
            seconds: self.ran.elapsed.as_secs_f64(),
            passed: *passed,
            failed: *failed,
            ignored: *ignored,
            failed_tests: failed_tests.clone(),
        }
    }
}

/// The tests that passed in `results` and failed before, in `earlier`, with when the
/// run they failed in started; and the units that failed before without naming a test
/// and passed whole now.
pub(crate) fn flaky(
    results: &[UnitResult],
    earlier: &BTreeMap<TestName, u64>,
) -> Vec<(TestName, u64)> {
    let mut flaky = Vec::new();
    for result in results {
        let whole = TestName {
            database: result.database.to_owned(),
            unit: result.unit.clone(),
            test: String::new(),
        };
        if result.scope == Scope::Whole
            && !result.failed()
            && let Some(&started) = earlier.get(&whole)
        {
            flaky.push((whole, started));
        }
        for test in &result.ran.report.passed_tests {
            let name = TestName {
                database: result.database.to_owned(),
                unit: result.unit.clone(),
                test: test.clone(),
            };
            if let Some(&started) = earlier.get(&name) {
                flaky.push((name, started));
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
        slowest.sort_by_key(|result| std::cmp::Reverse(result.ran.elapsed));
        for result in slowest {
            let report = &result.ran.report;
            let _ = writeln!(
                text,
                "  {:>9}  {}  {:<8}  {}  ({} passed, {} failed, {} ignored)",
                clock(result.ran.elapsed),
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
        text.push_str("\nfailed tests, and the command that runs them again\n");
        for result in failed {
            let report = &result.ran.report;
            let _ = writeln!(text, "  {} {}:", result.database, result.unit);
            for name in &report.failed_tests {
                let _ = writeln!(text, "    {name}");
            }
            if !report.names_every_failure() || report.failed_tests.is_empty() {
                text.push_str(
                    "    (the harness did not name every failure: see its output above; the \
                     command runs the whole unit)\n",
                );
            }
            let _ = writeln!(text, "    {}", result.rerun());
        }
    }
    if !flaky.is_empty() {
        text.push_str(
            "\nflaky: failed in an earlier run of this same source, passed in this one \
             (the earlier run stays failed)\n",
        );
        for (name, started) in flaky {
            let which = if name.test.is_empty() {
                "(the whole unit)"
            } else {
                name.test.as_str()
            };
            let _ = writeln!(
                text,
                "  {} {}: {}  (failed {} ago)",
                name.database,
                name.unit,
                which,
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
            ran: Ran {
                succeeded: matches!(exit, Exit::Success),
                report,
                elapsed: Duration::from_millis(83_400),
            },
        }
    }

    fn failing(names: &[&str], failed: u64) -> Report {
        Report {
            failed,
            failed_tests: names.iter().map(|&name| name.to_owned()).collect(),
            ..Report::default()
        }
    }

    #[test]
    fn the_rerun_names_the_failed_tests_when_every_failure_was_named() {
        let named = result(
            "postgres",
            "rooms",
            Exit::Failure,
            failing(&["rooms::a", "rooms::b"], 2),
        );
        assert_eq!(
            named.rerun(),
            "cargo xtask test --database postgres --unit rooms -- --exact rooms::a rooms::b"
        );
        let doc = result(
            "sqlite",
            "doc",
            Exit::Failure,
            failing(&["src/lib.rs - f (line 3)"], 1),
        );
        assert_eq!(
            doc.rerun(),
            "cargo xtask test --database sqlite --unit doc -- --exact 'src/lib.rs - f (line 3)'",
            "a name with spaces is quoted"
        );
    }

    #[test]
    fn a_unit_whose_failures_are_not_all_named_runs_again_whole() {
        let crashed = result("sqlite", "api", Exit::Failure, Report::default());
        assert!(crashed.failed(), "the exit status fails it");
        assert_eq!(
            crashed.rerun(),
            "cargo xtask test --database sqlite --unit api"
        );
        let short = result("s3", "api", Exit::Failure, failing(&["api::a"], 2));
        assert_eq!(short.rerun(), "cargo xtask test --database s3 --unit api");
        let text = lines(&[short], &[], 0);
        assert!(text.contains("did not name every failure"), "{text}");
    }

    #[test]
    fn a_named_failure_fails_the_unit_whatever_the_exit_status() {
        assert!(result("sqlite", "x", Exit::Success, failing(&["x::a"], 1)).failed());
        assert!(!result("sqlite", "x", Exit::Success, Report::default()).failed());
    }

    #[test]
    fn the_summary_lists_units_slowest_first_then_failures_then_flaky_tests() {
        let mut fast = result("sqlite", "libs", Exit::Success, Report::default());
        fast.ran.elapsed = Duration::from_secs(2);
        let slow = result("sqlite", "api", Exit::Failure, failing(&["api::x"], 1));
        let name = TestName {
            database: "sqlite".to_owned(),
            unit: "web".to_owned(),
            test: "web::y".to_owned(),
        };
        let text = lines(&[fast, slow], &[(name, 1000)], 1600);
        let api = text.find("FAIL  sqlite    api").expect(&text);
        let libs = text.find("PASS  sqlite    libs").expect(&text);
        assert!(api < libs, "slowest first: {text}");
        assert!(text.contains("  1m23.4s  FAIL"), "{text}");
        assert!(
            text.contains("    cargo xtask test --database sqlite --unit api -- --exact api::x"),
            "{text}"
        );
        assert!(
            text.contains("  sqlite web: web::y  (failed 10m00.0s ago)"),
            "{text}"
        );
    }

    #[test]
    fn a_test_that_passes_now_and_failed_before_is_flaky() {
        let passed = Report {
            passed_tests: vec!["api::x".to_owned(), "api::y".to_owned()],
            ..Report::default()
        };
        let results = [result("postgres", "api", Exit::Success, passed)];
        let earlier = BTreeMap::from([
            (
                TestName {
                    database: "postgres".to_owned(),
                    unit: "api".to_owned(),
                    test: "api::y".to_owned(),
                },
                7,
            ),
            (
                TestName {
                    database: "sqlite".to_owned(),
                    unit: "api".to_owned(),
                    test: "api::x".to_owned(),
                },
                8,
            ),
        ]);
        let flaky = flaky(&results, &earlier);
        assert_eq!(
            flaky.len(),
            1,
            "another database's failure is another test: {flaky:?}"
        );
        assert_eq!((flaky[0].0.test.as_str(), flaky[0].1), ("api::y", 7));
    }

    #[test]
    fn a_unit_that_failed_unnamed_is_flaky_only_when_it_passes_whole() {
        let earlier = BTreeMap::from([(
            TestName {
                database: "sqlite".to_owned(),
                unit: "api".to_owned(),
                test: String::new(),
            },
            5,
        )]);
        let whole = result("sqlite", "api", Exit::Success, Report::default());
        let flaky_units = flaky(std::slice::from_ref(&whole), &earlier);
        assert_eq!(flaky_units.len(), 1, "{flaky_units:?}");
        let text = lines(&[], &flaky_units, 65);
        assert!(
            text.contains("sqlite api: (the whole unit)  (failed 1m00.0s ago)"),
            "{text}"
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
