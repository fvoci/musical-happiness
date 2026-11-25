//! Subunit splitting and `libtest` filter construction for `cargo xtask test --part`.
//!
//! A whole `api` integration test target holds ~1,600 tests (~25 minutes on PostgreSQL),
//! which alone would dominate any shard of `--part`. When `--part` divides a database into
//! two or more parts, `api` becomes two logical subunits (`api/1` and `api/2`), and default
//! weights place `api/1`, `api/2` and `store` into separate shards when no `--times` file
//! is given. After the chosen targets are built, `-- --list --format terse` enumerates the
//! binary's exact test names so each subunit receives a compact module-prefix + `--skip`
//! filter (with `--exact` fallback) whose matched set partitions the binary without missing
//! or repeating a single test.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;

use anyhow::{Context as _, Result, bail};

use super::units::{Part, Unit};

/// The integration test unit whose ~1,600 tests dominate PostgreSQL and SQLite runs.
const API_UNIT: &str = "api";

/// The integration test unit of `fvoci-store`, the next heaviest target on PostgreSQL.
const STORE_UNIT: &str = "store";

/// How many logical subunits [`API_UNIT`] is split into when `--part` has at least two parts.
const API_SUBUNITS: usize = 2;

/// Default weight (in seconds) of each [`API_UNIT`] subunit when no `--times` file is given,
/// sized from CI's ~25-minute PostgreSQL `api` run (~750–800s per half).
const DEFAULT_API_SUBUNIT_SECONDS: f64 = 800.0;

/// Default weight (in seconds) of [`STORE_UNIT`] when no `--times` file is given, so that
/// `api/1`, `api/2` and `store` (+ the smaller units) land on the three shards `1/3..3/3`.
const DEFAULT_STORE_SECONDS: f64 = 500.0;

/// Default weight (in seconds) of every other unit when no `--times` file is given.
const DEFAULT_OTHER_SECONDS: f64 = 1.0;

/// A sentinel test name that no Rust test function has, used with `--exact` when a part of
/// a tiny binary receives zero tests so `libtest` runs none instead of all.
const NO_TEST_SENTINEL: &str = "";

/// The `--unit` target name underlying `unit_name`: `"api"` for `"api/1"`, else `unit_name`.
pub(crate) fn base_name(unit_name: &str) -> &str {
    unit_name
        .split_once('/')
        .map_or(unit_name, |(base, _)| base)
}

/// Expands whole [`API_UNIT`] into [`API_SUBUNITS`] logical subunits when `part` has at
/// least two shards. Ignored/filtered units (such as S3's `with_s3` subset) stay whole.
pub(crate) fn expand_units(units: Vec<Unit>, part: Part) -> Vec<Unit> {
    if part.count() < API_SUBUNITS {
        return units;
    }
    let mut expanded = Vec::with_capacity(units.len() + API_SUBUNITS - 1);
    for unit in units {
        if unit.name == API_UNIT
            && unit.flags.is_empty()
            && unit.filter.is_empty()
            && unit.slice.is_none()
        {
            for index in 1..=API_SUBUNITS {
                expanded.push(Unit {
                    name: format!("{}/{index}", unit.name),
                    slice: Part::new(index, API_SUBUNITS),
                    ..unit.clone()
                });
            }
        } else {
            expanded.push(unit);
        }
    }
    expanded
}

/// Weights for `units`: `recorded` times when given (splitting a whole parent's time evenly
/// across its subunits if the subunits themselves were not recorded), else built-in CI hints
/// that place `api/1`, `api/2` and `store` (+ the remaining units) on separate shards.
pub(crate) fn weigh_units(units: &[Unit], recorded: &HashMap<String, f64>) -> HashMap<String, f64> {
    if recorded.is_empty() {
        return units
            .iter()
            .map(|unit| {
                let weight = if unit.slice.is_some() {
                    DEFAULT_API_SUBUNIT_SECONDS
                } else if unit.name == API_UNIT {
                    DEFAULT_API_SUBUNIT_SECONDS
                        * f64::from(u32::try_from(API_SUBUNITS).unwrap_or(2))
                } else if unit.name == STORE_UNIT {
                    DEFAULT_STORE_SECONDS
                } else {
                    DEFAULT_OTHER_SECONDS
                };
                (unit.name.clone(), weight)
            })
            .collect();
    }
    let mut weights = recorded.clone();
    for unit in units {
        if let Some(slice) = unit.slice
            && !weights.contains_key(&unit.name)
            && let Some(&whole) = recorded.get(base_name(&unit.name))
        {
            let divisor = u32::try_from(slice.count()).map_or(1.0, f64::from);
            weights.insert(unit.name.clone(), whole / divisor);
        }
    }
    weights
}

/// Lists every test and benchmark name of an already-built `cargo test` target via
/// `-- --list --format terse`, keeping `packages` so Cargo reuses the preceding build.
///
/// # Errors
///
/// Fails if `cargo test -- --list` cannot be started, exits non-zero, or lists no tests.
pub(crate) fn list_tests(
    root: &Path,
    cargo: &OsStr,
    packages: &[&str],
    targets: &[String],
) -> Result<Vec<String>> {
    let arguments: Vec<&str> = ["test", "--locked"]
        .into_iter()
        .chain(packages.iter().copied())
        .chain(targets.iter().map(String::as_str))
        .chain(["--", "--list", "--format", "terse"])
        .collect();
    let output = Command::new(cargo)
        .args(&arguments)
        .current_dir(root)
        .output()
        .with_context(|| format!("starting `cargo {}`", arguments.join(" ")))?;
    if !output.status.success() {
        bail!(
            "`cargo {}` ended with {}: {}",
            arguments.join(" "),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let text = String::from_utf8(output.stdout)
        .context("reading test list from `--list --format terse`")?;
    let tests = parse_listed_tests(&text);
    if tests.is_empty() {
        bail!("no tests listed for `{}`", targets.join(" "));
    }
    Ok(tests)
}

/// Parses `libtest`'s `--list --format terse` output into sorted, unique test names.
pub(crate) fn parse_listed_tests(output: &str) -> Vec<String> {
    output
        .lines()
        .map(|line| line.trim_end_matches('\r'))
        .filter_map(|line| {
            line.strip_suffix(": test")
                .or_else(|| line.strip_suffix(": benchmark"))
        })
        .map(str::to_owned)
        .collect::<BTreeSet<String>>()
        .into_iter()
        .collect()
}

/// Top-level module key (`"boards::"`) for a module test, or the bare test name itself.
fn group_key(test_name: &str) -> String {
    match test_name.split_once("::") {
        Some((module, _)) => format!("{module}::"),
        None => test_name.to_owned(),
    }
}

/// Deterministic `part` slice of sorted, unique `tests`: cuts along top-level module
/// boundaries closest to equal test counts when there are enough modules, else by index.
fn slice_sorted<'a>(sorted: &[&'a str], part: Part) -> Vec<&'a str> {
    let total = sorted.len();
    let count = part.count();
    let index = part.index();
    let mut grouped: BTreeMap<String, Vec<&'a str>> = BTreeMap::new();
    for &test in sorted {
        grouped.entry(group_key(test)).or_default().push(test);
    }
    let groups: Vec<(String, Vec<&'a str>)> = grouped.into_iter().collect();
    if groups.len() < count {
        let start = (index - 1) * total / count;
        let end = index * total / count;
        return sorted[start..end].to_vec();
    }
    let mut prefix = Vec::with_capacity(groups.len() + 1);
    prefix.push(0_usize);
    for (_, members) in &groups {
        prefix.push(prefix.last().copied().unwrap_or(0) + members.len());
    }
    let mut bounds = Vec::with_capacity(count + 1);
    bounds.push(0_usize);
    for cut in 1..count {
        let prev = bounds.last().copied().unwrap_or(0);
        let max_end = groups.len() - (count - cut);
        let best = ((prev + 1)..=max_end)
            .min_by_key(|&candidate| ((prefix[candidate] * count).abs_diff(cut * total), candidate))
            .unwrap_or(prev + 1);
        bounds.push(best);
    }
    bounds.push(groups.len());
    groups[bounds[index - 1]..bounds[index]]
        .iter()
        .flat_map(|(_, members)| members.iter().copied())
        .collect()
}

/// Builds `libtest` filter arguments for `part` of `tests` such that every test in `tests`
/// matches in `libtest` if and only if it belongs to `part`'s slice.
pub(crate) fn filter_for_part(tests: &[String], part: Part) -> Vec<String> {
    let unique: BTreeSet<&str> = tests.iter().map(String::as_str).collect();
    let sorted: Vec<&str> = unique.into_iter().collect();
    if sorted.is_empty() {
        return Vec::new();
    }
    let wanted = slice_sorted(&sorted, part);
    if wanted.is_empty() {
        let mut sentinel = NO_TEST_SENTINEL.to_owned();
        while sorted.contains(&sentinel.as_str()) {
            sentinel.push('_');
        }
        return vec!["--exact".to_owned(), sentinel];
    }
    if wanted.len() == sorted.len() {
        return Vec::new();
    }
    if let Some(compact) = compact_prefix_filter(&sorted, &wanted) {
        let exact_bytes: usize = 8 + wanted.iter().map(|name| name.len() + 1).sum::<usize>();
        let compact_bytes: usize = compact.iter().map(|word| word.len() + 1).sum();
        if compact_bytes <= exact_bytes {
            return compact;
        }
    }
    std::iter::once("--exact".to_owned())
        .chain(wanted.into_iter().map(str::to_owned))
        .collect()
}

/// Builds a module-prefix + `--skip` filter when substring matching separates `wanted`
/// from `sorted \ wanted` cleanly.
fn compact_prefix_filter(sorted: &[&str], wanted: &[&str]) -> Option<Vec<String>> {
    let wanted_set: BTreeSet<&str> = wanted.iter().copied().collect();
    let raw_positives: BTreeSet<String> = wanted.iter().map(|&test| group_key(test)).collect();
    // Drop any prefix that already contains a shorter chosen prefix (e.g. `org_admin::`
    // when `admin::` is in the same slice).
    let positives: Vec<String> = raw_positives
        .iter()
        .filter(|candidate| {
            !raw_positives
                .iter()
                .any(|other| other.len() < candidate.len() && candidate.contains(other.as_str()))
        })
        .cloned()
        .collect();
    let mut skips: BTreeSet<String> = BTreeSet::new();
    for &other in sorted {
        if wanted_set.contains(other) {
            continue;
        }
        if !positives.iter().any(|pos| other.contains(pos.as_str()))
            || skips.iter().any(|skip| other.contains(skip.as_str()))
        {
            continue;
        }
        let module = group_key(other);
        if !wanted.iter().any(|&want| want.contains(module.as_str())) {
            skips.insert(module);
        } else if !wanted.iter().any(|&want| want.contains(other)) {
            skips.insert(other.to_owned());
        } else {
            return None;
        }
    }
    let mut arguments = positives;
    for skip in skips {
        arguments.push("--skip".to_owned());
        arguments.push(skip);
    }
    sorted
        .iter()
        .all(|&test| libtest_matches(&arguments, test) == wanted_set.contains(test))
        .then_some(arguments)
}

/// Evaluates `libtest`'s filter rules (`[FILTER ...]`, `--exact`, `--skip SKIP`) on `test_name`.
pub(crate) fn libtest_matches(arguments: &[String], test_name: &str) -> bool {
    let mut exact = false;
    let mut filters: Vec<&str> = Vec::new();
    let mut skips: Vec<&str> = Vec::new();
    let mut iter = arguments.iter().map(String::as_str);
    while let Some(arg) = iter.next() {
        match arg {
            "--exact" => exact = true,
            "--skip" => {
                if let Some(skip) = iter.next() {
                    skips.push(skip);
                }
            }
            other if !other.starts_with('-') => filters.push(other),
            _ => {}
        }
    }
    let matches_one = |pattern: &str| {
        if exact {
            test_name == pattern
        } else {
            test_name.contains(pattern)
        }
    };
    let included = filters.is_empty() || filters.iter().any(|&pattern| matches_one(pattern));
    included && !skips.iter().any(|&pattern| matches_one(pattern))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::Database;
    use crate::test::units::{self, Services};

    fn part(text: &str) -> Part {
        text.parse().expect("valid part")
    }

    /// Walks an integration test directory (`tests/api/main.rs` + submodules) with `syn`
    /// and collects every `#[test]` / `#[tokio::test]` path (`module::name`).
    fn collect_suite_tests(entry: &Path) -> Vec<String> {
        let mut out = Vec::new();
        collect_module_tests(entry, "", &mut out);
        out.sort();
        out.dedup();
        out
    }

    fn collect_module_tests(file: &Path, prefix: &str, out: &mut Vec<String>) {
        let source = std::fs::read_to_string(file)
            .unwrap_or_else(|error| panic!("reading {}: {error}", file.display()));
        let parsed = syn::parse_file(&source)
            .unwrap_or_else(|error| panic!("parsing {}: {error}", file.display()));
        let parent = file.parent().expect("test file has a parent directory");
        for item in parsed.items {
            match item {
                syn::Item::Fn(function) if is_test_fn(&function.attrs) => {
                    let name = function.sig.ident.to_string();
                    let full = if prefix.is_empty() {
                        name
                    } else {
                        format!("{prefix}::{name}")
                    };
                    out.push(full);
                }
                syn::Item::Mod(module) => {
                    let mod_name = module.ident.to_string();
                    let child_prefix = if prefix.is_empty() {
                        mod_name.clone()
                    } else {
                        format!("{prefix}::{mod_name}")
                    };
                    if let Some((_, items)) = module.content {
                        collect_inline_items(&items, &child_prefix, out);
                    } else {
                        let direct = parent.join(format!("{mod_name}.rs"));
                        let nested = parent.join(&mod_name).join("mod.rs");
                        let target = if direct.is_file() { direct } else { nested };
                        if target.is_file() {
                            collect_module_tests(&target, &child_prefix, out);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn collect_inline_items(items: &[syn::Item], prefix: &str, out: &mut Vec<String>) {
        for item in items {
            match item {
                syn::Item::Fn(function) if is_test_fn(&function.attrs) => {
                    out.push(format!("{prefix}::{}", function.sig.ident));
                }
                syn::Item::Mod(module) => {
                    let next = format!("{prefix}::{}", module.ident);
                    if let Some((_, nested)) = &module.content {
                        collect_inline_items(nested, &next, out);
                    }
                }
                _ => {}
            }
        }
    }

    fn is_test_fn(attrs: &[syn::Attribute]) -> bool {
        attrs.iter().any(|attr| {
            attr.path()
                .segments
                .last()
                .is_some_and(|segment| segment.ident == "test")
        })
    }

    /// Proves that across `1..=count`, every test in `tests` is matched in `libtest` by
    /// at least one shard (union == `tests`) and at most one shard (intersections == ∅).
    fn assert_exact_partition(tests: &[String], count: usize) {
        let expected: BTreeSet<&str> = tests.iter().map(String::as_str).collect();
        let mut union: BTreeSet<&str> = BTreeSet::new();
        for index in 1..=count {
            let filter = filter_for_part(tests, Part::new(index, count).expect("valid part"));
            let matched: Vec<&str> = tests
                .iter()
                .map(String::as_str)
                .filter(|&name| libtest_matches(&filter, name))
                .collect();
            for name in matched {
                assert!(
                    union.insert(name),
                    "duplicate test `{name}` in part {index}/{count} (filter: {filter:?})"
                );
            }
        }
        assert_eq!(
            union,
            expected,
            "union across 1..={count} must equal all {} tests",
            expected.len()
        );
    }

    #[test]
    fn real_workspace_api_and_store_suites_partition_with_zero_loss_and_zero_overlap() {
        let root = crate::workspace_root();
        let api_tests = collect_suite_tests(&root.join("crates/server/tests/api/main.rs"));
        let store_tests = collect_suite_tests(&root.join("crates/store/tests/store/main.rs"));
        assert!(
            api_tests.len() >= 1500,
            "expected >= 1500 real api tests, found {}",
            api_tests.len()
        );
        assert!(
            store_tests.len() >= 600,
            "expected >= 600 real store tests, found {}",
            store_tests.len()
        );

        for count in 1..=5 {
            assert_exact_partition(&api_tests, count);
            assert_exact_partition(&store_tests, count);
        }

        let first = filter_for_part(&api_tests, part("1/2"));
        let second = filter_for_part(&api_tests, part("2/2"));
        let first_count = api_tests
            .iter()
            .filter(|name| libtest_matches(&first, name))
            .count();
        let second_count = api_tests
            .iter()
            .filter(|name| libtest_matches(&second, name))
            .count();
        assert_eq!(
            first_count + second_count,
            api_tests.len(),
            "halves sum to all api tests"
        );
        assert!(
            first_count * 100 / api_tests.len() >= 45 && second_count * 100 / api_tests.len() >= 45,
            "api halves stay balanced: {first_count} vs {second_count}"
        );
        for (label, filter) in [("api/1", &first), ("api/2", &second)] {
            let bytes: usize = filter.iter().map(|word| word.len() + 1).sum();
            assert!(
                bytes < 4096,
                "{label} filter stays compact ({bytes} bytes < 4 KiB)"
            );
            assert!(
                !filter.iter().any(|word| word == "--exact"),
                "{label} uses compact module prefixes rather than --exact: {filter:?}"
            );
        }
    }

    #[test]
    fn sqlite_and_postgres_three_way_parts_separate_api_halves_and_store_covering_every_test() {
        let root = crate::workspace_root();
        let metadata =
            units::workspace_metadata(&root, &crate::steps::cargo()).expect("workspace metadata");
        let api_tests = collect_suite_tests(&root.join("crates/server/tests/api/main.rs"));

        for database in [Database::Sqlite, Database::Postgres] {
            let raw = database.units(&metadata).expect("database units");
            let expected_non_api: BTreeSet<String> = raw
                .iter()
                .filter(|unit| unit.name != API_UNIT)
                .map(|unit| unit.name.clone())
                .collect();

            let mut seen_non_api = BTreeSet::new();
            let mut seen_api_tests = BTreeSet::new();
            let mut shard_units: Vec<Vec<String>> = Vec::new();

            for index in 1..=3 {
                let current = Part::new(index, 3).expect("part");
                let expanded = expand_units(raw.clone(), current);
                let weights = weigh_units(&expanded, &HashMap::new());
                let chosen = units::partition(expanded, &weights, current);
                shard_units.push(chosen.iter().map(|unit| unit.name.clone()).collect());
                for unit in chosen {
                    if let Some(slice) = unit.slice {
                        let filter = filter_for_part(&api_tests, slice);
                        for test in &api_tests {
                            if libtest_matches(&filter, test) {
                                assert!(
                                    seen_api_tests.insert(test.as_str()),
                                    "duplicate api test `{test}` in {} {current}",
                                    database.name()
                                );
                            }
                        }
                    } else {
                        assert!(
                            seen_non_api.insert(unit.name.clone()),
                            "duplicate unit `{}` in {} {current}",
                            unit.name,
                            database.name()
                        );
                    }
                }
            }

            assert_eq!(shard_units[0], ["api/1"], "{} 1/3", database.name());
            assert_eq!(shard_units[1], ["api/2"], "{} 2/3", database.name());
            assert!(
                shard_units[2].iter().any(|name| name == "store"),
                "{} 3/3 holds store: {:?}",
                database.name(),
                shard_units[2]
            );
            assert_eq!(
                seen_non_api,
                expected_non_api,
                "{} non-api units covered once",
                database.name()
            );
            assert_eq!(
                seen_api_tests.len(),
                api_tests.len(),
                "{} api tests covered once",
                database.name()
            );
        }

        // Single-part (`1/1`) and S3 (`--ignored` api subset) stay unsplit.
        let s3 = Database::S3.units("{}").expect("s3 units");
        assert_eq!(expand_units(s3.clone(), part("1/2")), s3, "s3 stays whole");
        let single = vec![Unit {
            name: "api".to_owned(),
            targets: vec!["--test".to_owned(), "api".to_owned()],
            flags: Vec::new(),
            filter: Vec::new(),
            slice: None,
            services: Services::Sqlite,
        }];
        assert_eq!(
            expand_units(single.clone(), part("1/1")),
            single,
            "1/1 does not split api"
        );
    }

    #[test]
    fn recorded_whole_api_time_splits_across_subunits_when_given() {
        let single = Unit {
            name: "api".to_owned(),
            targets: vec!["--test".to_owned(), "api".to_owned()],
            flags: Vec::new(),
            filter: Vec::new(),
            slice: None,
            services: Services::Postgres,
        };
        let expanded = expand_units(vec![single], part("1/3"));
        let recorded = HashMap::from([("api".to_owned(), 1500.0)]);
        let weights = weigh_units(&expanded, &recorded);
        assert_eq!(weights.get("api/1"), Some(&750.0), "first half gets half");
        assert_eq!(weights.get("api/2"), Some(&750.0), "second half gets half");
    }

    #[test]
    fn adversarial_prefixes_nested_modules_and_short_lists_partition_cleanly() {
        let colliding: Vec<String> = [
            "admin::a",
            "admin::socket::b",
            "admins::c",
            "comments::d",
            "org_admin::e",
            "page_comments::f",
            "unit_comments::g",
            "workspace_admin::h",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        for count in 1..=5 {
            assert_exact_partition(&colliding, count);
        }

        // Cross-nested submodules and top-level substring collisions (`smoke` inside `smoke_fast`).
        let tricky: Vec<String> = [
            "alpha::beta::one",
            "beta::alpha::two",
            "smoke",
            "smoke_fast",
            "smoke_slow",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        for count in 1..=5 {
            assert_exact_partition(&tricky, count);
        }

        // Single module with 500 tests and a tiny 1-test binary split across 3 parts.
        let single_mod: Vec<String> = (0..500).map(|i| format!("only::case_{i:03}")).collect();
        for count in 1..=4 {
            assert_exact_partition(&single_mod, count);
        }
        let tiny = vec!["solo::only".to_owned()];
        assert_exact_partition(&tiny, 3);
    }

    #[cfg(unix)]
    #[test]
    fn real_libtest_binary_runs_each_listed_test_in_exactly_one_part() {
        use crate::test::harness::Reading;

        let scratch = crate::files::testing::scratch("shards-libtest");
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&scratch).expect("scratch dir");
        let source = scratch.join("sample.rs");
        let binary = scratch.join("sample-bin");
        std::fs::write(
            &source,
            r"
            #[test] fn root_case() {}
            mod admin {
                #[test] fn first() {}
                pub mod socket { #[test] fn nested() {} }
            }
            mod boards { #[test] #[ignore] fn slow() {} }
            mod org_admin { #[test] fn other_admin() {} }
            mod workspace_admin { #[test] fn third_admin() {} }
            ",
        )
        .expect("writes sample test source");

        let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
        let status = Command::new(rustc)
            .args(["--edition=2024", "--test"])
            .arg(&source)
            .arg("-o")
            .arg(&binary)
            .status()
            .expect("invokes rustc --test");
        assert!(status.success(), "rustc --test succeeded");

        let listed_out = Command::new(&binary)
            .args(["--list", "--format", "terse"])
            .output()
            .expect("runs --list");
        assert!(listed_out.status.success(), "--list succeeded");
        let listed = parse_listed_tests(&String::from_utf8(listed_out.stdout).expect("utf8"));
        assert_eq!(
            listed,
            [
                "admin::first",
                "admin::socket::nested",
                "boards::slow",
                "org_admin::other_admin",
                "root_case",
                "workspace_admin::third_admin",
            ]
        );

        for count in [2_usize, 3] {
            let mut seen = BTreeSet::new();
            for index in 1..=count {
                let filter = filter_for_part(&listed, Part::new(index, count).expect("part"));
                let run_out = Command::new(&binary)
                    .args(&filter)
                    .output()
                    .expect("runs sample test binary");
                assert!(run_out.status.success(), "part {index}/{count} passed");
                let mut reading = Reading::default();
                for line in String::from_utf8(run_out.stdout).expect("utf8").lines() {
                    reading.line(line);
                }
                let report = reading.finish();
                for test in report.seen_tests {
                    assert!(
                        seen.insert(test.clone()),
                        "libtest ran `{test}` twice across {count} parts"
                    );
                }
            }
            let expected: BTreeSet<String> = listed.iter().cloned().collect();
            assert_eq!(
                seen, expected,
                "libtest covered every test across {count} parts"
            );
        }
        let _ = std::fs::remove_dir_all(&scratch);
    }
}
