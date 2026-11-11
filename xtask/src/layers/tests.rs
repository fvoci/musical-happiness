use std::path::PathBuf;

use super::scan::read_source;
use super::walk::walk_crate;
use super::*;

/// The sources of a workspace made of `files`: each a module and its source.
fn sources(files: &[(&str, &str)]) -> Sources {
    let mut sources = Sources::default();
    for (module, source) in files {
        let path: Vec<String> = module.split("::").map(str::to_owned).collect();
        sources
            .add(&format!("{module}.rs"), &path, source)
            .expect("the source parses");
    }
    sources
}

/// What the module `module` names against the rule in a workspace made of `files`:
/// `from>to named`.
fn broken_in(files: &[(&str, &str)], module: &str) -> Vec<String> {
    sources(files)
        .violations()
        .iter()
        .filter(|violation| violation.file == format!("{module}.rs"))
        .map(|violation| format!("{}>{} {}", violation.from, violation.to, violation.named))
        .collect()
}

/// What `source`, as the module `module`, names against the rule: `from>to named`.
fn broken(module: &str, source: &str) -> Vec<String> {
    broken_in(&[(module, source)], module)
}

/// The paths `source` names, resolved, as the module `module`.
fn paths(module: &str, source: &str) -> Vec<String> {
    let module: Vec<String> = module.split("::").map(str::to_owned).collect();
    read_source(&module, source, &[])
        .expect("the source parses")
        .references
        .into_iter()
        .map(|Reference { target, .. }| match target {
            Target::Path(path) => path,
            Target::Handle { written, .. } => written,
        })
        .collect()
}

/// A directory for one test's files, under the workspace's `target/`.
fn scratch(name: &str) -> PathBuf {
    let directory = crate::workspace_root()
        .join("target")
        .join("tmp")
        .join("xtask")
        .join(name);
    let _ = std::fs::remove_dir_all(&directory);
    directory
}

fn write(root: &Path, file: &str, text: &str) {
    let path = root.join(file);
    std::fs::create_dir_all(path.parent().expect("the file is in a directory"))
        .expect("the directory is created");
    std::fs::write(path, text).expect("the file is written");
}

#[test]
fn the_table_of_layers_is_consistent() {
    let mut owned: Vec<&str> = Vec::new();
    for layer in LAYERS {
        for used in layer.may_use {
            assert!(
                LAYERS.iter().any(|other| other.name == *used),
                "{} may use {used}, which is no layer",
                layer.name
            );
        }
        owned.extend(layer.owns);
    }
    let count = owned.len();
    owned.sort_unstable();
    owned.dedup();
    assert_eq!(owned.len(), count, "no path is claimed twice");
    for (name, _) in CRATES {
        assert!(layer_of(name).is_some(), "the crate {name} is in a layer");
    }
}

#[test]
fn a_path_belongs_to_the_layer_with_the_longest_claim() {
    let name = |path| layer_of(path).map(|layer| layer.name);
    assert_eq!(name("fvoci_server"), Some("http"), "the crate root");
    assert_eq!(name("fvoci_server::api::users"), Some("http"));
    assert_eq!(name("fvoci_server::brand_new"), Some("http"), "the default");
    assert_eq!(name("fvoci_server::app"), Some("logic"));
    assert_eq!(name("fvoci_server::app::boards::tests"), Some("logic"));
    assert_eq!(
        name("fvoci_server::application"),
        Some("http"),
        "a claim ends at a whole name"
    );
    assert_eq!(name("fvoci_store::Store"), Some("store"));
    assert_eq!(name("axum::http::StatusCode"), Some("http"));
    assert_eq!(name("tokio::sync::Mutex"), None, "in no layer");
}

#[test]
fn http_code_that_names_the_store_is_found() {
    assert_eq!(
        broken("fvoci_server::api::planted", "use fvoci_store::Store;"),
        ["http>store fvoci_store::Store"]
    );
    assert_eq!(
        broken(
            "fvoci_server",
            "fn open() { let _ = fvoci_store::Store::connect(); }"
        ),
        ["http>store fvoci_store::Store::connect"],
        "the crate root is HTTP"
    );
}

#[test]
fn another_name_does_not_hide_an_import() {
    let module = "fvoci_server::api::planted";
    assert_eq!(
        broken(module, "use fvoci_store as database;"),
        ["http>store fvoci_store"]
    );
    assert_eq!(
        broken(module, "use fvoci_store::Store as Database;"),
        ["http>store fvoci_store::Store"]
    );
    assert_eq!(
        broken(module, "extern crate fvoci_store as database;"),
        ["http>store fvoci_store"]
    );
    assert_eq!(
        broken(
            module,
            "use fvoci_store::{self, boards::{Board as B, *}, Store as _};"
        ),
        [
            "http>store fvoci_store",
            "http>store fvoci_store::boards::Board",
            "http>store fvoci_store::boards::*",
            "http>store fvoci_store::Store",
        ]
    );
    assert_eq!(
        broken(module, "fn f() { use fvoci_store::Store as S; }"),
        ["http>store fvoci_store::Store"],
        "an import inside a function"
    );
}

#[test]
fn a_full_path_is_found_wherever_it_is_written() {
    let module = "fvoci_server::ws";
    let found = |source| broken(module, source);
    assert_eq!(
        found("async fn f() { fvoci_store::Store::connect().await; }"),
        ["http>store fvoci_store::Store::connect"],
        "an expression"
    );
    assert_eq!(
        found("fn f(store: &::fvoci_store::Store) {}"),
        ["http>store fvoci_store::Store"],
        "a type, from the root of all crates"
    );
    assert_eq!(
        found("struct S { all: Vec<Option<fvoci_store::Store>> }"),
        ["http>store fvoci_store::Store"],
        "inside generic arguments"
    );
    assert_eq!(
        found("fn f(e: E) -> bool { matches!(e, fvoci_store::StoreError::NotFound) }"),
        ["http>store fvoci_store::StoreError::NotFound"],
        "the input of a macro"
    );
    assert_eq!(
        found("fn f() { tokio::select! { x = ::fvoci_store::connect() => {} } }"),
        ["http>store fvoci_store::connect"],
        "the input of a macro, from the root of all crates"
    );
    assert_eq!(
        found(
            "#[utoipa::path(get, responses((status = 200, body = fvoci_store::Team)))] fn f() {}"
        ),
        ["http>store fvoci_store::Team"],
        "the arguments of an attribute"
    );
    assert_eq!(
        found(
            "impl From<fvoci_store::StoreError> for E { fn from(e: fvoci_store::StoreError) -> Self { E } }"
        ),
        [
            "http>store fvoci_store::StoreError",
            "http>store fvoci_store::StoreError"
        ],
        "a trait and a parameter"
    );
    assert_eq!(
        found("#[cfg(test)] mod tests { use fvoci_store::testing::memory; }"),
        ["http>store fvoci_store::testing::memory"],
        "a test module is checked like the code around it"
    );
    assert_eq!(
        found("macro_rules! open { () => { $crate::app::App::open(fvoci_store::Store::new()) }; }"),
        ["http>store fvoci_store::Store::new"],
        "the body of a macro definition"
    );
}

#[test]
fn a_handle_of_the_store_is_found() {
    let module = "fvoci_server::api::planted";
    let found = |source| broken(module, source);
    assert_eq!(
        found("async fn f(app: App) { app.store().get_block(1).await; }"),
        ["http>store .store().get_block"]
    );
    assert_eq!(
        found("async fn f(hub: Hub) { hub.inner.store.get_block(1).await; }"),
        ["http>store .store.get_block"],
        "a field"
    );
    assert_eq!(
        found("fn f(app: App) { let store = app.store(); pass(&app.inner.store); }"),
        ["http>store .store()", "http>store .store"],
        "taken without a call on it"
    );
    assert_eq!(
        found("async fn f(app: App) { tokio::join!(app.store().a(), other()); }"),
        ["http>store .store().a"],
        "inside a macro"
    );
    assert!(
        found("fn f(seen: AtomicBool, a: A) { seen.store(true, SeqCst); a.restore(); let store = 1; let _ = 0..store; }")
            .is_empty(),
        "a method with arguments, another name, a local and a range are not the handle"
    );
}

#[test]
fn a_name_given_elsewhere_does_not_hide_what_it_stands_for() {
    let logic = (
        "fvoci_server::app",
        "mod boards; pub use boards::*; pub use fvoci_store::Store as Database; \
         pub use fvoci_store::PublicBoards; pub type Shared = std::sync::Arc<Database>; \
         pub struct App;",
    );
    let boards = ("fvoci_server::app::boards", "pub use fvoci_store::Team;");
    let found = |source| {
        broken_in(
            &[logic, boards, ("fvoci_server::api::planted", source)],
            "fvoci_server::api::planted",
        )
    };
    assert_eq!(
        found("use crate::app::{App, Database, PublicBoards};"),
        [
            "http>store fvoci_store::Store",
            "http>store fvoci_store::PublicBoards"
        ],
        "a re-export, also under another name; the logic's own type is fine"
    );
    assert_eq!(
        found("fn f(shared: crate::app::Shared) {}"),
        ["http>store fvoci_store::Store"],
        "a type alias"
    );
    assert_eq!(
        found("use crate::app as logic; fn f() { logic::Database::connect(); }"),
        ["http>store fvoci_store::Store::connect"],
        "through a module that was given another name"
    );
    assert_eq!(
        found("use crate::app::Team;"),
        ["http>store fvoci_store::Team"],
        "through a module that re-exports everything of another"
    );
    assert_eq!(
        found(
            "use fvoci_store as database; fn f() { database::Store::connect(); database::open(); }"
        ),
        [
            "http>store fvoci_store",
            "http>store fvoci_store::Store::connect",
            "http>store fvoci_store::open"
        ],
        "an import and every path that starts at the name it gave"
    );
    assert!(
        broken_in(&[logic, boards], "fvoci_server::app").is_empty(),
        "the logic may give those names"
    );
}

#[test]
fn an_import_of_everything_of_another_layer_is_refused() {
    let sources = sources(&[
        (
            "fvoci_server::app::boards",
            "use fvoci_store::*;\nuse std::fmt::*;\nuse crate::app::*;\nmod tests { use super::*; }",
        ),
        ("fvoci_store::boards", "use fvoci_model::*;"),
    ]);
    assert_eq!(
        sources.unfollowed(),
        [
            "fvoci_server::app::boards.rs:1: logic imports everything of store: fvoci_store::*. \
             Name what is imported",
            "fvoci_store::boards.rs:1: store imports everything of model: fvoci_model::*. \
             Name what is imported",
        ],
        "allowed or not; a layer's own modules and what is in no layer are not refused"
    );
}

#[test]
fn logic_that_names_http_is_found() {
    let module = "fvoci_server::app::planted";
    let found = |source| broken(module, source);
    assert_eq!(
        found("use axum::http::StatusCode;"),
        ["logic>http axum::http::StatusCode"]
    );
    assert_eq!(
        found("fn f() -> http::StatusCode { http::StatusCode::OK }"),
        [
            "logic>http http::StatusCode",
            "logic>http http::StatusCode::OK"
        ]
    );
    assert_eq!(
        found("use crate::{web::WebRoot, ws::Hub, error::AppError};"),
        [
            "logic>http fvoci_server::web::WebRoot",
            "logic>http fvoci_server::ws::Hub"
        ],
        "modules of the server that are HTTP; the error type is logic"
    );
    assert_eq!(
        found("fn f() { super::super::api::created(); crate::listen_address(); }"),
        [
            "logic>http fvoci_server::api::created",
            "logic>http fvoci_server::listen_address"
        ],
        "through super, and an item of the crate root"
    );
    assert_eq!(
        found("mod tests { use super::super::super::request_log::Peer; }"),
        ["logic>http fvoci_server::request_log::Peer"],
        "super counts from the inline module"
    );
}

#[test]
fn what_the_rule_allows_is_not_reported() {
    assert!(
        broken(
            "fvoci_server::app::boards",
            "use fvoci_store::{Store, StoreError}; use fvoci_model::Board; use crate::auth::Authenticator; \
             pub(crate) fn f(app: &App) { app.store().get_board(); let http = 1; g(http); Self::h(); }"
        )
        .is_empty(),
        "logic uses the store, the model and itself; `pub(crate)` and a local are no path"
    );
    assert!(
        broken(
            "fvoci_server::api::boards",
            "use axum::Json; use fvoci_model::Board; use crate::app::App; use crate::error::AppError; \
             use std::sync::Arc; use tokio::sync::Mutex;"
        )
        .is_empty(),
        "http uses the logic, the model and what is in no layer"
    );
    assert!(
        broken(
            "fvoci_store::boards",
            "use fvoci_model::Board; use crate::db::Db; use sqlx::Row;"
        )
        .is_empty(),
        "the store uses the model"
    );
    assert_eq!(
        broken(
            "fvoci_store::boards",
            "use axum::Json; use fvoci_server::app::App;"
        ),
        [
            "store>http axum::Json",
            "store>logic fvoci_server::app::App"
        ]
    );
    assert_eq!(
        broken("fvoci_model::board", "use fvoci_store::Store;"),
        ["model>store fvoci_store::Store"]
    );
}

#[test]
fn paths_are_resolved_from_where_they_are_written() {
    assert_eq!(
        paths(
            "fvoci_server::auth",
            "mod token; use token::Origins; fn f() { token::read(); self::token::write(); }"
        ),
        [
            "fvoci_server::auth::token::Origins",
            "fvoci_server::auth::token::read",
            "fvoci_server::auth::token::write"
        ],
        "a module declared in the file"
    );
    assert_eq!(
        paths(
            "fvoci_server::auth",
            "mod token; mod tests { use super::*; fn f() { token::read(); } }"
        ),
        ["fvoci_server::auth::*", "fvoci_server::auth::token::read"],
        "a module declared around an inline module"
    );
    assert_eq!(
        paths("fvoci_server::main", "use crate::config::Configuration;"),
        ["fvoci_server::config::Configuration"],
        "the program's paths lead into its library"
    );
    assert_eq!(
        paths(
            "fvoci_server::api",
            "fn f() { <Vec<u8> as std::io::Write>::flush(); Vec::<a::B>::new(); x.y::<a::C>(); super::super::super::gone(); }"
        ),
        ["std::io::Write::flush", "Vec::new", "a::B", "a::C"],
        "a qualified path, generic arguments, and more `super` than there are modules"
    );
    assert_eq!(
        paths(
            "a",
            "fn f() { m!(<T as b::Trait>::name(), Vec::<u8>::new(), r#mod::x, .5); }"
        ),
        ["b::Trait", "mod::x"],
        "among the tokens of a macro, what follows `>::` goes on from before"
    );
}

#[test]
fn the_list_and_the_code_are_compared_both_ways() {
    let violation = |file: &str, line, named: &str| Violation {
        file: file.to_owned(),
        line,
        from: "http",
        to: "store",
        named: named.to_owned(),
    };
    let found = [
        violation("a.rs", 3, "fvoci_store::Store"),
        violation("a.rs", 9, "fvoci_store::Store"),
        violation("b.rs", 1, ".store().close"),
    ];
    let exact = parse_exceptions("# why\n\na.rs fvoci_store::Store 2\nb.rs  .store().close  1\n")
        .expect("the list reads");
    assert_eq!(
        judge(&found, &exact),
        Vec::<String>::new(),
        "exactly the listed uses"
    );

    let none = BTreeMap::new();
    let problems = judge(&found[2..], &none);
    assert_eq!(
        problems,
        ["b.rs:1: http names store: .store().close"],
        "a use that is not listed"
    );

    let problems = judge(&found[..1], &exact);
    assert_eq!(
        problems,
        [
            format!(
                "{EXCEPTIONS}: a.rs names fvoci_store::Store 1 times, listed 2: lower its count to 1"
            ),
            format!("{EXCEPTIONS}: b.rs names .store().close 0 times, listed 1: remove its line"),
        ],
        "listed uses that are gone"
    );

    let one = parse_exceptions("a.rs fvoci_store::Store 1\nb.rs .store().close 1\n")
        .expect("the list reads");
    let problems = judge(&found, &one);
    assert_eq!(
        problems,
        ["a.rs:3,9: http names store: fvoci_store::Store (2 times, listed 1)"],
        "one use more than listed"
    );
}

#[test]
fn a_list_that_cannot_be_read_is_refused() {
    for (text, why) in [
        ("a.rs fvoci_store::Store", "expected `file named count`"),
        ("a.rs fvoci_store::Store 1 2", "expected `file named count`"),
        ("a.rs fvoci_store::Store 0", "is no count"),
        ("a.rs fvoci_store::Store many", "is no count"),
        ("a.rs x 1\na.rs x 2", "listed twice"),
    ] {
        let error = parse_exceptions(text).expect_err("the list is refused");
        assert!(
            format!("{error:#}").contains(why),
            "{text:?} is refused for its form: {error:#}"
        );
    }
}

/// The modules of the crate `k` in `directory`, each with its file.
fn walked(directory: &Path) -> Result<Vec<String>> {
    let mut seen = Vec::new();
    walk_crate("k", directory, &mut |module_file| {
        let source = std::fs::read_to_string(&module_file.file)?;
        seen.push(format!(
            "{} {}",
            module_file.module.join("::"),
            shown_path(directory, &module_file.file)
        ));
        Ok(read_source(&module_file.module, &source, &[])?.declared)
    })?;
    seen.sort();
    Ok(seen)
}

#[test]
fn the_modules_of_a_crate_are_found_along_their_declarations() {
    let root = scratch("layers-walk");
    write(
        &root,
        "lib.rs",
        "mod a; mod b; #[path = \"elsewhere.rs\"] mod d; mod e { mod f; }",
    );
    write(&root, "main.rs", "");
    write(&root, "a.rs", "mod inner;");
    write(&root, "a/inner.rs", "");
    write(&root, "b/mod.rs", "mod c;");
    write(&root, "b/c.rs", "");
    write(&root, "elsewhere.rs", "");
    write(&root, "e/f.rs", "");

    let seen = walked(&root).expect("every file is a module");
    assert_eq!(
        seen,
        [
            "k lib.rs",
            "k::a a.rs",
            "k::a::inner a/inner.rs",
            "k::b b/mod.rs",
            "k::b::c b/c.rs",
            "k::d elsewhere.rs",
            "k::e::f e/f.rs",
            "k::main main.rs",
        ]
    );

    write(&root, "b/forgotten.rs", "use fvoci_store::Store;");
    let error = walked(&root).expect_err("a file that is no module");
    assert!(
        error.to_string().contains("forgotten.rs"),
        "a file the walk does not reach is not passed over: {error}"
    );

    std::fs::remove_file(root.join("b/forgotten.rs")).expect("the file is removed");
    std::fs::remove_file(root.join("b/c.rs")).expect("the file is removed");
    let error = walked(&root).expect_err("a module without a file");
    assert!(
        error.to_string().contains("k::b::c"),
        "a declared module must have a file: {error}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_planted_violation_is_found_in_a_workspace() {
    let root = scratch("layers-workspace");
    write(
        &root,
        "crates/server/src/lib.rs",
        "pub mod api; pub mod app;",
    );
    write(
        &root,
        "crates/server/src/api/mod.rs",
        "use crate::app::App;\nasync fn handler(app: App) {\n    app.store().purge().await;\n}\n",
    );
    write(
        &root,
        "crates/server/src/app.rs",
        "use fvoci_store::Store;\nuse axum::response::Response;\n",
    );
    write(&root, "crates/store/src/lib.rs", "use fvoci_model::Board;");
    write(&root, "crates/model/src/lib.rs", "");

    let found: Vec<String> = Sources::read(&root)
        .expect("the sources read")
        .violations()
        .iter()
        .map(|violation| format!("{}:{} {}", violation.file, violation.line, violation.named))
        .collect();
    assert_eq!(
        found,
        [
            "crates/server/src/api/mod.rs:3 .store().purge",
            "crates/server/src/app.rs:2 axum::response::Response",
        ]
    );
    let _ = std::fs::remove_dir_all(&root);
}
