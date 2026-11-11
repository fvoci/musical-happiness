//! `cargo xtask check`, the step "lengths": how long a Rust file and a function may be.
//!
//! The numbers are the constants of this file. What is over them today is listed in
//! [`EXCEPTIONS`] with its length as a ceiling: it may not grow, and when it shrinks
//! the step fails until the ceiling is lowered, or the line removed once the length is
//! within the limit. `cargo xtask measure` rewrites the list from the code as it is,
//! and is the one way a ceiling is to go up.
//!
//! The step holds the code to the list; it cannot tell a number that was raised by
//! hand from one that was measured. By hand a line is lowered, removed, or renamed
//! with its code. A raised or added line shows in the diff of the list, and whoever
//! merges looks for it there.

mod measure;

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context as _, Result, bail};

use crate::files;
use crate::steps;

/// The most lines a file may have.
pub(crate) const MAX_FILE_LINES: usize = 1000;

/// The most lines a function may have, from the line of its `fn` to the line of its
/// closing brace, comments and empty lines included.
pub(crate) const MAX_FUNCTION_LINES: usize = 100;

/// The most lines a file of tests may have (see [`is_test_file`]). Tests get twice the
/// limits of the code: a test file is a list of cases that are read one at a time, and
/// a test is often one long scene.
pub(crate) const MAX_TEST_FILE_LINES: usize = 2 * MAX_FILE_LINES;

/// The most lines a function of tests may have: one in a file of tests, or in a module
/// named `tests`. The step refuses a module of that name that is not `#[cfg(test)]`,
/// and a function, a type or a trait of that name, so that the name says it.
pub(crate) const MAX_TEST_FUNCTION_LINES: usize = 2 * MAX_FUNCTION_LINES;

/// The name of the module that holds tests.
pub(crate) const TEST_MODULE: &str = "tests";

/// The directories whose `.rs` files are measured.
pub(crate) const DIRECTORIES: &[&str] = &["crates", "xtask"];

/// The file of what is over a limit, with its ceilings.
pub(crate) const EXCEPTIONS: &str = "xtask/exceptions/lengths.txt";

/// What a length is the length of.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Subject {
    /// A file, by its path from the workspace root.
    File(String),
    /// A function.
    Function {
        /// Its file, by the path from the workspace root.
        file: String,
        /// Its name with the modules, the type or the trait and the functions around
        /// it: `tests::Server::start`, `<AppError as From<StoreError>>::from`. Two
        /// functions of a file may still have one name (two `impl` blocks of a type,
        /// two `cfg` variants); those are told apart by their lengths alone.
        name: String,
    },
}

/// Whether a file is one of tests: a file under the directory `tests` of a crate, next
/// to its `src`, or the file `tests.rs` of a test module.
pub(crate) fn is_test_file(file: &str) -> bool {
    let mut parts = file.split('/');
    let beside_src = parts
        .by_ref()
        .take_while(|part| *part != "src")
        .any(|part| part == TEST_MODULE);
    beside_src || file.rsplit('/').next() == Some("tests.rs")
}

impl Subject {
    /// The file the subject is, or is in.
    fn file(&self) -> &str {
        match self {
            Self::File(file) | Self::Function { file, .. } => file,
        }
    }

    /// The most lines it may have without being listed.
    fn limit(&self) -> usize {
        match self {
            Self::File(file) if is_test_file(file) => MAX_TEST_FILE_LINES,
            Self::File(_) => MAX_FILE_LINES,
            Self::Function { file, name }
                if is_test_file(file) || name.split("::").any(|part| part == TEST_MODULE) =>
            {
                MAX_TEST_FUNCTION_LINES
            }
            Self::Function { .. } => MAX_FUNCTION_LINES,
        }
    }
}

impl std::fmt::Display for Subject {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::File(file) => write!(formatter, "{file}"),
            Self::Function { file, name } => write!(formatter, "{file}: fn {name}"),
        }
    }
}

/// A length that was measured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Length {
    pub(crate) subject: Subject,
    pub(crate) lines: usize,
    /// The line the subject starts on, from 1.
    pub(crate) at: usize,
}

/// What the measuring of the sources gave.
#[derive(Debug, Default)]
pub(crate) struct Measured {
    pub(crate) lengths: Vec<Length>,
    /// What is refused whatever its length, because it would make the measure say
    /// less than there is: each a line for the reader.
    pub(crate) refused: Vec<String>,
}

/// The lengths over their limit, longest first, for each subject.
type Over = BTreeMap<Subject, Vec<usize>>;

/// The step: measures the files and compares what is over a limit with the list.
///
/// # Errors
///
/// Fails if a file cannot be read, or if what is over a limit is not exactly what the
/// list has, with the same lengths.
pub(crate) fn run(root: &Path) -> Result<()> {
    let measured = measure_all(root)?;
    let path = root.join(EXCEPTIONS);
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let listed = parse_exceptions(&text).with_context(|| format!("reading {EXCEPTIONS}"))?;
    let mut problems = measured.refused;
    problems.extend(judge(&measured.lengths, &listed));
    if problems.is_empty() {
        steps::say(&format!(
            "    every file is within {MAX_FILE_LINES} lines and every function within \
             {MAX_FUNCTION_LINES} (tests: {MAX_TEST_FILE_LINES} and {MAX_TEST_FUNCTION_LINES}), or at \
             its listed ceiling ({} listed)",
            listed.values().map(Vec::len).sum::<usize>()
        ));
        return Ok(());
    }
    for problem in &problems {
        steps::say(&format!("    {problem}"));
    }
    steps::say(
        "    by hand a ceiling goes down and never up: shorten the code, or split it. \
         `cargo xtask measure` is for the one who merges, once",
    );
    bail!(
        "{} differences between the code and {EXCEPTIONS}",
        problems.len()
    )
}

/// `cargo xtask measure`: writes the list anew from the code as it is. This is the one
/// way a ceiling is to go up; it is for the moment after a merge of work that was
/// measured apart, and no check calls it.
///
/// # Errors
///
/// Fails if a file cannot be read, if the sources hold something the step refuses
/// whatever its length, or if the list cannot be written.
pub(crate) fn rewrite(root: &Path) -> Result<()> {
    let measured = measure_all(root)?;
    if !measured.refused.is_empty() {
        for problem in &measured.refused {
            steps::say(&format!("    {problem}"));
        }
        bail!("the list is not written: no list makes the step pass with these");
    }
    let over = over_limit(&measured.lengths);
    let path = root.join(EXCEPTIONS);
    std::fs::write(&path, render_exceptions(&over)?)
        .with_context(|| format!("writing {}", path.display()))?;
    steps::say(&format!(
        "wrote {EXCEPTIONS}: {} files and {} functions over their limit",
        over.keys()
            .filter(|subject| matches!(subject, Subject::File(_)))
            .count(),
        over.iter()
            .filter(|(subject, _)| matches!(subject, Subject::Function { .. }))
            .map(|(_, lengths)| lengths.len())
            .sum::<usize>(),
    ));
    Ok(())
}

/// The length of every `.rs` file under [`DIRECTORIES`] and of every function in them.
fn measure_all(root: &Path) -> Result<Measured> {
    let mut measured = Measured::default();
    for directory in DIRECTORIES {
        for file in files::rust_files(&root.join(directory))? {
            let source = std::fs::read_to_string(&file)
                .with_context(|| format!("reading {}", file.display()))?;
            let shown = files::shown_path(root, &file);
            let source = measure::measure_source(&shown, &source)
                .with_context(|| format!("reading {shown}"))?;
            measured.lengths.extend(source.lengths);
            measured.refused.extend(source.refused);
        }
    }
    Ok(measured)
}

/// What is over its limit, of what was measured.
fn over_limit(measured: &[Length]) -> Over {
    let mut over = Over::new();
    for length in measured {
        if length.lines > length.subject.limit() {
            over.entry(length.subject.clone())
                .or_default()
                .push(length.lines);
        }
    }
    for lengths in over.values_mut() {
        lengths.sort_unstable_by_key(|lines| std::cmp::Reverse(*lines));
    }
    over
}

/// The head of the list, which says what it is to whoever opens it.
const EXCEPTIONS_HEAD: &str = "\
# Rust files and functions that are over the limit of their length (xtask/src/lengths.rs),
# each with the number of lines it has: its ceiling.
#
# Written by `cargo xtask measure`. `cargo xtask check` fails when the code is longer
# than its ceiling, when something not listed is over the limit, and when the code got
# shorter, until the number here is lowered (or the line removed, once the code is
# within the limit).
#
# By hand a line is lowered, removed, or renamed with its code; never raised or added.
# The check cannot tell a raised number from a measured one: the diff of this file
# shows it. `cargo xtask measure` is run once after work that was measured apart is
# merged, not to make a check pass.
#
# file <lines> <path>
# function <lines> <path> <name>
";

/// The list as `cargo xtask measure` writes it.
///
/// # Errors
///
/// Fails for a path with a space in it, which the list could not be read back with.
fn render_exceptions(over: &Over) -> Result<String> {
    let mut text = format!("{EXCEPTIONS_HEAD}\n");
    for (subject, lengths) in over {
        if subject.file().contains(char::is_whitespace) {
            bail!(
                "{} has a space in its path, which the list cannot hold",
                subject.file()
            );
        }
        for lines in lengths {
            text.push_str(&match subject {
                Subject::File(file) => ["file", &lines.to_string(), file].join(" "),
                Subject::Function { file, name } => {
                    ["function", &lines.to_string(), file, name].join(" ")
                }
            });
            text.push('\n');
        }
    }
    Ok(text)
}

/// Reads the list. Empty lines and lines that start with `#` are skipped.
fn parse_exceptions(text: &str) -> Result<Over> {
    let mut listed = Over::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        let (lines, subject) = match fields[..] {
            ["file", lines, file] => (lines, Subject::File(file.to_owned())),
            ["function", lines, file, name] => (
                lines,
                Subject::Function {
                    file: file.to_owned(),
                    name: name.to_owned(),
                },
            ),
            _ => bail!(
                "line {}: expected `file lines path` or `function lines path name`, found `{line}`",
                index + 1
            ),
        };
        let lines: usize = lines
            .parse()
            .with_context(|| format!("line {}: `{lines}` is no number of lines", index + 1))?;
        if lines <= subject.limit() {
            bail!(
                "line {}: {lines} lines are within the limit of {}; the line is not needed",
                index + 1,
                subject.limit()
            );
        }
        listed.entry(subject).or_default().push(lines);
    }
    for lengths in listed.values_mut() {
        lengths.sort_unstable_by_key(|lines| std::cmp::Reverse(*lines));
    }
    Ok(listed)
}

/// Compares what was measured with the list. Each line of the answer is one
/// difference: something over the limit that is not listed, something longer than its
/// ceiling, or a ceiling that is higher than the code needs.
fn judge(measured: &[Length], listed: &Over) -> Vec<String> {
    let mut found: BTreeMap<&Subject, Vec<&Length>> = BTreeMap::new();
    for length in measured {
        if length.lines > length.subject.limit() || listed.contains_key(&length.subject) {
            found.entry(&length.subject).or_default().push(length);
        }
    }
    let mut problems = Vec::new();
    for (subject, lengths) in &mut found {
        lengths.sort_by_key(|length| std::cmp::Reverse(length.lines));
        let limit = subject.limit();
        let ceilings = listed.get(*subject).map_or(&[][..], Vec::as_slice);
        // The longest is held to the highest ceiling, and so on down; what is left
        // over on either side is new, or gone.
        for (index, length) in lengths.iter().enumerate() {
            let place = format!("{}:{}", subject.file(), length.at);
            let lines = length.lines;
            match ceilings.get(index) {
                Some(ceiling) if lines > *ceiling => problems.push(format!(
                    "{place}: {subject} has {lines} lines, its ceiling is {ceiling}"
                )),
                Some(ceiling) if lines < *ceiling && lines > limit => problems.push(format!(
                    "{EXCEPTIONS}: {subject} has {lines} lines now: lower its ceiling of {ceiling}"
                )),
                Some(ceiling) if lines < *ceiling => problems.push(format!(
                    "{EXCEPTIONS}: {subject} has {lines} lines now, within the limit of {limit}: \
                     remove its line ({ceiling})"
                )),
                None if lines > limit => problems.push(format!(
                    "{place}: {subject} has {lines} lines, the limit is {limit}"
                )),
                Some(_) | None => {}
            }
        }
    }
    for (subject, ceilings) in listed {
        let measured = found.get(subject).map_or(0, Vec::len);
        for ceiling in ceilings.iter().skip(measured) {
            problems.push(format!(
                "{EXCEPTIONS}: {subject} is not in the code: remove its line ({ceiling})"
            ));
        }
    }
    problems
}

#[cfg(test)]
mod tests;
