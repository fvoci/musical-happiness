use super::measure::measure_source;
use super::*;
use crate::files::testing::{scratch, write};

const CODE: &str = "crates/a/src/lib.rs";

fn file(name: &str, lines: usize) -> Length {
    Length {
        subject: Subject::File(name.to_owned()),
        lines,
        at: 1,
    }
}

/// A function of the file [`CODE`].
fn function(name: &str, lines: usize, at: usize) -> Length {
    function_in(CODE, name, lines, at)
}

fn function_in(file: &str, name: &str, lines: usize, at: usize) -> Length {
    Length {
        subject: Subject::Function {
            file: file.to_owned(),
            name: name.to_owned(),
        },
        lines,
        at,
    }
}

fn listed(text: &str) -> Over {
    parse_exceptions(text).expect("the list reads")
}

/// The functions of `source`, as `name lines`.
fn functions(source: &str) -> Vec<String> {
    measure_source("planted.rs", source)
        .expect("the source parses")
        .lengths
        .into_iter()
        .filter_map(|length| match length.subject {
            Subject::Function { name, .. } => Some(format!("{name} {}", length.lines)),
            Subject::File(_) => None,
        })
        .collect()
}

/// What is refused in `source`, as the file `file`, whatever its lengths.
fn refused(file: &str, source: &str) -> Vec<String> {
    measure_source(file, source)
        .expect("the source parses")
        .refused
}

/// A function of `lines` lines.
fn long(name: &str, lines: usize) -> String {
    format!("fn {name}() {{\n{}}}\n", "    // …\n".repeat(lines - 2))
}

/// The differences the step finds in the workspace `root`.
fn differences(root: &Path) -> Vec<String> {
    let measured = measure_all(root).expect("the sources read");
    let text = std::fs::read_to_string(root.join(EXCEPTIONS)).expect("the list is there");
    judge(&measured.lengths, &listed(&text))
}

#[test]
fn what_is_within_its_limit_needs_no_line() {
    let measured = [
        file(CODE, MAX_FILE_LINES),
        file("crates/a/tests/api/main.rs", MAX_TEST_FILE_LINES),
        file("crates/a/src/lengths/tests.rs", MAX_TEST_FILE_LINES),
        function("short", MAX_FUNCTION_LINES, 1),
        function("tests::a_scene", MAX_TEST_FUNCTION_LINES, 1),
        function_in(
            "crates/a/tests/api/main.rs",
            "scene",
            MAX_TEST_FUNCTION_LINES,
            1,
        ),
    ];
    assert_eq!(
        judge(&measured, &Over::new()),
        Vec::<String>::new(),
        "at the limit is within it"
    );
}

#[test]
fn a_long_file_and_a_long_function_that_are_not_listed_are_refused() {
    let measured = [
        file(CODE, 1001),
        file("crates/a/tests/api/main.rs", 2001),
        function("grown", 101, 7),
        function("tests::a_scene", 201, 900),
    ];
    assert_eq!(
        judge(&measured, &listed("# nothing\n")),
        [
            "crates/a/src/lib.rs:1: crates/a/src/lib.rs has 1001 lines, the limit is 1000",
            "crates/a/tests/api/main.rs:1: crates/a/tests/api/main.rs has 2001 lines, the limit is 2000",
            "crates/a/src/lib.rs:7: crates/a/src/lib.rs: fn grown has 101 lines, the limit is 100",
            "crates/a/src/lib.rs:900: crates/a/src/lib.rs: fn tests::a_scene has 201 lines, the limit is 200",
        ]
    );
}

#[test]
fn only_the_tests_of_a_crate_get_the_limits_of_tests() {
    for (path, of_tests) in [
        ("crates/a/tests/api/main.rs", true),
        ("xtask/tests/it.rs", true),
        ("crates/a/src/app/tests.rs", true),
        ("crates/a/src/tests/production.rs", false),
        ("crates/a/src/testsuite.rs", false),
        ("crates/a/src/app/contests.rs", false),
    ] {
        assert_eq!(is_test_file(path), of_tests, "{path}");
    }
    let limit = |file: &str, name: &str| function_in(file, name, 1, 1).subject.limit();
    assert_eq!(limit(CODE, "tests::a_scene"), MAX_TEST_FUNCTION_LINES);
    assert_eq!(limit(CODE, "contests::run"), MAX_FUNCTION_LINES);
    assert_eq!(
        limit("crates/a/tests/it.rs", "scene"),
        MAX_TEST_FUNCTION_LINES
    );
    assert_eq!(
        limit("crates/a/src/tests/production.rs", "run"),
        MAX_FUNCTION_LINES,
        "a directory named tests inside src is code"
    );
}

#[test]
fn the_name_of_the_test_module_is_kept_for_it() {
    let source = "
mod tests { fn production() {} }
#[cfg(not(test))] mod tests {}
mod inner { mod tests; }
fn tests() {}
struct tests;
impl tests { fn run(&self) {} }
trait tests {}
";
    let lines: Vec<String> = refused(CODE, source)
        .iter()
        .map(|problem| problem.split(' ').take(4).collect::<Vec<_>>().join(" "))
        .collect();
    assert_eq!(
        lines,
        [
            "crates/a/src/lib.rs:2: a module that",
            "crates/a/src/lib.rs:3: a module that",
            "crates/a/src/lib.rs:4: a module that",
            "crates/a/src/lib.rs:5: a function is",
            "crates/a/src/lib.rs:6: a type is",
            "crates/a/src/lib.rs:7: a type is",
            "crates/a/src/lib.rs:8: a trait is",
        ],
        "code cannot take the limit of tests by its name"
    );
    assert_eq!(
        refused(
            CODE,
            "#[cfg(test)] mod tests { fn a() {} }\n#[cfg(all(test, unix))] mod tests;\nmod contests {}"
        ),
        Vec::<String>::new(),
        "a test module, inline or in its file"
    );
    assert_eq!(
        refused("crates/a/tests/api/main.rs", "mod tests; fn tests() {}"),
        Vec::<String>::new(),
        "in the test directory of a crate the name decides nothing"
    );
}

#[test]
fn a_line_that_the_formatter_does_not_break_is_refused() {
    let problems = refused(
        CODE,
        "fn fine() {}\n#[rustfmt::skip]\nfn crammed() { a(); b(); c(); }\n#[cfg_attr(rustfmt, rustfmt::skip)]\nmod m {}",
    );
    assert_eq!(problems.len(), 2, "{problems:?}");
    assert!(
        problems[0].starts_with("crates/a/src/lib.rs:2: rustfmt::skip")
            && problems[1].starts_with("crates/a/src/lib.rs:4: rustfmt::skip"),
        "{problems:?}"
    );
    let error = measure_source(CODE, "fn a() {\r    b();\r}\r").expect_err("lone carriage returns");
    assert!(
        format!("{error:#}").contains("carriage return"),
        "lines that end without a line feed are not counted as one: {error:#}"
    );
    assert_eq!(
        functions("fn a() {\r\n    b();\r\n}\r\n"),
        ["a 3"],
        "a carriage return before the line feed is fine"
    );
}

#[test]
fn what_is_listed_is_held_to_its_ceiling_up_and_down() {
    let list = listed("file 1200 crates/a/src/lib.rs\nfunction 150 crates/a/src/lib.rs App::run\n");
    let measured = |file_lines, function_lines| {
        [
            file(CODE, file_lines),
            function("App::run", function_lines, 9),
        ]
    };
    assert_eq!(
        judge(&measured(1200, 150), &list),
        Vec::<String>::new(),
        "exactly at the ceiling"
    );
    assert_eq!(
        judge(&measured(1201, 151), &list),
        [
            "crates/a/src/lib.rs:1: crates/a/src/lib.rs has 1201 lines, its ceiling is 1200",
            "crates/a/src/lib.rs:9: crates/a/src/lib.rs: fn App::run has 151 lines, its ceiling is 150",
        ],
        "longer than the ceiling"
    );
    assert_eq!(
        judge(&measured(1199, 149), &list),
        [
            format!(
                "{EXCEPTIONS}: crates/a/src/lib.rs has 1199 lines now: lower its ceiling of 1200"
            ),
            format!(
                "{EXCEPTIONS}: crates/a/src/lib.rs: fn App::run has 149 lines now: lower its ceiling of 150"
            ),
        ],
        "shorter than the ceiling"
    );
    assert_eq!(
        judge(&measured(1000, 100), &list),
        [
            format!(
                "{EXCEPTIONS}: crates/a/src/lib.rs has 1000 lines now, within the limit of 1000: \
                 remove its line (1200)"
            ),
            format!(
                "{EXCEPTIONS}: crates/a/src/lib.rs: fn App::run has 100 lines now, within the limit of 100: \
                 remove its line (150)"
            ),
        ],
        "within the limit"
    );
}

#[test]
fn a_line_whose_file_or_function_is_gone_is_refused() {
    let list = listed("file 1200 crates/a/src/gone.rs\nfunction 150 crates/a/src/lib.rs renamed\n");
    assert_eq!(
        judge(&[file(CODE, 10), function("other", 10, 1)], &list),
        [
            format!(
                "{EXCEPTIONS}: crates/a/src/gone.rs is not in the code: remove its line (1200)"
            ),
            format!(
                "{EXCEPTIONS}: crates/a/src/lib.rs: fn renamed is not in the code: remove its line (150)"
            ),
        ]
    );
}

#[test]
fn two_functions_of_one_name_are_told_apart_by_their_lengths() {
    let list = listed(
        "function 120 crates/a/src/lib.rs Wire::new\nfunction 150 crates/a/src/lib.rs Wire::new\n",
    );
    let two = |first, second| {
        vec![
            function("Wire::new", first, 1),
            function("Wire::new", second, 200),
            function("Wire::new", 3, 400),
        ]
    };
    assert_eq!(judge(&two(120, 150), &list), Vec::<String>::new());
    assert_eq!(
        judge(&two(121, 150), &list),
        [
            "crates/a/src/lib.rs:1: crates/a/src/lib.rs: fn Wire::new has 121 lines, its ceiling is 120"
        ]
    );
    let mut three = two(120, 150);
    three.push(function("Wire::new", 101, 600));
    assert_eq!(
        judge(&three, &list),
        [
            "crates/a/src/lib.rs:600: crates/a/src/lib.rs: fn Wire::new has 101 lines, the limit is 100"
        ]
    );
}

#[test]
fn the_list_is_written_from_what_was_measured_and_what_it_writes_passes() {
    let measured = [
        file("crates/b/src/lib.rs", 1100),
        file(CODE, 20),
        file("crates/a/tests/it.rs", 1500),
        function("seed", 223, 1),
        function("Wire::new", 139, 30),
        function("Wire::new", 180, 300),
        function("small", 5, 2),
    ];
    let text = render_exceptions(&over_limit(&measured)).expect("the list is written");
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    assert_eq!(
        lines,
        [
            "file 1100 crates/b/src/lib.rs",
            "function 180 crates/a/src/lib.rs Wire::new",
            "function 139 crates/a/src/lib.rs Wire::new",
            "function 223 crates/a/src/lib.rs seed",
        ]
    );
    assert_eq!(
        judge(&measured, &listed(&text)),
        Vec::<String>::new(),
        "what was just written is what the code needs"
    );
    let error = render_exceptions(&over_limit(&[file("crates/a/src/my file.rs", 1001)]))
        .expect_err("a path the list could not be read back with");
    assert!(
        error.to_string().contains("has a space in its path"),
        "{error}"
    );
}

#[test]
fn a_list_that_cannot_be_read_or_has_a_needless_line_is_refused() {
    for (text, why) in [
        ("file crates/a/src/lib.rs", "expected `file lines path`"),
        (
            "file 1200 crates/a/src/lib.rs extra",
            "expected `file lines path`",
        ),
        (
            "function 150 crates/a/src/lib.rs",
            "expected `file lines path`",
        ),
        (
            "module 1200 crates/a/src/lib.rs",
            "expected `file lines path`",
        ),
        ("file many crates/a/src/lib.rs", "is no number of lines"),
        ("file 1000 crates/a/src/lib.rs", "within the limit of 1000"),
        ("file 2000 crates/a/tests/it.rs", "within the limit of 2000"),
        (
            "file 2000 crates/a/src/app/tests.rs",
            "within the limit of 2000",
        ),
        (
            "function 100 crates/a/src/lib.rs f",
            "within the limit of 100",
        ),
        (
            "function 200 crates/a/src/lib.rs tests::f",
            "within the limit of 200",
        ),
        (
            "function 200 crates/a/tests/it.rs f",
            "within the limit of 200",
        ),
    ] {
        let error = parse_exceptions(text).expect_err("the list is refused");
        assert!(
            format!("{error:#}").contains(why),
            "{text:?} is refused: {error:#}"
        );
    }
}

#[test]
fn a_planted_long_function_is_measured_from_fn_to_its_closing_brace() {
    let source = format!(
        "/// Documented.\n#[inline]\npub(crate) async\nfn planted() {{\n{}}}\n",
        "\n    // a comment counts, and so does an empty line\n".repeat(60)
    );
    assert_eq!(functions(&source), ["planted 122"]);
    let measured = measure_source(CODE, &source).expect("the source parses");
    assert_eq!(
        judge(&measured.lengths, &Over::new()),
        ["crates/a/src/lib.rs:4: crates/a/src/lib.rs: fn planted has 122 lines, the limit is 100"]
    );
}

#[test]
fn a_function_is_named_with_what_is_around_it() {
    let source = "
fn free() {}
struct App;
impl App {
    fn run(&self) {
        fn helper() {}
        let closure = || {
            1
        };
    }
}
impl<T> From<T> for App { fn from(_: T) -> Self { App } }
impl From<&'static str> for App { fn from(_: &'static str) -> Self { App } }
impl std::fmt::Display for &App { fn fmt(&self) {} }
impl Tuple for (u8, u8) { fn first(&self) {} }
trait Shape {
    fn area(&self) -> u32;
    fn doubled(&self) -> u32 {
        self.area() * 2
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn r#move() {}
}
";
    assert_eq!(
        functions(source),
        [
            "free 1",
            "App::run 6",
            "App::run::helper 1",
            "<App as From<T>>::from 1",
            "<App as From<&'staticstr>>::from 1",
            "<App as std::fmt::Display>::fmt 1",
            "<_ as Tuple>::first 1",
            "Shape::doubled 3",
            "tests::move 1",
        ],
        "a closure is part of its function; a declaration without a body is no function; \
         the conversions from two types are two names"
    );
}

#[test]
fn a_file_is_measured_in_lines() {
    let lines = |source| {
        measure_source("a.rs", source)
            .expect("the source parses")
            .lengths
            .first()
            .map(|length| length.lines)
    };
    assert_eq!(lines(""), Some(0));
    assert_eq!(lines("fn a() {}\n"), Some(1));
    assert_eq!(
        lines("fn a() {}\n\nfn b() {}"),
        Some(3),
        "without a last line break"
    );
    assert!(
        measure_source("a.rs", "fn (").is_err(),
        "a file that does not parse is not passed over"
    );
}

#[test]
fn planted_long_code_is_found_in_a_workspace_and_the_list_is_rewritten_from_it() {
    let root = scratch("lengths-workspace");
    write(&root, "crates/a/src/lib.rs", &long("planted", 101));
    write(&root, "crates/a/src/long.rs", &"// a line\n".repeat(1001));
    write(
        &root,
        "crates/a/tests/it.rs",
        &format!("{}{}", long("scene", 200), "// a line\n".repeat(801)),
    );
    write(&root, "crates/a/assets/data.txt", &"a line\n".repeat(5000));
    write(&root, "xtask/src/main.rs", "fn main() {}\n");
    write(&root, EXCEPTIONS, "");

    let not_listed = [
        "crates/a/src/long.rs:1: crates/a/src/long.rs has 1001 lines, the limit is 1000",
        "crates/a/src/lib.rs:1: crates/a/src/lib.rs: fn planted has 101 lines, the limit is 100",
    ];
    assert_eq!(
        differences(&root),
        not_listed,
        "a test file and a test of that length are within their limits, and only Rust is measured"
    );
    let error = run(&root).expect_err("the step fails for what is not listed");
    assert_eq!(
        error.to_string(),
        format!("2 differences between the code and {EXCEPTIONS}")
    );

    rewrite(&root).expect("the list is written");
    run(&root).expect("the step passes with the list that was just written");
    let text = std::fs::read_to_string(root.join(EXCEPTIONS)).expect("the list is there");
    assert!(
        text.ends_with(
            "\nfile 1001 crates/a/src/long.rs\nfunction 101 crates/a/src/lib.rs planted\n"
        ),
        "the list has the two: {text}"
    );

    write(&root, "crates/a/src/lib.rs", &long("planted", 102));
    assert_eq!(
        differences(&root),
        [
            "crates/a/src/lib.rs:1: crates/a/src/lib.rs: fn planted has 102 lines, its ceiling is 101"
        ],
        "a listed function that grows"
    );
    run(&root).expect_err("and the step fails for it");
    write(&root, "crates/a/src/lib.rs", &long("planted", 100));
    assert_eq!(
        differences(&root),
        [format!(
            "{EXCEPTIONS}: crates/a/src/lib.rs: fn planted has 100 lines now, within the limit of 100: \
             remove its line (101)"
        )],
        "a listed function that came within the limit"
    );
    run(&root).expect_err("and the step fails until its line is removed");

    write(
        &root,
        "crates/a/src/lib.rs",
        "#[rustfmt::skip]\nfn crammed() {}\n",
    );
    let error = rewrite(&root).expect_err("what is refused whatever its length");
    assert!(
        error.to_string().contains("the list is not written"),
        "no list is written for what no list excuses: {error}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[cfg(unix)]
#[test]
fn a_symbolic_link_among_the_sources_is_refused() {
    let root = scratch("lengths-link");
    write(&root, "crates/a/src/lib.rs", "fn a() {}\n");
    std::os::unix::fs::symlink(
        root.join("crates/a/src/lib.rs"),
        root.join("crates/a/src/linked.rs"),
    )
    .expect("the link is made");
    let error = files::rust_files(&root.join("crates")).expect_err("a link is not followed");
    assert!(
        error.to_string().ends_with("linked.rs is a symbolic link"),
        "{error}"
    );
    let _ = std::fs::remove_dir_all(&root);
}
