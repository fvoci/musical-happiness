//! What the test harness (libtest) of a `cargo test` says: how many tests passed, failed
//! and were ignored, and which by name.
//!
//! The command's exit status decides whether a unit failed; the names only say which
//! tests to run again. A harness that ended without naming a failure (it aborted, or its
//! process was killed) leaves a failed unit with no names, run again as a whole.

use std::io::{BufRead as _, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail};

/// The tests of a unit, as its harness reported them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Report {
    pub(crate) passed: u64,
    pub(crate) failed: u64,
    pub(crate) ignored: u64,
    /// The tests it named as failed, each once, in the order it named them.
    pub(crate) failed_tests: Vec<String>,
    /// The tests it named as passed.
    pub(crate) passed_tests: Vec<String>,
}

impl Report {
    /// Whether it named every test that failed, so that running those names again runs
    /// every failure again.
    pub(crate) fn names_every_failure(&self) -> bool {
        u64::try_from(self.failed_tests.len()).is_ok_and(|named| named >= self.failed)
    }
}

/// Reads the harness's output a line at a time.
#[derive(Debug, Default)]
pub(crate) struct Reading {
    report: Report,
    /// The names under the last `failures:` heading, until something else than a name
    /// comes: only the list just before a `test result:` line is the list of failures,
    /// and not a line of a failed test's output that says `failures:`.
    listing: Option<Vec<String>>,
}

impl Reading {
    pub(crate) fn line(&mut self, line: &str) {
        let line = line.trim_end_matches(['\r', '\n']);
        if let Some(result) = line.strip_prefix("test result: ") {
            self.counts(result);
            for name in self.listing.take().unwrap_or_default() {
                self.failed(name);
            }
        } else if line == "failures:" {
            self.listing = Some(Vec::new());
        } else if let Some(names) = &mut self.listing
            && (line.is_empty() || line.starts_with("    "))
        {
            if !line.trim().is_empty() {
                names.push(line.trim_start().to_owned());
            }
        } else {
            self.listing = None;
            if let Some((name, verdict)) = line
                .strip_prefix("test ")
                .and_then(|rest| rest.rsplit_once(" ... "))
            {
                match verdict {
                    "FAILED" => self.failed(name.to_owned()),
                    "ok" => self.report.passed_tests.push(name.to_owned()),
                    _ => {}
                }
            }
        }
    }

    pub(crate) fn finish(self) -> Report {
        self.report
    }

    fn failed(&mut self, name: String) {
        if !self.report.failed_tests.contains(&name) {
            self.report.failed_tests.push(name);
        }
    }

    /// Adds the counts of a line such as `ok. 3 passed; 1 failed; 2 ignored; 0 measured;
    /// 0 filtered out; finished in 0.10s`.
    fn counts(&mut self, result: &str) {
        let counts = result.split_once(". ").map_or(result, |(_, counts)| counts);
        for part in counts.split("; ") {
            let Some((number, what)) = part.split_once(' ') else {
                continue;
            };
            let Ok(number) = number.parse::<u64>() else {
                continue;
            };
            match what {
                "passed" => self.report.passed += number,
                "failed" => self.report.failed += number,
                "ignored" => self.report.ignored += number,
                _ => {}
            }
        }
    }
}

/// A command of tests that ran to its end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Ran {
    /// Whether it exited with success.
    pub(crate) succeeded: bool,
    pub(crate) report: Report,
    pub(crate) elapsed: Duration,
}

/// Runs a command of tests in `directory`, its standard output read as the harness's
/// and passed on to this task's, its standard error going where this task's goes.
///
/// # Errors
///
/// Fails if the program cannot be started or its output cannot be read.
pub(crate) fn run(
    directory: &Path,
    program: &std::ffi::OsStr,
    arguments: &[String],
    environment: &[(String, String)],
) -> Result<Ran> {
    let shown = format!("{} {}", program.to_string_lossy(), arguments.join(" "));
    if crate::steps::interrupted() {
        bail!("interrupted before `{shown}`");
    }
    crate::steps::say(&format!("    $ {shown}"));
    let mut command = Command::new(program);
    command
        .args(arguments)
        .envs(environment.iter().map(|(name, value)| (name, value)))
        .current_dir(directory);
    run_echoing(command, &shown, &mut std::io::stdout())
}

/// Runs `command`, its standard output read as the harness's and written to `echo`.
fn run_echoing(mut command: Command, shown: &str, echo: &mut dyn Write) -> Result<Ran> {
    let started = Instant::now();
    let mut child = command
        .stdout(Stdio::piped())
        .spawn()
        .with_context(|| format!("starting `{shown}`"))?;
    let mut reading = Reading::default();
    if let Some(output) = child.stdout.take() {
        let mut output = BufReader::new(output);
        let mut line = Vec::new();
        loop {
            line.clear();
            let read = output
                .read_until(b'\n', &mut line)
                .with_context(|| format!("reading the output of `{shown}`"))?;
            if read == 0 {
                break;
            }
            // A closed standard output leaves nobody to show the line to; it is still read.
            let _ = echo.write_all(&line);
            reading.line(&String::from_utf8_lossy(&line));
        }
    }
    let status = child
        .wait()
        .with_context(|| format!("waiting for `{shown}`"))?;
    Ok(Ran {
        succeeded: status.success(),
        report: reading.finish(),
        elapsed: started.elapsed(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(text: &str) -> Report {
        let mut reading = Reading::default();
        text.lines().for_each(|line| reading.line(line));
        reading.finish()
    }

    /// The output of two binaries, one with failures, as libtest writes it: a failed
    /// test's own output, which says `failures:` and names a test, comes first.
    const TWO_BINARIES: &str = "
running 2 tests
test a::passes ... ok
test a::slow ... ignored, takes a minute

test result: ok. 1 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.01s


running 4 tests
test b::fine ... ok
test b::broken ... FAILED
test b::fine_too ... ok
test b::also_broken ... FAILED

failures:

---- b::broken stdout ----
failures:
    b::not_a_failure

thread 'b::broken' panicked at src/lib.rs:3:5:
no

failures:
    b::also_broken
    b::broken

test result: FAILED. 2 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
";

    #[test]
    fn counts_add_up_and_the_failures_are_named_once() {
        let report = read(TWO_BINARIES);
        assert_eq!((report.passed, report.failed, report.ignored), (3, 2, 1));
        assert_eq!(
            report.failed_tests,
            ["b::broken", "b::also_broken"],
            "each once, and not the name in a test's own output"
        );
        assert_eq!(report.passed_tests, ["a::passes", "b::fine", "b::fine_too"]);
        assert!(report.names_every_failure());
    }

    #[test]
    fn a_failure_is_named_by_the_list_when_its_line_was_cut() {
        let report = read(
            "running 1 test\ntest c::one ... some output of a child processFAILED\n\n\
             failures:\n\n---- c::one stdout ----\nx\n\nfailures:\n    c::one\n\n\
             test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; \
             finished in 0.00s\n",
        );
        assert_eq!(report.failed_tests, ["c::one"]);
    }

    #[test]
    fn documentation_tests_are_named_with_their_spaces() {
        let report = read(
            "running 1 test\ntest crates/x/src/lib.rs - f (line 3) ... FAILED\n\n\
             test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; \
             finished in 0.00s\n",
        );
        assert_eq!(report.failed_tests, ["crates/x/src/lib.rs - f (line 3)"]);
    }

    #[test]
    fn a_harness_that_ended_early_has_not_named_every_failure() {
        let report = read("running 3 tests\ntest d::a ... ok\n");
        assert_eq!(report.failed, 0, "no result line, no counts");
        let counted = read("test result: FAILED. 0 passed; 2 failed; 0 ignored;\n");
        assert!(
            !counted.names_every_failure(),
            "two failures counted, none named"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_command_is_run_and_read_and_its_status_kept() {
        // Its output goes to `echo`, not to this test's: the harness of a run of these
        // tests would read it as its own.
        let script = "echo 'test e::x ... FAILED'; \
                      echo 'test result: FAILED. 0 passed; 1 failed; 0 ignored'; exit 101";
        let mut command = Command::new("sh");
        command.args(["-c", script]);
        let mut echo = Vec::new();
        let ran = run_echoing(command, "sh", &mut echo).expect("sh starts");
        assert!(!ran.succeeded, "the exit status is kept");
        assert_eq!(ran.report.failed_tests, ["e::x"]);
        assert!(
            String::from_utf8_lossy(&echo).starts_with("test e::x ... FAILED\n"),
            "every line is passed on"
        );

        let mut command = Command::new("sh");
        command
            .args(["-c", "test \"$THE_VALUE\" = given"])
            .env("THE_VALUE", "given");
        let ran = run_echoing(command, "sh", &mut Vec::new()).expect("sh starts");
        assert!(ran.succeeded, "a command that succeeds");
    }
}
