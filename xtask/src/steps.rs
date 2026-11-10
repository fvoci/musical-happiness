//! Named steps that all run, with a summary at the end, and the commands they start.

use std::ffi::OsString;
use std::io::Write as _;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail};

/// One named part of a task.
pub(crate) struct Step<'a> {
    name: String,
    action: Box<dyn FnOnce() -> Result<()> + 'a>,
}

impl<'a> Step<'a> {
    pub(crate) fn new(name: impl Into<String>, action: impl FnOnce() -> Result<()> + 'a) -> Self {
        Self {
            name: name.into(),
            action: Box::new(action),
        }
    }
}

impl std::fmt::Debug for Step<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Step")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

/// How a step ended.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Outcome {
    Passed,
    Failed(String),
}

/// Writes a line to standard error, where the output of the started commands goes too.
pub(crate) fn say(line: &str) {
    // A closed standard error leaves nobody to tell.
    let _ = writeln!(std::io::stderr().lock(), "{line}");
}

/// Runs every step, also after one failed, and prints a summary.
///
/// # Errors
///
/// Fails if any step failed; the error names them.
pub(crate) fn run_all(task: &str, steps: Vec<Step<'_>>) -> Result<()> {
    run_all_reporting(task, steps, &mut say)
}

/// [`run_all`] with the progress lines going to `report`.
fn run_all_reporting(task: &str, steps: Vec<Step<'_>>, report: &mut dyn FnMut(&str)) -> Result<()> {
    let mut results: Vec<(String, Outcome, Duration)> = Vec::with_capacity(steps.len());
    for step in steps {
        report(&format!("\n==> {}", step.name));
        let started = Instant::now();
        let outcome = match (step.action)() {
            Ok(()) => Outcome::Passed,
            Err(error) => {
                report(&format!("    failed: {error:#}"));
                Outcome::Failed(format!("{error:#}"))
            }
        };
        results.push((step.name, outcome, started.elapsed()));
    }

    report(&format!("\n{}", summary(task, &results)));
    let failed: Vec<&str> = results
        .iter()
        .filter(|(_, outcome, _)| *outcome != Outcome::Passed)
        .map(|(name, _, _)| name.as_str())
        .collect();
    if failed.is_empty() {
        Ok(())
    } else {
        bail!("{task}: {} failed: {}", failed.len(), failed.join(", "))
    }
}

fn summary(task: &str, results: &[(String, Outcome, Duration)]) -> String {
    let mut lines = vec![format!("{task} summary")];
    for (name, outcome, elapsed) in results {
        let verdict = match outcome {
            Outcome::Passed => "PASS",
            Outcome::Failed(_) => "FAIL",
        };
        lines.push(format!(
            "  {verdict}  {name}  ({:.1}s)",
            elapsed.as_secs_f64()
        ));
    }
    lines.join("\n")
}

/// The `cargo` that runs this task.
pub(crate) fn cargo() -> OsString {
    std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into())
}

/// Runs a command in `directory` with extra environment variables, its output going to
/// the terminal.
///
/// # Errors
///
/// Fails if the program cannot be started or exits with a failure.
pub(crate) fn run(
    directory: &Path,
    program: impl AsRef<std::ffi::OsStr>,
    arguments: &[&str],
    environment: &[(&str, &str)],
) -> Result<()> {
    let program = program.as_ref();
    let shown = format!("{} {}", program.to_string_lossy(), arguments.join(" "));
    say(&format!("    $ {shown}"));
    let status = Command::new(program)
        .args(arguments)
        .envs(environment.iter().copied())
        .current_dir(directory)
        .status()
        .with_context(|| format!("starting `{shown}`"))?;
    if !status.success() {
        bail!("`{shown}` ended with {status}");
    }
    Ok(())
}

/// Runs a command and returns what it wrote to standard output, or `None` if it exited
/// with a failure.
///
/// # Errors
///
/// Fails if the program cannot be started.
pub(crate) fn capture(program: &str, arguments: &[&str]) -> Result<Option<String>> {
    let output = Command::new(program)
        .args(arguments)
        .output()
        .with_context(|| format!("starting `{program} {}`", arguments.join(" ")))?;
    Ok(output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned()))
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[test]
    fn every_step_runs_even_after_a_failure() {
        let ran = Cell::new(0);
        let steps = vec![
            Step::new("first", || {
                ran.set(ran.get() + 1);
                bail!("broken")
            }),
            Step::new("second", || {
                ran.set(ran.get() + 1);
                Ok(())
            }),
        ];
        let mut lines = Vec::new();
        let error = run_all_reporting("task", steps, &mut |line| lines.push(line.to_owned()))
            .expect_err("a failed step fails the task");
        assert_eq!(ran.get(), 2, "both steps ran");
        assert_eq!(
            error.to_string(),
            "task: 1 failed: first",
            "the error names the failed step"
        );
        assert_eq!(
            lines[..3],
            ["\n==> first", "    failed: broken", "\n==> second"],
            "each step is announced, and a failure with its reason"
        );
    }

    #[test]
    fn the_summary_lists_every_step_with_its_verdict() {
        let results = [
            (
                "fmt".to_owned(),
                Outcome::Passed,
                Duration::from_millis(1500),
            ),
            (
                "clippy".to_owned(),
                Outcome::Failed("x".to_owned()),
                Duration::from_secs(2),
            ),
        ];
        assert_eq!(
            summary("check", &results),
            "check summary\n  PASS  fmt  (1.5s)\n  FAIL  clippy  (2.0s)",
            "one line per step"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_failing_command_is_an_error_that_names_it() {
        let error = run(Path::new("."), "false", &["an-argument"], &[])
            .expect_err("`false` exits with a failure");
        assert!(
            error.to_string().contains("`false an-argument` ended with"),
            "the error names the command: {error}"
        );
    }

    #[test]
    fn a_program_that_does_not_exist_is_an_error_that_names_it() {
        let error = run(Path::new("."), "no-such-program-of-xtask", &[], &[])
            .expect_err("a missing program cannot be started");
        assert!(
            error
                .to_string()
                .contains("starting `no-such-program-of-xtask"),
            "the error names the program: {error}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn captured_output_is_trimmed_and_a_failure_is_none() {
        assert_eq!(
            capture("sh", &["-c", "echo ' true '"]).expect("sh starts"),
            Some("true".to_owned()),
            "the output without surrounding space"
        );
        assert_eq!(
            capture("sh", &["-c", "echo no; exit 3"]).expect("sh starts"),
            None,
            "a failing command has no output to trust"
        );
    }
}
