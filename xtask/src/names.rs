//! `cargo xtask check`, the step "names": the name the original gave the workspace.
//!
//! The original gives a workspace the name [`FORMER`]; here it is a workspace
//! everywhere, and that name will be a group of people's. It is refused in any case, in
//! plurals and compounds, in every file git tracks or would track but the web app's,
//! which its own check holds to a list of its own. What stays is listed in
//! [`EXCEPTIONS`], each under the reason it stays; an entry that nothing needs any more
//! fails the step until it is removed.

use std::path::Path;
use std::process::Command;

use anyhow::{Context as _, Result, bail};

use crate::steps;

/// The former name, written in halves so that this file passes its own check.
const FORMER: &str = concat!("te", "am");

/// The file of what may keep the former name, and why.
pub(crate) const EXCEPTIONS: &str = "xtask/exceptions/names.txt";

/// The folder the web app's own check covers.
const WEB_APP: &str = "apps/web/";

/// An entry of [`EXCEPTIONS`]: the files it is for and, unless it lets a whole file
/// keep the name, the text within which a line may.
#[derive(Debug, PartialEq, Eq)]
struct Exception {
    /// Its line in the file of exceptions.
    line: usize,
    /// A path from the repository root; `*` stands for any part of a name, `**` for
    /// any number of folders.
    pattern: String,
    /// The text, lowercase; none for the whole file, its name included.
    text: Option<String>,
}

/// Fails if a file has the former name where no exception lets it, or an exception
/// lets what is not there any more.
///
/// # Errors
///
/// Fails on what it finds, and if git cannot list the files or the exceptions cannot
/// be read.
pub(crate) fn run(root: &Path) -> Result<()> {
    let listed = std::fs::read_to_string(root.join(EXCEPTIONS))
        .with_context(|| format!("reading {EXCEPTIONS}"))?;
    let exceptions = parse(&listed)?;
    let mut used = vec![false; exceptions.len()];
    let mut hits = Vec::new();
    for file in tracked_files(root)? {
        if !is_checked(&file) {
            continue;
        }
        let text = tracked_text(&root.join(&file)).with_context(|| format!("reading {file}"))?;
        hits.extend(check_file(&file, &text, &exceptions, &mut used));
    }
    let stale: Vec<String> = exceptions
        .iter()
        .zip(&used)
        .filter(|(exception, used)| !**used && !exception.pattern.starts_with(WEB_APP))
        .map(|(exception, _)| {
            format!(
                "{EXCEPTIONS}:{}: nothing needs it: remove it",
                exception.line
            )
        })
        .collect();
    if hits.is_empty() && stale.is_empty() {
        steps::say(&format!(
            "    the name {FORMER} only where the {} exceptions of {EXCEPTIONS} let it",
            exceptions.len()
        ));
        return Ok(());
    }
    for line in hits.iter().chain(&stale) {
        steps::say(&format!("    {line}"));
    }
    steps::say(&format!(
        "    a workspace is a workspace; what keeps the original's name is listed in {EXCEPTIONS} with its reason"
    ));
    bail!(
        "{} places have {FORMER}, {} exceptions are stale",
        hits.len(),
        stale.len()
    )
}

/// Whether this check covers the file: not the web app's, which has its own, nor the
/// list of exceptions.
fn is_checked(file: &str) -> bool {
    !file.starts_with(WEB_APP) && file != EXCEPTIONS
}

/// The entries of the file of exceptions. Each stands in a paragraph that begins with
/// a comment, its reason.
fn parse(listed: &str) -> Result<Vec<Exception>> {
    let mut exceptions = Vec::new();
    let mut reason = false;
    for (index, line) in listed.lines().enumerate() {
        let line_number = index + 1;
        let line = line.trim();
        if line.is_empty() {
            reason = false;
            continue;
        }
        if line.starts_with('#') {
            reason = true;
            continue;
        }
        if !reason {
            bail!("{EXCEPTIONS}:{line_number}: an exception needs its reason, a comment above it");
        }
        let (pattern, text) = match line.split_once(" | ") {
            Some((pattern, text)) => (pattern.trim(), Some(text.trim().to_lowercase())),
            None => (line, None),
        };
        if text.as_deref().is_some_and(|text| !text.contains(FORMER)) {
            bail!("{EXCEPTIONS}:{line_number}: the text has no {FORMER} in it");
        }
        exceptions.push(Exception {
            line: line_number,
            pattern: pattern.to_owned(),
            text,
        });
    }
    Ok(exceptions)
}

/// The files git tracks or would track, by their paths from the root.
pub(crate) fn tracked_files(root: &Path) -> Result<Vec<String>> {
    let output = Command::new("git")
        .args(["ls-files", "--cached", "--others", "--exclude-standard"])
        .current_dir(root)
        .output()
        .context("running git ls-files")?;
    if !output.status.success() {
        bail!(
            "git ls-files failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let listed = String::from_utf8(output.stdout).context("git lists the files as text")?;
    let mut files: Vec<String> = listed.lines().map(str::to_owned).collect();
    files.sort();
    Ok(files)
}

/// The hits of one file, `path:line` (`path` for its name), and the exceptions it used.
fn check_file(file: &str, text: &str, exceptions: &[Exception], used: &mut [bool]) -> Vec<String> {
    let mine: Vec<usize> = (0..exceptions.len())
        .filter(|&index| matches(&exceptions[index].pattern, file))
        .collect();
    let whole = mine
        .iter()
        .copied()
        .find(|&index| exceptions[index].text.is_none());
    let name_has_it = file.to_lowercase().contains(FORMER);
    let mut hits = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let mut rest = line.to_lowercase();
        if !rest.contains(FORMER) {
            continue;
        }
        if let Some(whole) = whole {
            used[whole] = true;
            continue;
        }
        for &exception in &mine {
            if let Some(allowed) = &exceptions[exception].text
                && rest.contains(allowed.as_str())
            {
                used[exception] = true;
                rest = rest.replace(allowed.as_str(), "");
            }
        }
        if rest.contains(FORMER) {
            hits.push(format!("{file}:{}", index + 1));
        }
    }
    if name_has_it {
        match whole {
            Some(whole) => used[whole] = true,
            None => hits.insert(0, format!("{file}: in its name")),
        }
    }
    hits
}

/// Whether `path` is one of the paths `pattern` stands for.
fn matches(pattern: &str, path: &str) -> bool {
    let pattern: Vec<&str> = pattern.split('/').collect();
    let path: Vec<&str> = path.split('/').collect();
    matches_parts(&pattern, &path)
}

fn matches_parts(pattern: &[&str], path: &[&str]) -> bool {
    match pattern.split_first() {
        None => path.is_empty(),
        Some((&"**", rest)) => (0..=path.len()).any(|skip| matches_parts(rest, &path[skip..])),
        Some((first, rest)) => path
            .split_first()
            .is_some_and(|(name, others)| matches_name(first, name) && matches_parts(rest, others)),
    }
}

/// Whether a name is one `pattern` stands for, `*` standing for any part of it.
fn matches_name(pattern: &str, name: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == name,
        Some((head, tail)) => name.strip_prefix(head).is_some_and(|after| {
            (0..=after.len())
                .filter(|&cut| after.is_char_boundary(cut))
                .any(|cut| matches_name(tail, &after[cut..]))
        }),
    }
}

/// The text of a file git tracks, as git holds it: a link is the path it points at (a
/// link to a folder, as the skills' under `.claude/skills/`, has no text of its own),
/// a file its bytes read as text whatever their encoding, as the web app's check reads,
/// and a file deleted from the working tree none, though its name is still checked.
pub(crate) fn tracked_text(path: &Path) -> std::io::Result<String> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_symlink() => {
            Ok(std::fs::read_link(path)?.to_string_lossy().into_owned())
        }
        Ok(_) => Ok(String::from_utf8_lossy(&std::fs::read(path)?).into_owned()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::testing;

    fn exceptions(listed: &str) -> Vec<Exception> {
        parse(listed).expect("the exceptions are read")
    }

    #[test]
    fn a_link_is_read_as_the_path_it_points_at_and_a_gone_file_as_nothing() {
        let root = testing::scratch("names-tracked-text");
        testing::write(&root, "skills/a/SKILL.md", "a skill\n");
        std::fs::create_dir_all(root.join("links")).expect("the folder of links is made");
        std::os::unix::fs::symlink("../skills/a", root.join("links/a")).expect("the link is made");

        assert_eq!(
            tracked_text(&root.join("links/a")).expect("a link is read"),
            "../skills/a"
        );
        assert_eq!(
            tracked_text(&root.join("skills/a/SKILL.md")).expect("a file is read"),
            "a skill\n"
        );
        assert_eq!(
            tracked_text(&root.join("gone.md")).expect("a gone file is read"),
            ""
        );
    }

    #[test]
    fn the_web_app_and_the_exceptions_are_left_to_their_own_check() {
        assert!(!is_checked("apps/web/src/a.ts"));
        assert!(!is_checked(EXCEPTIONS));
        assert!(is_checked("apps/another/a.ts"));
        assert!(is_checked("crates/a.rs"));
    }

    #[test]
    fn a_pattern_stands_for_names_and_folders() {
        assert!(matches("crates/a/*.sql", "crates/a/0001_x.sql"));
        assert!(!matches("crates/a/*.sql", "crates/a/b/0001_x.sql"));
        assert!(matches("crates/**/x.rs", "crates/x.rs"));
        assert!(matches("crates/**/x.rs", "crates/a/b/x.rs"));
        assert!(matches("**", "any/path"));
        assert!(matches(
            "crates/*/0001_*.sql",
            "crates/sqlite/0001_baseline.sql"
        ));
        assert!(!matches(
            "crates/*/0001_*.sql",
            "crates/sqlite/0011_baseline.sql"
        ));
    }

    #[test]
    fn every_exception_needs_its_reason_and_a_text_with_the_name() {
        assert!(parse("crates/x.rs\n").is_err(), "no reason");
        assert!(
            parse("# why\n\ncrates/x.rs\n").is_err(),
            "the reason of another paragraph"
        );
        let text = concat!("# why\ncrates/x.rs | Te", "amId\ncrates/y.rs\n");
        let listed = exceptions(text);
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].text.as_deref(), Some(concat!("te", "amid")));
        assert_eq!(listed[1].text, None);
        assert!(
            parse("# why\ncrates/x.rs | workspace\n").is_err(),
            "a text without the name"
        );
    }

    #[test]
    fn a_line_passes_when_its_name_is_within_the_texts_of_its_file() {
        let listed = exceptions(concat!(
            "# why\n",
            "a.rs | `TestTe",
            "amUpload`\n",
            "b.rs\n",
            "# why\n",
            "c.rs | te",
            "ammate\n",
        ));
        let mut used = vec![false; listed.len()];
        let text = concat!(
            "fn x() {}\n",
            "// `TestTe",
            "amUpload`\n",
            "// `TestTe",
            "amUpload` and a te",
            "am\n",
            "let te",
            "am_id = 1;\n",
        );
        assert_eq!(
            check_file("a.rs", text, &listed, &mut used),
            ["a.rs:3", "a.rs:4"],
            "the rest of the line is checked, and another file's text does not count"
        );
        assert_eq!(used, [true, false, false]);
        assert_eq!(
            check_file(
                concat!("te", "ams/b.rs"),
                concat!("te", "am\n"),
                &listed,
                &mut used
            ),
            [
                concat!("te", "ams/b.rs: in its name"),
                concat!("te", "ams/b.rs:1")
            ],
            "a whole-file exception is for its own path"
        );
        assert_eq!(
            check_file("b.rs", concat!("TE", "AMS\n"), &listed, &mut used),
            Vec::<String>::new()
        );
        assert_eq!(
            used,
            [true, true, false],
            "an exception nothing used stays unused"
        );
    }
}
