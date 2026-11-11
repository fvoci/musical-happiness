//! `cargo xtask check`, the step "layers": which code may name which.
//!
//! The layers are defined in one place, [`LAYERS`]. Code of a layer names its own layer,
//! the layers its entry lists, and everything that is in no layer (the standard library,
//! `tokio`, `serde`). The step reads the sources of [`CRATES`]; what it sees and what it
//! cannot see is said at [`scan::read_source`].
//!
//! What breaks the rule today is listed in [`EXCEPTIONS`], by file and by what is named.
//! The list only shrinks: a use that is not listed fails the step, and so does a line
//! whose use is gone, until the line is removed.

mod scan;
mod walk;

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context as _, Result, bail};

use crate::steps;
use scan::{Declared, Reference, Target};
use walk::ModuleFile;

/// A layer: the code that belongs to it and the layers that code may name.
#[derive(Debug)]
pub(crate) struct Layer {
    name: &'static str,
    /// What the layer is made of: crates (`axum`) and modules of the crates of this
    /// workspace (`fvoci_server::app`), with everything under them. Where two layers
    /// claim a path, the longer claim wins: `fvoci_server::app` is logic although
    /// `fvoci_server` is HTTP.
    owns: &'static [&'static str],
    /// The other layers its code may name.
    may_use: &'static [&'static str],
    /// The accessors and fields through which other code holds this layer: `.store()`
    /// and `.store` are the database, whatever the type is called where they are used.
    handles: &'static [&'static str],
}

/// The layers, from the top. A request comes in through HTTP, which asks the logic,
/// which asks the store; all three speak in the types of the model.
///
/// * **http** does not call the store itself: it has no query to write and no
///   transaction to keep, and what it needs it asks of the logic.
/// * **logic** does not know HTTP: no `axum`, no request, response, header or status.
///   What it decides can be asked from a test or a task as well as from a handler.
///
/// A module of the server that is not listed under `logic` is HTTP: the one that may do
/// least with the data.
pub(crate) const LAYERS: &[Layer] = &[
    Layer {
        name: "http",
        owns: &[
            "fvoci_server",
            "axum",
            "axum_core",
            "axum_extra",
            "http",
            "http_body",
            "http_body_util",
            "hyper",
            "hyper_util",
            "tower",
            "tower_http",
            "tower_layer",
            "tower_service",
            "utoipa_axum",
        ],
        may_use: &["logic", "model"],
        handles: &[],
    },
    Layer {
        name: "logic",
        owns: &[
            "fvoci_server::app",
            "fvoci_server::auth",
            "fvoci_server::config",
            "fvoci_server::error",
            "fvoci_server::permissions",
            "fvoci_server::scheduler",
            "fvoci_server::storage",
            "fvoci_server::webhook",
        ],
        may_use: &["store", "model"],
        handles: &[],
    },
    Layer {
        name: "store",
        owns: &["fvoci_store"],
        may_use: &["model"],
        handles: &["store"],
    },
    Layer {
        name: "model",
        owns: &["fvoci_model"],
        may_use: &[],
        handles: &[],
    },
];

/// The crates whose sources are read: the name paths are written with, and the
/// directory of the sources. Tests outside `src` are not read: they drive the server
/// from outside and prepare what they need in the store.
pub(crate) const CRATES: &[(&str, &str)] = &[
    ("fvoci_server", "crates/server/src"),
    ("fvoci_store", "crates/store/src"),
    ("fvoci_model", "crates/model/src"),
];

/// The file of the uses that break the rule and are still there.
pub(crate) const EXCEPTIONS: &str = "xtask/exceptions/layers.txt";

/// Code of one layer that names another layer it may not name.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Violation {
    /// The file, from the workspace root, with `/`.
    file: String,
    line: usize,
    from: &'static str,
    to: &'static str,
    /// What is named: a path (`fvoci_store::Store`) or a handle (`.store().get_block`).
    named: String,
}

/// The step: reads the sources and compares what breaks the rule with the list.
///
/// # Errors
///
/// Fails if a source cannot be read, or if the uses that break the rule are not exactly
/// the listed ones.
pub(crate) fn run(root: &Path) -> Result<()> {
    let sources = Sources::read(root)?;
    let path = root.join(EXCEPTIONS);
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let listed = parse_exceptions(&text).with_context(|| format!("reading {EXCEPTIONS}"))?;
    let mut problems = sources.unfollowed();
    problems.extend(judge(&sources.violations(), &listed));
    if problems.is_empty() {
        steps::say(&format!(
            "    no layer names another against the rule; {} listed exceptions remain",
            listed.values().sum::<usize>()
        ));
        return Ok(());
    }
    for problem in &problems {
        steps::say(&format!("    {problem}"));
    }
    steps::say(
        "    the list only shrinks: a use that is not listed moves to a layer that may \
         name it, and the line of a use that is gone is removed",
    );
    bail!(
        "{} differences between the code and {EXCEPTIONS}",
        problems.len()
    )
}

/// What the sources of the workspace name.
#[derive(Debug, Default)]
struct Sources {
    /// Each file, as the list and the messages write it, with what it names.
    files: Vec<(String, Vec<Reference>)>,
    /// The names given to paths, in any module: who names the key names the values.
    aliases: BTreeMap<String, Vec<String>>,
    /// The modules that import everything of a path (`use path::*`): who names
    /// something in the key may be naming it in one of the values.
    globs: BTreeMap<String, Vec<String>>,
}

impl Sources {
    /// Reads the sources of [`CRATES`] below `root`.
    fn read(root: &Path) -> Result<Self> {
        let mut sources = Self::default();
        for (name, directory) in CRATES {
            walk::walk_crate(name, &root.join(directory), &mut |module_file| {
                let ModuleFile { module, file } = module_file;
                let source = std::fs::read_to_string(file)
                    .with_context(|| format!("reading {}", file.display()))?;
                let shown = shown_path(root, file);
                sources
                    .add(&shown, module, &source)
                    .with_context(|| format!("reading {shown}"))
            })?;
        }
        sources.files.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(sources)
    }

    /// Adds the file `file`, which is the module `module`, and answers the modules it
    /// declares.
    fn add(&mut self, file: &str, module: &[String], source: &str) -> Result<Vec<Declared>> {
        let handles: Vec<&str> = LAYERS
            .iter()
            .flat_map(|layer| layer.handles.iter().copied())
            .collect();
        let read = scan::read_source(module, source, &handles)?;
        for alias in read.aliases {
            self.aliases
                .entry(alias.name)
                .or_default()
                .push(alias.target);
        }
        for reference in &read.references {
            if let Target::Path(path) = &reference.target
                && let Some(everything_of) = path.strip_suffix("::*")
            {
                self.globs
                    .entry(reference.module.clone())
                    .or_default()
                    .push(everything_of.to_owned());
            }
        }
        self.files.push((file.to_owned(), read.references));
        Ok(read.declared)
    }

    /// Everything a path may stand for: itself, and what the names on its way were
    /// given to. A re-export, a renamed import and a type alias lead on from here.
    fn reached(&self, path: &str) -> Vec<String> {
        let mut reached = vec![path.to_owned()];
        let mut at = 0;
        while let Some(current) = reached.get(at).cloned()
            && reached.len() < MOST_FORMS
        {
            at += 1;
            let ends = current
                .match_indices("::")
                .map(|(end, _)| end)
                .chain([current.len()]);
            for end in ends {
                let (prefix, rest) = current.split_at(end);
                let named = self.aliases.get(prefix).into_iter().flatten();
                // What is named inside a module that imports everything of a path may
                // be what that path holds.
                let held = self
                    .globs
                    .get(prefix)
                    .into_iter()
                    .flatten()
                    .filter(|_| !rest.is_empty());
                for target in named.chain(held) {
                    let next = format!("{target}{rest}");
                    if !reached.contains(&next) {
                        reached.push(next);
                    }
                }
            }
        }
        reached
    }

    /// The layer `from` may not name and `path` leads to, with the path as it is there.
    fn broken_by(&self, from: &Layer, path: &str) -> Option<(&'static Layer, String)> {
        self.reached(path).into_iter().find_map(|form| {
            let to = layer_of(&form)?;
            (from.name != to.name && !from.may_use.contains(&to.name)).then_some((to, form))
        })
    }

    /// Every use that breaks the rule.
    fn violations(&self) -> Vec<Violation> {
        let mut violations = Vec::new();
        for (file, references) in &self.files {
            for reference in references {
                let Some(from) = layer_of(&reference.module) else {
                    continue;
                };
                let broken = match &reference.target {
                    Target::Path(path) => self.broken_by(from, path),
                    Target::Handle { name, written } => LAYERS
                        .iter()
                        .find(|layer| layer.handles.contains(&name.as_str()))
                        .filter(|to| from.name != to.name && !from.may_use.contains(&to.name))
                        .map(|to| (to, written.clone())),
                };
                if let Some((to, named)) = broken {
                    violations.push(Violation {
                        file: file.clone(),
                        line: reference.line,
                        from: from.name,
                        to: to.name,
                        named,
                    });
                }
            }
        }
        violations
    }

    /// The imports of everything of another layer (`use fvoci_store::*`). They are
    /// refused, listed or not: what such an import brings in has no name to follow.
    fn unfollowed(&self) -> Vec<String> {
        let mut problems = Vec::new();
        for (file, references) in &self.files {
            for reference in references {
                let Target::Path(path) = &reference.target else {
                    continue;
                };
                let layers = (layer_of(&reference.module), layer_of(path));
                if let (Some(from), Some(to)) = layers
                    && path.ends_with("::*")
                    && from.name != to.name
                {
                    problems.push(format!(
                        "{file}:{}: {} imports everything of {}: {path}. Name what is imported",
                        reference.line, from.name, to.name
                    ));
                }
            }
        }
        problems
    }
}

/// How many forms of one path are followed: more than any real chain of names, and an
/// end for names that are given in a circle.
const MOST_FORMS: usize = 64;

/// A path below the workspace root as the list and the messages write it.
fn shown_path(root: &Path, file: &Path) -> String {
    let relative = file.strip_prefix(root).unwrap_or(file);
    let parts: Vec<String> = relative
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect();
    parts.join("/")
}

/// The layer a path belongs to: the one with the longest claim on it.
fn layer_of(path: &str) -> Option<&'static Layer> {
    LAYERS
        .iter()
        .flat_map(|layer| layer.owns.iter().map(move |owned| (layer, *owned)))
        .filter(|(_, owned)| {
            path.strip_prefix(owned)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with("::"))
        })
        .max_by_key(|(_, owned)| owned.len())
        .map(|(layer, _)| layer)
}

/// Reads the list: a line is a file, what it names, and how many times, separated by
/// spaces. Empty lines and lines that start with `#` are skipped.
fn parse_exceptions(text: &str) -> Result<BTreeMap<(String, String), usize>> {
    let mut listed = BTreeMap::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        let [file, named, count] = fields[..] else {
            bail!(
                "line {}: expected `file named count`, found `{line}`",
                index + 1
            );
        };
        let count: usize = count
            .parse()
            .ok()
            .filter(|count| *count > 0)
            .with_context(|| format!("line {}: `{count}` is no count", index + 1))?;
        if listed
            .insert((file.to_owned(), named.to_owned()), count)
            .is_some()
        {
            bail!("line {}: {file} {named} is listed twice", index + 1);
        }
    }
    Ok(listed)
}

/// Compares what the code does with what the list allows. Each line of the answer is
/// one difference: a use the list does not have, or a listed use the code no longer has.
fn judge(violations: &[Violation], listed: &BTreeMap<(String, String), usize>) -> Vec<String> {
    let mut found: BTreeMap<(String, String), Vec<&Violation>> = BTreeMap::new();
    for violation in violations {
        found
            .entry((violation.file.clone(), violation.named.clone()))
            .or_default()
            .push(violation);
    }
    let mut problems = Vec::new();
    for ((file, named), uses) in &found {
        let allowed = listed
            .get(&(file.clone(), named.clone()))
            .copied()
            .unwrap_or(0);
        let Some(first) = uses.first().filter(|_| uses.len() > allowed) else {
            continue;
        };
        let lines: Vec<String> = uses.iter().map(|used| used.line.to_string()).collect();
        let counted = if allowed == 0 {
            String::new()
        } else {
            format!(" ({} times, listed {allowed})", uses.len())
        };
        problems.push(format!(
            "{file}:{}: {} names {}: {named}{counted}",
            lines.join(","),
            first.from,
            first.to,
        ));
    }
    for ((file, named), allowed) in listed {
        let now = found
            .get(&(file.clone(), named.clone()))
            .map_or(0, Vec::len);
        if now < *allowed {
            let fix = if now == 0 {
                "remove its line".to_owned()
            } else {
                format!("lower its count to {now}")
            };
            problems.push(format!(
                "{EXCEPTIONS}: {file} names {named} {now} times, listed {allowed}: {fix}"
            ));
        }
    }
    problems
}

#[cfg(test)]
mod tests;
