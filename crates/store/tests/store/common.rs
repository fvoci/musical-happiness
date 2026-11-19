//! The test harness: a fresh database per test, on SQLite or PostgreSQL.

use std::path::PathBuf;

use fvoci_model::{Block, Container, ContainerType, User, Workspace, new_id};
use fvoci_store::testing::TestDatabase;
use fvoci_store::{CopyTitles, DbType, Purged, Store};
use sqlx::postgres::PgConnection;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{ConnectOptions as _, Connection as _};

/// The titles of copies, in English, as a caller of the store brings them.
pub const TITLES: CopyTitles = CopyTitles {
    copy_of: |title| format!("{title} copy"),
    new_template: "New board template",
};

/// A store on a database that belongs to one test, reaching the rows of the one
/// organization the test works in.
pub(crate) struct TestStore {
    pub(crate) store: Store,
    /// The organization of the test.
    pub(crate) org_id: String,
    /// The first workspace of that organization, its default one.
    pub(crate) workspace_id: String,
    database: TestDatabase,
}

impl std::ops::Deref for TestStore {
    type Target = Store;

    fn deref(&self) -> &Store {
        &self.store
    }
}

/// Which database the tests run against.
pub(crate) fn db_type() -> DbType {
    fvoci_store::testing::db_type_of_the_tests()
}

/// Connects a store to a fresh database.
pub(crate) async fn setup() -> TestStore {
    let directory = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("store-tests");
    let database = TestDatabase::create(&directory).await;
    let store = Store::connect(database.db_type(), database.database_connection())
        .await
        .expect("a store connects to a fresh database");
    let (org_id, workspace_id) = organization(&store, "").await;
    let store = store.for_org(&org_id).expect("an identifier");
    TestStore {
        store,
        org_id,
        workspace_id,
        database,
    }
}

/// Makes an organization with its first workspace and returns the identifiers of both.
pub(crate) async fn organization(store: &Store, name: &str) -> (String, String) {
    let workspace = Workspace {
        id: new_id(),
        ..Workspace::default()
    };
    let org_id = store
        .create_organization(name, &workspace)
        .await
        .expect("the organization is made");
    (org_id, workspace.id)
}

/// Creates a fresh database without connecting a store to it, for tests of what
/// [`Store::connect`] makes of a database that is not empty.
pub(crate) async fn empty_database() -> TestDatabase {
    let directory = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("store-tests");
    TestDatabase::create(&directory).await
}

/// What reaches a test database through a connection of the test's own: the path of the
/// SQLite file, or the PostgreSQL URL with the test's schema.
pub(crate) fn raw_connection(database: &TestDatabase) -> String {
    match database.sqlite_path() {
        Some(path) => path.display().to_string(),
        None => database.database_connection().to_owned(),
    }
}

/// A PostgreSQL connection of the test's own to its database, past the store and past
/// the wall between organizations: it reaches the rows of all, and a row it stores
/// without saying whose it is goes to the oldest organization, the one of the test.
pub(crate) async fn connect_past_the_store(database_connection: &str) -> PgConnection {
    let (url, schema) = database_connection
        .rsplit_once("search_path=")
        .expect("the test database connection names its schema");
    let mut conn = PgConnection::connect(url.trim_end_matches(['?', '&']))
        .await
        .expect("the test PostgreSQL server accepts connections");
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("SET search_path TO {schema}")))
        .execute(&mut conn)
        .await
        .expect("the schema is selected");
    // A database that is not migrated yet has no organizations, and no wall.
    let has_organizations: bool =
        sqlx::query_scalar("SELECT to_regclass('organizations') IS NOT NULL")
            .fetch_one(&mut conn)
            .await
            .expect("the tables are looked up");
    if has_organizations {
        // The system's reach first: without it the organizations are out of reach too.
        sqlx::raw_sql(
            "SELECT set_config('fvoci.system', 'on', false); \
             SELECT set_config('fvoci.org', \
             COALESCE((SELECT id::text FROM organizations WHERE NOT is_system \
             ORDER BY create_at, id LIMIT 1), ''), false)",
        )
        .execute(&mut conn)
        .await
        .expect("the connection is let past the wall");
    }
    conn
}

/// Runs statements on a connection of its own, past the store.
pub(crate) async fn execute_raw(database_connection: &str, sql: &str) {
    let sql = sqlx::AssertSqlSafe(sql.to_owned());
    match db_type() {
        DbType::Sqlite => {
            let options = SqliteConnectOptions::new()
                .filename(database_connection)
                .create_if_missing(true);
            let mut conn = options
                .connect()
                .await
                .expect("the test database file opens");
            sqlx::raw_sql(sql)
                .execute(&mut conn)
                .await
                .expect("the statement runs");
            conn.close().await.expect("the connection closes");
        }
        DbType::Postgres => {
            let mut conn = connect_past_the_store(database_connection).await;
            sqlx::raw_sql(sql)
                .execute(&mut conn)
                .await
                .expect("the statement runs");
            conn.close().await.expect("the connection closes");
        }
    }
}

impl TestStore {
    /// What reaches this test's database again: the path of the SQLite file, or the
    /// PostgreSQL URL with the test's schema.
    pub(crate) fn database_connection(&self) -> String {
        raw_connection(&self.database)
    }

    /// A second store on this test's database, as the first is: connected anew (and so
    /// migrating what the test put back to an older state) and reaching the rows of the
    /// test's organization.
    pub(crate) async fn reconnect(&self) -> Store {
        Store::connect(db_type(), &self.database_connection())
            .await
            .expect("the store connects to the database again")
            .for_org(&self.org_id)
            .expect("an identifier")
    }

    /// Closes the store and removes its database.
    pub(crate) async fn teardown(self) {
        self.store.close().await;
        self.database.remove().await;
    }
}

/// Stores a user with the given name and returns their identifier. The email address
/// is derived from the name; both must be unique within a test's database.
pub(crate) async fn user(store: &Store, username: &str) -> String {
    let mut user = User {
        id: new_id(),
        username: username.to_owned(),
        email: format!("{username}@example.com"),
        ..User::default()
    };
    store
        .create_user(&mut user)
        .await
        .expect("the user is stored");
    user.id
}

/// Stores a workspace and returns its identifier.
pub(crate) async fn workspace(store: &Store) -> String {
    let workspace = Workspace {
        id: new_id(),
        ..Workspace::default()
    };
    store
        .create_workspace(&workspace)
        .await
        .expect("the workspace is stored");
    workspace.id
}

/// Stores a workspace that the users work in, as members of the workspace: its open boards
/// are theirs to find. Returns its identifier.
pub(crate) async fn workspace_of(store: &Store, users: &[&str]) -> String {
    let workspace_id = workspace(store).await;
    for user_id in users {
        store
            .add_workspace_member(&workspace_id, user_id, fvoci_model::WorkspaceRole::Member)
            .await
            .expect("the user is let into the workspace");
    }
    workspace_id
}

/// Lets the user into the workspace as a member, unless they are in it: whom a board is
/// stored for as its admin works in its workspace.
pub(crate) async fn works_in(store: &Store, workspace_id: &str, user_id: &str) {
    let found = store
        .texts_unchecked(&format!(
            "SELECT role FROM workspace_members \
             WHERE workspace_id = '{workspace_id}' AND user_id = '{user_id}'"
        ))
        .await
        .expect("the membership is looked for");
    if found.is_empty() {
        store
            .add_workspace_member(workspace_id, user_id, fvoci_model::WorkspaceRole::Member)
            .await
            .expect("the user is let into the workspace");
    }
}

/// `storetests.InsertBlocks`: inserts each block on its own.
pub(crate) async fn insert_blocks(store: &Store, blocks: &mut [Block], user_id: &str) {
    for block in blocks {
        store
            .insert_block(block, user_id)
            .await
            .expect("the block is inserted");
    }
}

/// `storetests.DeleteBlocks`: deletes each block on its own.
pub(crate) async fn delete_blocks(store: &Store, blocks: &[Block], modified_by: &str) {
    for block in blocks {
        store
            .delete_block(&block.id, modified_by)
            .await
            .expect("the block is deleted");
    }
}

/// `storetests.ContainsBlockWithID`.
pub(crate) fn contains_block_with_id(blocks: &[Block], block_id: &str) -> bool {
    blocks.iter().any(|block| block.id == block_id)
}

/// The identifiers of the blocks, sorted.
pub(crate) fn sorted_ids(blocks: &[Block]) -> Vec<String> {
    let mut ids: Vec<String> = blocks.iter().map(|block| block.id.clone()).collect();
    ids.sort();
    ids
}

/// `storetests.createTestBoards`: inserts `count` open boards of a workspace.
pub(crate) async fn create_test_boards(
    store: &Store,
    workspace_id: &str,
    user_id: &str,
    count: usize,
) -> Vec<Container> {
    let mut boards = Vec::with_capacity(count);
    for index in 0..count {
        let mut board = Container {
            id: new_id(),
            workspace_id: workspace_id.to_owned(),
            container_type: ContainerType::Open,
            created_by: user_id.to_owned(),
            title: format!("board {index}"),
            ..Container::default()
        };
        store
            .insert_container(&mut board, user_id)
            .await
            .expect("the board is inserted");
        boards.push(board);
    }
    boards
}

/// What a purge of the trash answers when that many boards and blocks went for good and
/// no board among them had a file.
pub(crate) fn purged_without_files(records: u64) -> Purged {
    Purged {
        records,
        files: Vec::new(),
    }
}

/// Whether the database refused a statement over a foreign key: a row that refers to
/// what does not exist, or the removal of a row that another one still refers to.
pub(crate) fn is_foreign_key_violation(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .is_some_and(sqlx::error::DatabaseError::is_foreign_key_violation)
}

/// Whether a store operation failed because the database refused it over a foreign key.
pub(crate) fn refused_over_a_foreign_key(error: &fvoci_store::StoreError) -> bool {
    matches!(error, fvoci_store::StoreError::Database(error) if is_foreign_key_violation(error))
}

/// Runs statements on a connection of its own, past the store, as [`execute_raw`] does,
/// and returns the database's answer where `execute_raw` expects success: how many rows
/// the statements changed, or the refusal.
pub(crate) async fn try_execute_raw(
    database_connection: &str,
    sql: &str,
) -> Result<u64, sqlx::Error> {
    let sql = sqlx::AssertSqlSafe(sql.to_owned());
    match db_type() {
        DbType::Sqlite => {
            // Foreign keys are enforced per connection on SQLite: this one asks for
            // them, as the store's connections do.
            let options = SqliteConnectOptions::new()
                .filename(database_connection)
                .foreign_keys(true);
            let mut conn = options
                .connect()
                .await
                .expect("the test database file opens");
            let result = sqlx::raw_sql(sql).execute(&mut conn).await;
            conn.close().await.expect("the connection closes");
            result.map(|done| done.rows_affected())
        }
        DbType::Postgres => {
            let mut conn = connect_past_the_store(database_connection).await;
            let result = sqlx::raw_sql(sql).execute(&mut conn).await;
            conn.close().await.expect("the connection closes");
            result.map(|done| done.rows_affected())
        }
    }
}

/// The organization's provider of sign-ins (one, whoever sets it), the one it moved
/// away from, its pending settings, a sign-in through it that is held, and one that has
/// not come back.
pub(crate) async fn sign_in_under_way(store: &Store, name: &str) {
    let settings = |issuer: &str| fvoci_store::StoredProvider {
        id: String::new(),
        org_id: String::new(),
        issuer: issuer.to_owned(),
        client_id: "fvoci".to_owned(),
        sealed_client_secret: String::new(),
        auto_join: true,
        enabled: true,
        create_at: 0,
        update_at: 0,
        sso_required: true,
        revision: 0,
    };
    // Moved once: a second move would end the sign-ins under way through the first.
    let provider = if let Some(provider) = store.oidc_provider().await.expect("read") {
        provider
    } else {
        let former = store
            .put_oidc_provider(&settings("https://former.example.com"))
            .await
            .expect("a provider is stored");
        store
            .put_oidc_provider_as_read(
                &settings("https://idp.example.com"),
                Some(&former),
                &fvoci_model::new_id(),
                std::time::Duration::from_hours(24),
            )
            .await
            .expect("asked")
            .expect("another provider is stored, and the one before kept")
    };
    store
        .put_oidc_pending_provider(
            &fvoci_store::PendingSettings {
                issuer: "https://idp.example.com".to_owned(),
                client_id: format!("fvoci of {name}"),
                sealed_client_secret: String::new(),
            },
            &provider,
            &provider,
            &fvoci_model::new_id(),
        )
        .await
        .expect("asked")
        .expect("pending settings are stored");
    let is_held = store
        .hold_oidc_sign_in(&fvoci_store::HeldSignIn {
            held_hash: format!("held for {name}"),
            org_id: provider.org_id.clone(),
            provider_id: provider.id.clone(),
            revision: provider.revision,
            issuer: provider.issuer.clone(),
            subject: name.to_owned(),
            email: String::new(),
            email_unverified: false,
            name: String::new(),
            preferred_username: String::new(),
            create_at: 0,
            expires_at: i64::MAX,
        })
        .await
        .expect("asked");
    assert!(is_held, "a sign-in is held while a former provider is kept");
    store
        .create_oidc_login_flow(&fvoci_store::LoginFlow {
            state_hash: format!("state of {name}"),
            browser_hash: format!("browser of {name}"),
            nonce: String::new(),
            pkce_verifier: String::new(),
            org_id: provider.org_id.clone(),
            provider_id: provider.id,
            create_at: 0,
            expires_at: i64::MAX,
            purpose: fvoci_store::FlowPurpose::SignIn,
        })
        .await
        .expect("a sign-in is stored");
}
