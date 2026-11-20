//! The files that may reach past the wall between organizations: the sources are read,
//! and those that ask for the system's reach, name an organization for a handle, or
//! connect as a superuser are the few listed here.

use std::path::Path;

/// The Rust files below a directory.
fn rust_sources(directory: &Path, found: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(directory).expect("a source directory") {
        let path = entry.expect("an entry").path();
        if path.is_dir() {
            rust_sources(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}

/// The files that may ask for the system's reach, by which a handle reaches the rows of
/// every organization: the store itself, the directory (and the two reads of it that a
/// transaction of one organization makes of all, each named for what it asks:
/// `directory/every_organization.rs`), the records of the server's
/// operator in each organization of a person, the deleting of a person's account (whose
/// memberships are of several organizations), the changes of the data after a
/// migration, and the one function of the server that its housekeeping works through;
/// and the revisions of the cached checks, which name it to tell what its writes move.
/// Asked by the handle or by the setting of a transaction alike. Anywhere else, a
/// request would be let past the wall it is there to be held by. (The system
/// organization, which holds the built-in templates, is an
/// organization like another to the wall: `Tenant::org(SYSTEM_ORG_ID)` is not this.)
#[test]
fn the_systems_reach_is_asked_for_in_few_places() {
    const ALLOWED: [&str; 11] = [
        "store/src/lib.rs",
        "store/src/account_deletion.rs",
        "store/src/account_erasure.rs",
        "store/src/directory.rs",
        "store/src/directory/every_organization.rs",
        "store/src/directory/sign_in.rs",
        "store/src/operator_records.rs",
        "store/src/signups.rs",
        "store/src/data_steps.rs",
        "store/src/visibility.rs",
        "server/src/app/housekeeping.rs",
    ];
    let crates = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crates directory");
    let mut files = Vec::new();
    for source in ["store/src", "server/src"] {
        rust_sources(&crates.join(source), &mut files);
    }
    assert!(files.len() > 50, "the sources are found: {}", files.len());
    let mut asking = Vec::new();
    for file in files {
        let name = file
            .strip_prefix(crates)
            .expect("under the crates directory")
            .to_string_lossy()
            .replace('\\', "/");
        let text = std::fs::read_to_string(&file).expect("a source file");
        // A file's unit tests, in its `mod tests` at the end, are compiled into no
        // server; an item that is `#[cfg(test)]` alone among the rest is still read.
        let asks = text
            .lines()
            .take_while(|line| !line.trim_start().starts_with("mod tests"))
            .filter(|line| !line.trim_start().starts_with("//"))
            .any(|line| {
                // By the variant, also where it is imported by its bare name, and by
                // the method, however it is called.
                line.contains("Tenant::System")
                    || (line.contains("Tenant::{") && line.contains("System"))
                    || line.contains(".system()")
                    || line.contains("::system(")
                    || line.contains("fvoci.system")
            });
        // The test support is compiled into no server.
        if asks && name != "store/src/testing.rs" {
            asking.push(name);
        }
    }
    asking.sort();
    let mut allowed = ALLOWED.map(str::to_owned).to_vec();
    allowed.sort();
    assert_eq!(asking, allowed, "the files that ask for the system's reach");
}

/// A transaction's wall is moved to another organization in one place: where an
/// account's own space is stored with the account, under an identifier made there.
/// Anywhere else, moving it would put rows into an organization the request did not
/// name, or read them from it.
#[test]
fn the_wall_of_a_transaction_is_moved_in_one_place() {
    let store = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_sources(&store, &mut files);
    let mut moving = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(&file).expect("a source file");
        let moves = text
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .any(|line| line.contains("in_new_organization(") || line.contains("ORG_SETTING"));
        if moves {
            let name = file.strip_prefix(&store).expect("under the sources");
            moving.push(name.to_string_lossy().replace('\\', "/"));
        }
    }
    moving.sort();
    // The setting is named where a transaction begins (`lib.rs`) and where it is
    // moved (`db.rs`); `accounts.rs` is the caller.
    assert_eq!(moving, ["accounts.rs", "db.rs", "lib.rs"]);
}

/// The files of the server that say whose rows a handle reaches: the way into a
/// workspace, the templates (as the system organization), the start (the one
/// organization of the routes that name no workspace), the housekeeping, the
/// application's own `scoped`, the erasing of an account (each organization the store
/// says it has something in), the asking to delete one (the account's own space), and the documents of bodies edited together, which the
/// server writes to their cards as the organization of each when it starts and stops. All of them are the logic's; no handler names one. A handler that named an organization by what a request
/// says, past the extractor, would open the wall from the inside.
#[test]
fn the_server_names_an_organization_for_a_handle_in_few_places() {
    const NAMING: &str = ".scoped( for_org( Tenant::org( Tenant::Org( .in_organization(";
    const ALLOWED: [&str; 7] = [
        // The space of the account that asks what goes with it: named by its owner.
        "app/account_deletion.rs",
        "app/account_erasure.rs",
        "app/housekeeping.rs",
        "app/mod.rs",
        "app/templates.rs",
        "app/workspaces.rs",
        "collab/mod.rs",
    ];
    let server = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crates directory")
        .join("server/src");
    let mut files = Vec::new();
    rust_sources(&server, &mut files);
    let mut naming = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(&file).expect("a source file");
        let names = text
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .any(|line| NAMING.split(' ').any(|naming| line.contains(naming)));
        if names {
            naming.push(
                file.strip_prefix(&server)
                    .expect("under the server's sources")
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
    }
    naming.sort();
    assert_eq!(
        naming, ALLOWED,
        "the files that name an organization for a handle"
    );
}

/// The tests that connect to PostgreSQL as a superuser, whom no policy holds: those of
/// what the server and the store make of such a role, and nothing else. A test that
/// took that connection for convenience would show nothing of the wall.
#[test]
fn few_tests_connect_as_a_superuser() {
    // The last is this file, which names what it looks for.
    const ALLOWED: [&str; 7] = [
        "server/tests/api/common.rs",
        "server/tests/api/organizations.rs",
        "server/tests/api/table_owner.rs",
        "store/src/testing.rs",
        "store/tests/store/table_owner.rs",
        "store/tests/store/tenancy_sources.rs",
        "store/tests/store/tenancy_tables.rs",
    ];
    let crates = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crates directory");
    let mut files = Vec::new();
    rust_sources(crates, &mut files);
    let mut connecting: Vec<String> = files
        .iter()
        .filter(|file| {
            let text = std::fs::read_to_string(file).expect("a source file");
            [
                "create_as_superuser",
                "create_as_role_with_bypassrls",
                "create_owned_apart",
                "connecting_as_superuser",
                "SUPERUSER_CONN_STRING_ENV",
            ]
            .iter()
            .any(|name| text.contains(name))
        })
        .map(|file| {
            file.strip_prefix(crates)
                .expect("under the crates directory")
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    connecting.sort();
    assert_eq!(connecting, ALLOWED, "the files that connect as a superuser");
}

/// What leaves the server for an address its operator chose, the webhooks of
/// `block_change_webhooks`, is posted from one place in the server
/// (`App::tell_webhooks`), which posts nothing of an account's own space. A second
/// place that posted would post what the first holds back.
#[test]
fn the_operators_webhooks_are_posted_from_one_place() {
    let server = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crates directory")
        .join("server/src");
    let mut files = Vec::new();
    rust_sources(&server, &mut files);
    let mut posting = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(&file).expect("a source file");
        let name = file
            .strip_prefix(&server)
            .expect("under the server's sources")
            .to_string_lossy()
            .replace('\\', "/");
        for line in text.lines() {
            let code = line.trim_start();
            if code.starts_with("//") || code.starts_with("pub async fn notify_update(") {
                continue;
            }
            if code.contains("notify_update(") || code.contains(".webhook") {
                posting.push(name.clone());
            }
        }
    }
    assert_eq!(
        posting,
        ["app/webhooks.rs"],
        "the lines that reach the webhook client"
    );
}
