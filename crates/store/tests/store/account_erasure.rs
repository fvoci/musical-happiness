//! Erasing an account: nothing of the person is left, what they wrote and the records
//! about them stay, nothing is left without somebody to run it, an organization that
//! has nobody else goes with it, and an erasing that was cut off is taken up again.

use fvoci_model::{
    AuditAction, Block, BlockType, Category, Container, ContainerType, FileInfo, OrgRole, Session,
    User, WorkspaceRole, new_id,
};
use fvoci_store::{
    AuditPlace, DbType, DeletionAsking, ErasedFrom, Erasing, ErasureEnd, ErasureStart, Identity,
    Looking, NewLink, Paging, Reach, Sight, Store, StoreError,
};

use crate::common::{self, TestStore};

/// A time in milliseconds that the tests count from.
const T0: i64 = 1_800_000_000_000;

/// No column, and nobody.
const NOBODY: [String; 0] = [];

/// Every record, on one page.
const EVERY: Paging = Paging {
    page: 0,
    per_page: 0,
};

/// The wait the tests ask for.
const WAIT: i64 = 30 * 24 * 60 * 60 * 1000;

/// What is the person's of the account [`person`] makes: none of it is anywhere once
/// the account is erased.
const OF_THE_PERSON: [&str; 9] = [
    "leaver-shelf",
    "nder-leaver@example.com",
    "leaver-name",
    "leaver-mail@example.com",
    "Leaver Shown",
    "leaver-password-hash",
    "leaver-subject",
    "leaver-at-the-provider@example.com",
    "leaver-browser",
];

/// Where the identifier of the account [`person`] makes stays once it is erased, with
/// what the first test writes as it: the row itself, what it made and changed, and the
/// records about it.
const KEEPS_THE_IDENTIFIER: [&str; 16] = [
    "audit_log.actor_id",
    "audit_log.subject_id",
    "blocks.created_by",
    "blocks.fields",
    "blocks.modified_by",
    "body_version_editors.user_id",
    "comment_versions.written_by",
    "containers.created_by",
    "containers.modified_by",
    "deleted_blocks.block",
    "deleted_blocks.deleted_by",
    "deleted_containers.container",
    "deleted_containers.deleted_by",
    "invitations.created_by",
    "uploads.user_id",
    "users.id",
];

/// Gives a member of the handle's organization the role in it, past the routes.
async fn set_org_role(store: &Store, user_id: &str, role: OrgRole) {
    store
        .execute_unchecked(&format!(
            "UPDATE organization_members SET role = '{}' WHERE user_id = '{user_id}'",
            role.as_str()
        ))
        .await
        .expect("the role is set");
}

/// The first user of the instance, who administers it and the test's organization.
async fn founder(test: &TestStore) -> String {
    common::user(test, "founder").await
}

/// An account with everything a person leaves in the tables that are not an
/// organization's: a name, an address, a shown name, a password hash, an identity at a
/// provider, a session, a setting and a credential to set the password anew. A member
/// of the handle's organization and its default workspace.
async fn person(store: &Store) -> String {
    let mut user = User {
        id: new_id(),
        username: "leaver-name".to_owned(),
        email: "leaver-mail@example.com".to_owned(),
        display_name: "Leaver Shown".to_owned(),
        password_hash: "leaver-password-hash".to_owned(),
        ..User::default()
    };
    store.create_user(&mut user).await.expect("stored");
    let identity = Identity {
        issuer: "https://provider.example",
        subject: "leaver-subject",
    };
    store
        .add_identity(&user.id, identity, "leaver-at-the-provider@example.com")
        .await
        .expect("tied");
    // An address with a capital that is no ASCII letter: the databases fold such a
    // letter each in its own way, and the address is found as it is written all the same.
    let other = Identity {
        issuer: "https://other.example",
        subject: "leaver-subject",
    };
    store
        .add_identity(&user.id, other, "Ünder-leaver@example.com")
        .await
        .expect("tied");
    let session = Session {
        id: new_id(),
        user_id: user.id.clone(),
        created_at: T0,
        last_used_at: T0,
        expires_at: T0 + 2 * WAIT,
        user_agent: "leaver-browser".to_owned(),
        login: None,
    };
    assert!(
        store
            .create_session(&session, "a session's digest", "leaver-password-hash")
            .await
            .expect("stored"),
        "the session is stored"
    );
    assert!(
        store
            .create_password_reset(&user.id, "a credential's digest", T0)
            .await
            .expect("stored"),
        "the credential is stored"
    );
    store
        .execute_unchecked(&format!(
            "INSERT INTO preferences (user_id, category, name, value) \
             VALUES ('{}', 'client', 'seen', 'yes')",
            user.id
        ))
        .await
        .expect("stored");
    user.id
}

/// A private board of the workspace whose admin is `user_id`, with a card they wrote.
async fn board_of(store: &Store, workspace_id: &str, user_id: &str) -> (String, String) {
    let mut board = Container {
        id: new_id(),
        workspace_id: workspace_id.to_owned(),
        container_type: ContainerType::Private,
        title: "a board".to_owned(),
        ..Container::default()
    };
    store
        .insert_container_with_admin(&mut board, user_id)
        .await
        .expect("the board is stored");
    let mut card = Block {
        id: new_id(),
        container_id: board.id.clone(),
        block_type: BlockType::Card,
        title: "a card".to_owned(),
        ..Block::default()
    };
    store
        .insert_block(&mut card, user_id)
        .await
        .expect("the card is stored");
    (board.id, card.id)
}

/// A link into the workspace made by `user_id`, under the digest.
async fn link_by(store: &Store, workspace_id: &str, user_id: &str, digest: &str) {
    let link = NewLink {
        workspace_id,
        guest_of: None,
        token_hash: digest,
        created_by: user_id,
        now: T0,
        expires_at: T0 + WAIT,
        max_uses: None,
        mailed: None,
    };
    store
        .create_invitation(&link)
        .await
        .expect("the link is made");
}

/// Erases the account through every step, on the operator's word, and returns what
/// became of it in each organization.
async fn erase(test: &TestStore, user_id: &str, erasing: Erasing, now: i64) -> Vec<ErasedFrom> {
    let begun = test
        .begin_account_erasure(user_id, now, erasing)
        .await
        .expect("begun");
    assert_eq!(begun, ErasureStart::Begun, "the erasing begins");
    let erased = erase_from_organizations(test, user_id, erasing).await;
    assert_eq!(
        test.finish_account_erasure(user_id, now)
            .await
            .expect("finished"),
        ErasureEnd::Erased,
        "the erasing ends"
    );
    erased
}

async fn erase_from_organizations(
    test: &TestStore,
    user_id: &str,
    erasing: Erasing,
) -> Vec<ErasedFrom> {
    let mut erased = Vec::new();
    for org_id in test
        .organizations_of_account(user_id)
        .await
        .expect("listed")
    {
        let store = test.for_org(&org_id).expect("an organization");
        erased.push(
            store
                .erase_account_from_organization(user_id, erasing)
                .await
                .expect("erased from the organization"),
        );
    }
    erased
}

/// A column of the schema, as the database's own catalog lists it.
struct Column {
    table: String,
    name: String,
    /// Whether it holds bytes, which PostgreSQL shows as hexadecimal unless told.
    bytes: bool,
}

/// Every column of every table of the schema, read from the database's own catalog:
/// a table or a column that is added later is swept with the others.
async fn columns(system: &Store) -> Vec<Column> {
    let listed = match common::db_type() {
        DbType::Sqlite => {
            system
                .texts_unchecked(
                    "SELECT m.name || '.' || c.name || '.' || lower(c.type) \
                     FROM sqlite_master AS m JOIN pragma_table_info(m.name) AS c \
                     WHERE m.type = 'table' AND m.name NOT LIKE 'sqlite_%'",
                )
                .await
        }
        DbType::Postgres => {
            system
                .texts_unchecked(
                    "SELECT table_name || '.' || column_name || '.' || data_type \
                     FROM information_schema.columns WHERE table_schema = current_schema()",
                )
                .await
        }
    }
    .expect("the catalog is read");
    assert!(
        listed.len() > 200,
        "the columns are found: {}",
        listed.len()
    );
    listed
        .iter()
        .map(|name| {
            let mut parts = name.splitn(3, '.');
            let mut part = || {
                parts
                    .next()
                    .expect("a table, a column and a type")
                    .to_owned()
            };
            let (table, name, kind) = (part(), part(), part());
            Column {
                table,
                name,
                bytes: kind == "bytea" || kind == "blob",
            }
        })
        .collect()
}

/// The columns that hold `needle` in some row, of any organization, in any letter
/// case: as text, a value of any type, JSON included; in a column of bytes, the bytes
/// of the needle as it is written and in small letters, whatever bytes are around
/// them (SQLite reads bytes as text up to the first zero byte only).
async fn holding(system: &Store, needle: &str) -> Vec<String> {
    let small = needle.to_lowercase();
    let mut found = Vec::new();
    for column in columns(system).await {
        let (table, name) = (&column.table, &column.name);
        let holds = match (common::db_type(), column.bytes) {
            (DbType::Postgres, true) => {
                format!("lower(encode(\"{name}\", 'escape')) LIKE '%{small}%'")
            }
            (DbType::Postgres, false) => {
                format!("lower(CAST(\"{name}\" AS TEXT)) LIKE '%{small}%'")
            }
            (DbType::Sqlite, true) => format!(
                "instr(hex(\"{name}\"), hex('{needle}')) > 0 \
                 OR instr(hex(\"{name}\"), hex('{small}')) > 0"
            ),
            (DbType::Sqlite, false) => {
                format!("instr(lower(CAST(\"{name}\" AS TEXT)), '{small}') > 0")
            }
        };
        let count = system
            .texts_unchecked(&format!(
                "SELECT CAST(count(*) AS TEXT) FROM \"{table}\" WHERE {holds}"
            ))
            .await
            .unwrap_or_else(|error| panic!("{table}.{name} is searched: {error}"));
        if count != ["0"] {
            found.push(format!("{table}.{name}"));
        }
    }
    found.sort();
    found
}

/// How many rows each table holds, of every organization.
async fn rows_of_every_table(system: &Store) -> std::collections::BTreeMap<String, i64> {
    let mut rows = std::collections::BTreeMap::new();
    for column in columns(system).await {
        if rows.contains_key(&column.table) {
            continue;
        }
        let query = format!("SELECT CAST(count(*) AS TEXT) FROM \"{}\"", column.table);
        let counted = count(system, &query).await;
        rows.insert(column.table, counted);
    }
    rows
}

/// What somebody leaves on a card of theirs besides the card: a comment with a version
/// they wrote, a kept version of the body they edited, and that they follow the card.
async fn work_on(store: &Store, board: &str, card: &str, user_id: &str) {
    let mut comment = Block {
        id: new_id(),
        container_id: board.to_owned(),
        parent_id: card.to_owned(),
        block_type: BlockType::Comment,
        ..Block::default()
    };
    store
        .insert_block(&mut comment, user_id)
        .await
        .expect("the comment is stored");
    let digest = match common::db_type() {
        DbType::Postgres => "'\\x00'::bytea",
        DbType::Sqlite => "x'00'",
    };
    for statement in [
        format!(
            "INSERT INTO comment_versions (block_id, version, files, written_by, written_at) \
             VALUES ('{}', 1, '[]', '{user_id}', {T0})",
            comment.id
        ),
        format!(
            "INSERT INTO body_versions (block_id, version, body_digest, files, kept_at) \
             VALUES ('{card}', 1, {digest}, '[]', {T0})"
        ),
        format!(
            "INSERT INTO body_version_editors (block_id, version, user_id) \
             VALUES ('{card}', 1, '{user_id}')"
        ),
        format!(
            "INSERT INTO subscriptions (block_type, block_id, subscriber_type, subscriber_id, \
             create_at) VALUES ('card', '{card}', 'user', '{user_id}', {T0})"
        ),
    ] {
        store
            .execute_unchecked(&statement)
            .await
            .unwrap_or_else(|error| panic!("{statement}: {error}"));
    }
}

/// How many rows of each table that names its organization (`org_id`) are the
/// organization's.
async fn rows_of(system: &Store, org_id: &str) -> Vec<(String, i64)> {
    let mut rows = Vec::new();
    for column in columns(system).await {
        if column.name != "org_id" {
            continue;
        }
        let query = format!(
            "SELECT CAST(count(*) AS TEXT) FROM \"{}\" WHERE CAST(org_id AS TEXT) = '{org_id}'",
            column.table
        );
        rows.push((column.table, count(system, &query).await));
    }
    rows.sort();
    assert!(
        rows.len() > 10,
        "the tables of an organization are found: {rows:?}"
    );
    rows
}

async fn count(store: &Store, query: &str) -> i64 {
    store.texts_unchecked(query).await.expect("counted")[0]
        .parse()
        .expect("a number")
}

/// What a person leaves behind besides what [`person`] stores, in the test's
/// organization: links they made and links mailed to them by `founder`, used and not;
/// a sign-in of theirs held at a provider, and a record that says who their identity
/// is; a board of theirs in the trash; an upload they began on `board`; and what the
/// operator did about their account.
async fn leave_traces(test: &TestStore, founder: &str, leaver: &str, board: &str) {
    link_by(test, &test.workspace_id, leaver, "the leaver's unused link").await;
    // Links mailed to the person's addresses by somebody else: one unused, one used.
    link_by(test, &test.workspace_id, founder, "mailed, unused").await;
    link_by(test, &test.workspace_id, founder, "mailed, used").await;
    link_by(
        test,
        &test.workspace_id,
        founder,
        "mailed to the other address",
    )
    .await;
    // A sign-in through a provider held for somebody of their address to say that the
    // account is theirs (it is tied to no account), and a record that says who their
    // identity is.
    let provider_id = new_id();
    for statement in [
        "UPDATE invitations SET recipient = 'Leaver-Mail@example.com' \
         WHERE token_hash = 'mailed, unused'"
            .to_owned(),
        "UPDATE invitations SET recipient = 'leaver-at-the-provider@example.com', uses = 1 \
         WHERE token_hash = 'mailed, used'"
            .to_owned(),
        "UPDATE invitations SET recipient = 'Ünder-leaver@example.com' \
         WHERE token_hash = 'mailed to the other address'"
            .to_owned(),
        format!(
            "INSERT INTO oidc_providers (id, org_id, issuer, client_id, client_secret, \
             auto_join, enabled, create_at, update_at) VALUES ('{provider_id}', '{org}', \
             'https://provider.example', 'client', '', true, true, {T0}, {T0})",
            org = test.org_id
        ),
        format!(
            "INSERT INTO oidc_held_sign_ins (held_hash, org_id, provider_id, revision, issuer, \
             subject, email, email_unverified, name, preferred_username, create_at, expires_at) \
             VALUES ('held', '{org}', '{provider_id}', 1, 'https://provider.example', \
             'somebody at the provider', 'LEAVER-MAIL@example.com', false, 'Leaver Shown', \
             'leaver-name', {T0}, {later})",
            org = test.org_id,
            later = T0 + WAIT
        ),
        format!(
            "INSERT INTO audit_log (id, org_id, actor_id, actor_kind, action, target_id, \
             subject_id, create_at, detail, identity) VALUES ('{id}', '{org}', '{leaver}', \
             'person', 'oidc_identity_bound', '{org}', '{leaver}', {T0}, \
             'https://provider.example', 'leaver-at-the-provider@example.com')",
            id = new_id(),
            org = test.org_id
        ),
    ] {
        test.execute_unchecked(&statement)
            .await
            .unwrap_or_else(|error| panic!("{statement}: {error}"));
    }
    // A link of theirs that somebody used, a board of theirs in the trash with a card,
    // and an upload they had begun: what stays names them by identifier alone.
    link_by(test, &test.workspace_id, leaver, "the leaver's used link").await;
    test.execute_unchecked(
        "UPDATE invitations SET uses = 1 WHERE token_hash = 'the leaver''s used link'",
    )
    .await
    .expect("used");
    // Links of theirs that others were made like, for the recipients of their mail:
    // one whose copy was used, one whose copy was not.
    for (like, copy, uses) in [
        ("made like, the copy used", "a copy that was used", 1),
        ("made like, the copy unused", "a copy nobody used", 0),
    ] {
        link_by(test, &test.workspace_id, leaver, like).await;
        link_by(test, &test.workspace_id, leaver, copy).await;
        let statement = format!(
            "UPDATE invitations SET uses = {uses}, recipient = 'somebody@example.com', \
             parent_id = (SELECT id FROM invitations WHERE token_hash = '{like}') \
             WHERE token_hash = '{copy}'"
        );
        test.execute_unchecked(&statement)
            .await
            .unwrap_or_else(|error| panic!("{statement}: {error}"));
    }
    let (thrown_away, _) = board_of(test, &test.workspace_id, leaver).await;
    test.delete_container(&thrown_away, leaver)
        .await
        .expect("to the trash");
    test.execute_unchecked(&format!(
        "INSERT INTO uploads (id, user_id, container_id, name, size, content_type, part_size, \
         create_at, expires_at, closed_at, multipart_id) VALUES ('an upload', '{leaver}', \
         '{board}', 'a file', 1, 'text/plain', 1, {T0}, {later}, 0, '')",
        later = T0 + WAIT
    ))
    .await
    .expect("an upload is begun");
    // What the operator did about the account before is on the record.
    assert!(
        test.create_password_reset_by_operator(leaver, "the operator's digest", T0)
            .await
            .expect("stored"),
        "the operator's credential is stored"
    );
}

/// What the person wrote stays, under their identifier; so does the record about
/// them, without who their identity is; and of the links, the used ones, without the
/// address they were mailed to.
async fn check_what_stays(test: &TestStore, leaver: &str, board: &str, card: &str) {
    // What they wrote stays, under their identifier; so does the record about them,
    // without who their identity is.
    let written = test.get_block(card).await.expect("the card stays");
    assert_eq!(written.created_by, leaver, "the card names its writer");
    assert_eq!(
        test.get_container(board)
            .await
            .expect("the board stays")
            .created_by,
        leaver,
        "the board names its maker"
    );
    let records = test
        .audit_records(AuditPlace::Organization, EVERY)
        .await
        .expect("read");
    let bound = records
        .iter()
        .find(|record| record.action == AuditAction::OidcIdentityBound)
        .expect("the record stays");
    assert_eq!(bound.identity, None, "without who the identity is");
    assert_eq!(
        bound.detail.as_deref(),
        Some("https://provider.example"),
        "with the provider it was of"
    );
    assert_eq!(
        bound.subject.as_ref().map(|subject| subject.id.as_str()),
        Some(leaver),
        "about the account, by its identifier"
    );
    // The links: theirs that nobody used and the unused one mailed to them are gone;
    // the used ones stay, the mailed one without the address, and with a copy that
    // was used the link it was made like.
    let mut links = test
        .texts_unchecked("SELECT token_hash FROM invitations")
        .await
        .expect("read");
    links.sort();
    assert_eq!(
        links,
        [
            "a copy that was used",
            "made like, the copy used",
            "mailed, used",
            "the leaver's used link"
        ],
        "the links that were used stay"
    );
}

/// Nothing of the person is in any column of any table once the account is erased;
/// the row keeps its identifier, and what the person wrote and the records about them
/// stay, pointing at it.
#[tokio::test]
async fn an_erased_account_leaves_its_identifier_and_nothing_of_the_person() {
    let test = common::setup().await;
    let system = test.system();
    let founder = founder(&test).await;
    let leaver = person(&test).await;
    let (board, card) = board_of(&test, &test.workspace_id, &leaver).await;
    work_on(&test, &board, &card, &leaver).await;
    // A shelf of their sidebar with the board on it, and a card of somebody else's
    // that names them in a property of people.
    let shelf = Category {
        id: new_id(),
        name: "leaver-shelf".to_owned(),
        user_id: leaver.clone(),
        workspace_id: test.workspace_id.clone(),
        ..Category::default()
    };
    test.create_category(&shelf).await.expect("a shelf");
    test.add_update_category_container(&leaver, &shelf.id, std::slice::from_ref(&board))
        .await
        .expect("the board is on it");
    let (_, assigned) = board_of(&test, &test.workspace_id, &founder).await;
    let statement = format!(
        "UPDATE blocks SET fields = '{{\"properties\":{{\"who\":\"{leaver}\"}}}}' \
         WHERE id = '{assigned}'"
    );
    test.execute_unchecked(&statement)
        .await
        .unwrap_or_else(|error| panic!("{statement}: {error}"));
    let people = test
        .create_team(&test.workspace_id, "some people", &founder)
        .await
        .expect("a team of the workspace");
    test.add_team_member(&test.workspace_id, &people, &leaver, &founder)
        .await
        .expect("the leaver is in it");
    leave_traces(&test, &founder, &leaver, &board).await;
    for needle in OF_THE_PERSON {
        assert!(
            !holding(&system, needle).await.is_empty(),
            "{needle} is stored before the account is erased"
        );
    }

    erase(&test, &leaver, Erasing::AtOnce, T0 + 1).await;

    for needle in OF_THE_PERSON {
        assert_eq!(
            holding(&system, needle).await,
            NOBODY,
            "{needle} is stored nowhere"
        );
    }
    let left = test
        .texts_unchecked(&format!(
            "SELECT username || '|' || email || '|' || display_name || '|' || password_hash \
             || '|' || CAST(erased_at AS TEXT) FROM users WHERE id = '{leaver}'"
        ))
        .await
        .expect("read");
    assert_eq!(
        left,
        [format!("||||{}", T0 + 1)],
        "the row is its identifier"
    );
    // The identifier stays where it says who wrote or did something, and nowhere else:
    // no membership, nothing that was the person's own.
    assert_eq!(holding(&system, &leaver).await, KEEPS_THE_IDENTIFIER);
    check_what_stays(&test, &leaver, &board, &card).await;
    // The name and the address are free for somebody else.
    let mut again = User {
        id: new_id(),
        username: "leaver-name".to_owned(),
        email: "leaver-mail@example.com".to_owned(),
        ..User::default()
    };
    test.create_user(&mut again)
        .await
        .expect("the name is free");
    test.teardown().await;
}

/// The guard of the audit log lets who an identity is be taken out of the records
/// about an account that is being erased, and nothing else: no other column, no other
/// record, no removal.
#[tokio::test]
async fn the_audit_log_gives_up_who_an_identity_is_and_nothing_else() {
    let test = common::setup().await;
    founder(&test).await;
    let leaver = person(&test).await;
    let stays = common::user(&test, "stays").await;
    for user_id in [&leaver, &stays] {
        test.execute_unchecked(&format!(
            "INSERT INTO audit_log (id, org_id, actor_id, actor_kind, action, target_id, \
             subject_id, create_at, identity) VALUES ('{id}', '{org}', '{user_id}', 'person', \
             'oidc_identity_bound', '{org}', '{user_id}', {T0}, 'who@example.com')",
            id = new_id(),
            org = test.org_id
        ))
        .await
        .expect("recorded");
    }
    let of = |user_id: &str| format!("actor_id = '{user_id}' AND identity IS NOT NULL");
    let refused = |result: Result<u64, StoreError>, what: &str| {
        let error = result.expect_err(what).to_string();
        assert!(
            error.contains("a record of the audit log is not changed or removed"),
            "{what}: {error}"
        );
    };

    // Nobody is being erased: no record gives anything up.
    for user_id in [&leaver, &stays] {
        refused(
            test.execute_unchecked(&format!(
                "UPDATE audit_log SET identity = NULL WHERE {}",
                of(user_id)
            ))
            .await,
            "an identity of an account that is not being erased",
        );
    }
    assert_eq!(
        test.begin_account_erasure(&leaver, T0, Erasing::AtOnce)
            .await
            .expect("begun"),
        ErasureStart::Begun
    );
    // Of the record about the account being erased, every other change is refused: of
    // each column to another value, of one that is NULL to a value, and with the
    // identity taken out in the same statement.
    let workspace = &test.workspace_id;
    for change in [
        "identity = 'somebody.else@example.com'".to_owned(),
        "identity = ''".to_owned(),
        format!("id = '{}'", new_id()),
        format!("actor_id = '{stays}'"),
        format!("subject_id = '{stays}'"),
        "subject_id = NULL".to_owned(),
        "actor_id = NULL, actor_kind = 'server'".to_owned(),
        "action = 'oidc_identity_carried_over'".to_owned(),
        format!("target_id = '{}'", new_id()),
        "create_at = 1".to_owned(),
        "detail = 'https://another.example'".to_owned(),
        format!("workspace_id = '{workspace}', action = 'org_admin_joined'"),
    ] {
        for with_identity in ["", ", identity = NULL"] {
            let statement = format!(
                "UPDATE audit_log SET {change}{with_identity} WHERE {}",
                of(&leaver)
            );
            if change.starts_with("identity") && !with_identity.is_empty() {
                continue;
            }
            refused(test.execute_unchecked(&statement).await, &statement);
        }
    }
    refused(
        test.execute_unchecked(&format!("DELETE FROM audit_log WHERE {}", of(&leaver)))
            .await,
        "a removal",
    );
    refused(
        test.execute_unchecked(&format!(
            "UPDATE audit_log SET identity = NULL WHERE {}",
            of(&stays)
        ))
        .await,
        "the identity of somebody else",
    );

    let given_up = test
        .execute_unchecked(&format!(
            "UPDATE audit_log SET identity = NULL WHERE {}",
            of(&leaver)
        ))
        .await
        .expect("the identity of the account being erased is taken out");
    assert_eq!(given_up, 1);
    assert_eq!(
        count(
            &test,
            "SELECT CAST(count(*) AS TEXT) FROM audit_log WHERE identity IS NOT NULL"
        )
        .await,
        1,
        "the other record keeps its identity"
    );
    test.teardown().await;
}

/// Where a leaver is in the test's organization, and who is there with them.
struct Places {
    leaver: String,
    /// The administrator of the default workspace who has been one longest on their
    /// own account, and one who has been one for less long.
    heir: String,
    younger: String,
    /// Boards of the default workspace the leaver is the admin of: alone, and with
    /// `younger`.
    passes: String,
    shared: String,
    /// A board of `alone`, a workspace the leaver administers alone.
    orphan: String,
    alone: String,
    /// A workspace in the trash, with a board of the leaver's in the trash too.
    trashed: String,
}

async fn places_of_a_leaver(test: &TestStore, founder: &str) -> Places {
    let leaver = common::user(test, "leaver").await;
    let heir = common::user(test, "heir").await;
    let younger = common::user(test, "younger").await;
    // The default workspace: the founder joined it as the organization's
    // administrator, which makes them no heir; two administrators on their own account.
    for statement in [
        format!("UPDATE workspace_members SET via_org_admin = true WHERE user_id = '{founder}'"),
        format!(
            "UPDATE workspace_members SET role = 'admin', create_at = 10 WHERE user_id = '{heir}'"
        ),
        format!(
            "UPDATE workspace_members SET role = 'admin', create_at = 20 \
             WHERE user_id = '{younger}'"
        ),
    ] {
        test.execute_unchecked(&statement).await.expect("set");
    }
    let (passes, _) = board_of(test, &test.workspace_id, &leaver).await;
    let (shared, _) = board_of(test, &test.workspace_id, &leaver).await;
    let other_admin = fvoci_model::ContainerMember {
        container_id: shared.clone(),
        user_id: younger.clone(),
        ..fvoci_model::ContainerMember::default()
    }
    .with_role(Some(fvoci_model::ContainerRole::Admin));
    test.save_member(&other_admin)
        .await
        .expect("a second admin");
    // A second workspace, of the leaver alone, and a third in the trash.
    let alone = common::workspace(test).await;
    let trashed = common::workspace(test).await;
    for workspace_id in [&alone, &trashed] {
        test.add_workspace_member(workspace_id, &leaver, WorkspaceRole::Admin)
            .await
            .expect("let in");
    }
    let (orphan, _) = board_of(test, &alone, &leaver).await;
    let (in_the_trash, _) = board_of(test, &trashed, &leaver).await;
    test.delete_container(&in_the_trash, &leaver)
        .await
        .expect("to the trash");
    test.execute_unchecked(&format!(
        "UPDATE workspaces SET delete_at = {T0} WHERE id = '{trashed}'"
    ))
    .await
    .expect("the workspace is in the trash");

    Places {
        leaver,
        heir,
        younger,
        passes,
        shared,
        orphan,
        alone,
        trashed,
    }
}

/// A board whose only admin is erased passes to the administrator of its workspace who
/// has been one longest on their own account, and that is recorded as the server's; a
/// board of a workspace without one is left for whoever administers it next. The
/// account goes from a workspace in the trash too, and from what its boards in the
/// trash would come back with.
#[tokio::test]
async fn a_board_whose_only_admin_is_erased_passes_to_an_administrator_of_its_workspace() {
    let test = common::setup().await;
    let founder = founder(&test).await;
    let Places {
        leaver,
        heir,
        younger,
        passes,
        shared,
        orphan,
        alone,
        trashed,
    } = places_of_a_leaver(&test, &founder).await;

    assert!(matches!(
        test.request_account_deletion(&leaver, "", T0, T0 + WAIT)
            .await
            .expect("asked"),
        DeletionAsking::Asked(_)
    ));
    let erased = erase(&test, &leaver, Erasing::AfterTheWait, T0 + WAIT).await;

    let handed: Vec<(&str, &str)> = erased
        .iter()
        .flat_map(|from| &from.handed_over)
        .map(|passed| (passed.container_id.as_str(), passed.heir_id.as_str()))
        .collect();
    assert_eq!(handed, [(passes.as_str(), heir.as_str())]);
    let mut left: Vec<&str> = erased
        .iter()
        .flat_map(|from| &from.left)
        .map(String::as_str)
        .collect();
    left.sort_unstable();
    let mut workspaces = [test.workspace_id.as_str(), alone.as_str(), trashed.as_str()];
    workspaces.sort_unstable();
    assert_eq!(
        left, workspaces,
        "every workspace, the one in the trash too"
    );
    let admins = |container_id: String| {
        let test = &test;
        async move {
            test.texts_unchecked(&format!(
                "SELECT CAST(user_id AS TEXT) FROM container_members \
                 WHERE container_id = '{container_id}' AND scheme_admin ORDER BY 1"
            ))
            .await
            .expect("read")
        }
    };
    assert_eq!(admins(passes.clone()).await, std::slice::from_ref(&heir));
    assert_eq!(
        admins(shared).await,
        [younger],
        "an admin it had stays its admin"
    );
    assert_eq!(admins(orphan).await, NOBODY, "nobody to pass to");
    let recorded = test
        .audit_records(AuditPlace::Workspace(&test.workspace_id), EVERY)
        .await
        .expect("read");
    let passing = recorded
        .iter()
        .find(|record| record.target_id == passes)
        .expect("the passing is recorded");
    assert_eq!(passing.action, AuditAction::AddedToContainer);
    assert_eq!(passing.actor, fvoci_model::AuditRecordActor::Server);
    assert_eq!(
        passing.subject.as_ref().map(|subject| subject.id.as_str()),
        Some(heir.as_str())
    );
    assert_eq!(
        count(
            &test,
            &format!(
                "SELECT CAST(count(*) AS TEXT) FROM deleted_containers \
                 WHERE CAST(members AS TEXT) LIKE '%{leaver}%' \
                 OR CAST(places AS TEXT) LIKE '%{leaver}%'"
            )
        )
        .await,
        0,
        "a board of the trash comes back without the account"
    );
    test.teardown().await;
}

/// The question of the asking is asked again when the erasing begins: an account that
/// has become the only administrator of an organization with other people is not
/// erased, and nothing of it changes; once somebody else administers it, it is.
#[tokio::test]
async fn an_account_that_became_the_only_administrator_meanwhile_is_not_erased() {
    let test = common::setup().await;
    let founder = founder(&test).await;
    let second = common::user(&test, "second").await;
    set_org_role(&test, &second, OrgRole::Admin).await;
    test.execute_unchecked(&format!(
        "UPDATE users SET is_admin = true WHERE id = '{second}'"
    ))
    .await
    .expect("set");
    let asked = test
        .request_account_deletion(&founder, "", T0, T0 + WAIT)
        .await
        .expect("asked");
    assert!(matches!(asked, DeletionAsking::Asked(_)));

    // Not before its wait ran out.
    let early = test
        .begin_account_erasure(&founder, T0 + WAIT - 1, Erasing::AfterTheWait)
        .await
        .expect("asked");
    assert_eq!(early, ErasureStart::Waits);
    assert_eq!(
        test.accounts_to_erase(T0 + WAIT - 1).await.expect("listed"),
        NOBODY
    );
    assert_eq!(
        test.accounts_to_erase(T0 + WAIT).await.expect("listed"),
        std::slice::from_ref(&founder)
    );

    // The other administrator is one no more by then.
    set_org_role(&test, &second, OrgRole::Member).await;
    test.execute_unchecked(&format!(
        "UPDATE users SET is_admin = false WHERE id = '{second}'"
    ))
    .await
    .expect("set");
    let ErasureStart::Refused(standing) = test
        .begin_account_erasure(&founder, T0 + WAIT, Erasing::AfterTheWait)
        .await
        .expect("asked")
    else {
        panic!("the only administrator was erased");
    };
    assert!(standing.last_instance_admin);
    assert_eq!(standing.sole_admin_of.len(), 1);
    assert_eq!(
        test.account_by_id(&founder)
            .await
            .expect("it waits still")
            .username,
        "founder"
    );
    // The operator's word does not go past it either.
    assert!(matches!(
        test.begin_account_erasure(&founder, T0 + WAIT, Erasing::AtOnce)
            .await
            .expect("asked"),
        ErasureStart::Refused(_)
    ));

    // With an administrator again, the next run erases it; and then it is not taken back.
    set_org_role(&test, &second, OrgRole::Admin).await;
    test.execute_unchecked(&format!(
        "UPDATE users SET is_admin = true WHERE id = '{second}'"
    ))
    .await
    .expect("set");
    assert_eq!(
        test.begin_account_erasure(&founder, T0 + WAIT + 1, Erasing::AfterTheWait)
            .await
            .expect("asked"),
        ErasureStart::Begun
    );
    assert!(
        !test
            .cancel_account_deletion(&founder, T0 + WAIT + 2)
            .await
            .expect("asked"),
        "an erasing that began is not taken back"
    );
    test.teardown().await;
}

/// An erasing that was cut off after its first step, or between two organizations, is
/// found again by its mark and ends when it is run again; it does not end while the
/// account still has a place somewhere.
#[tokio::test]
async fn an_erasing_that_was_cut_off_is_taken_up_and_ends() {
    let test = common::setup().await;
    founder(&test).await;
    let leaver = person(&test).await;
    board_of(&test, &test.workspace_id, &leaver).await;

    assert_eq!(
        test.begin_account_erasure(&leaver, T0, Erasing::AtOnce)
            .await
            .expect("begun"),
        ErasureStart::Begun
    );
    // Cut off here: the account has its places still, and is not ended.
    assert_eq!(
        test.finish_account_erasure(&leaver, T0 + 1)
            .await
            .expect("asked"),
        ErasureEnd::StillPlaced,
        "an account with a place somewhere is not ended"
    );
    assert_eq!(
        test.accounts_to_erase(T0 + 1).await.expect("listed"),
        std::slice::from_ref(&leaver),
        "the mark finds it again, whatever its wait"
    );
    // Taken up: the first step says it began, and the rest is done once.
    assert_eq!(
        test.begin_account_erasure(&leaver, T0 + 2, Erasing::AfterTheWait)
            .await
            .expect("asked"),
        ErasureStart::Begun
    );
    erase_from_organizations(&test, &leaver, Erasing::AfterTheWait).await;
    assert_eq!(
        erase_from_organizations(&test, &leaver, Erasing::AfterTheWait).await,
        Vec::<ErasedFrom>::new(),
        "no organization is left to go from"
    );
    assert_eq!(
        test.finish_account_erasure(&leaver, T0 + 3)
            .await
            .expect("ended"),
        ErasureEnd::Erased,
        "ended"
    );
    assert_eq!(
        test.finish_account_erasure(&leaver, T0 + 4)
            .await
            .expect("asked"),
        ErasureEnd::ErasedBefore,
        "ended once"
    );
    assert_eq!(
        test.accounts_to_erase(T0 + 5).await.expect("listed"),
        NOBODY
    );
    assert_eq!(
        test.begin_account_erasure(&leaver, T0 + 5, Erasing::AtOnce)
            .await
            .expect("asked"),
        ErasureStart::NotThere
    );
    test.teardown().await;
}

fn file_of(board_id: &str) -> FileInfo {
    FileInfo {
        id: new_id(),
        container_id: board_id.to_owned(),
        stored_name: "stored.png".to_owned(),
        name: "a picture".to_owned(),
        extension: ".png".to_owned(),
        size: 2048,
        path: format!("boards/{board_id}/stored.png"),
        create_at: 1,
    }
}

/// Tables that hold rows of an organization in use: of the one that goes, they hold
/// none afterwards.
const HELD_BY_AN_ORGANIZATION: [&str; 11] = [
    "audit_log",
    "blocks",
    "containers",
    "container_members",
    "deleted_containers",
    "file_info",
    "invitations",
    "oidc_providers",
    "organization_members",
    "workspace_members",
    "workspaces",
];

/// Where the identifier of an organization that went is still found: what the file
/// storage is yet to be told, which its job takes away once the objects are removed.
/// (SQLite, which holds one organization, does not name it there.)
fn keeps_the_organization() -> &'static [&'static str] {
    match common::db_type() {
        DbType::Postgres => &["loose_objects.org_id"],
        DbType::Sqlite => &[],
    }
}

/// An account that is alone in an organization it administers.
struct Alone {
    leaver: String,
    org: String,
    /// The organization's default workspace, which the leaver administers.
    workspace: String,
    /// On PostgreSQL, an organization of others; the test's own is somebody else's
    /// too, with the leaver a member of it. SQLite holds one organization, the
    /// leaver's.
    third: Option<String>,
}

/// Makes the account that is alone in an organization, and somebody who administers
/// the instance from outside of it: the instance keeps an administrator.
async fn alone_in_an_organization(test: &TestStore) -> Alone {
    let system = test.system();
    let operator = common::user(&system, "operator").await;
    system
        .execute_unchecked(&format!(
            "UPDATE users SET is_admin = true WHERE id = '{operator}'"
        ))
        .await
        .expect("set");
    let several = common::db_type() == DbType::Postgres;
    let (alone, alone_workspace, third) = if several {
        // The test's organization is somebody else's, the leaver a member of it; a
        // second is the leaver's alone, and a third nobody's of them.
        founder(test).await;
        let (alone, alone_workspace) = common::organization(&system, "Alone").await;
        let (third, third_workspace) = common::organization(&system, "Third").await;
        let third_store = test.for_org(&third).expect("an organization");
        let elsewhere = common::user(&third_store, "elsewhere").await;
        board_of(&third_store, &third_workspace, &elsewhere).await;
        link_by(
            &third_store,
            &third_workspace,
            &elsewhere,
            "a link of the third",
        )
        .await;
        (alone, alone_workspace, Some(third))
    } else {
        (test.org_id.clone(), test.workspace_id.clone(), None)
    };
    let alone_store = test.for_org(&alone).expect("an organization");
    let leaver = common::user(test, "leaver").await;
    if several {
        board_of(test, &test.workspace_id, &leaver).await;
        alone_store
            .add_organization_member(&leaver, OrgRole::Admin)
            .await
            .expect("its administrator");
        alone_store
            .add_workspace_member(&alone_workspace, &leaver, WorkspaceRole::Admin)
            .await
            .expect("let in");
    } else {
        // The one organization is the leaver's alone, who administers it and not the
        // instance: the operator, who is in no organization, does that.
        for statement in [
            format!("UPDATE users SET is_admin = false WHERE id = '{leaver}'"),
            format!("UPDATE organization_members SET role = 'admin' WHERE user_id = '{leaver}'"),
            format!("UPDATE workspace_members SET role = 'admin' WHERE user_id = '{leaver}'"),
            format!("DELETE FROM workspace_members WHERE user_id = '{operator}'"),
            format!("DELETE FROM organization_members WHERE user_id = '{operator}'"),
        ] {
            system.execute_unchecked(&statement).await.expect("set");
        }
    }
    Alone {
        leaver,
        org: alone,
        workspace: alone_workspace,
        third,
    }
}

/// Begins erasing the account that is alone in `org` at `now`, as its wait running out
/// begins it. SQLite's one organization is the instance's own, which goes with no
/// account: the first step refuses there, and what it would write is written here, so
/// that the statements that take an organization away are held to both databases.
async fn begin_alone(test: &TestStore, leaver: &str, org: &str, now: i64) {
    if common::db_type() == DbType::Postgres {
        let begun = test
            .begin_account_erasure(leaver, now, Erasing::AfterTheWait)
            .await
            .expect("asked");
        assert_eq!(begun, ErasureStart::Begun, "the erasing begins");
        return;
    }
    // Its asking was refused too: the operator's word is refused the same way.
    let begun = test
        .begin_account_erasure(leaver, now, Erasing::AtOnce)
        .await
        .expect("asked");
    let ErasureStart::Refused(standing) = begun else {
        panic!("the instance's own organization was to go: {begun:?}");
    };
    assert_eq!(standing.sole_admin_of[0].id, org, "{standing:?}");
    for statement in [
        format!(
            "UPDATE users SET delete_at = {now}, delete_after = {now}, \
             erase_started_at = {now} WHERE id = '{leaver}'"
        ),
        format!("UPDATE organizations SET delete_at = {now} WHERE id = '{org}'"),
    ] {
        test.system()
            .execute_unchecked(&statement)
            .await
            .unwrap_or_else(|error| panic!("{statement}: {error}"));
    }
}

/// Fills an organization with what one holds: a board with a card and a file, what
/// its person wrote on the card and a group of people they are in, a board and a
/// workspace in the trash, a link, a provider, and a record.
async fn fill(alone_store: &Store, alone: &str, alone_workspace: &str, leaver: &str) {
    let (board, card) = board_of(alone_store, alone_workspace, leaver).await;
    work_on(alone_store, &board, &card, leaver).await;
    let people = alone_store
        .create_team(alone_workspace, "some people", leaver)
        .await
        .expect("a team of the workspace");
    alone_store
        .add_team_member(alone_workspace, &people, leaver, leaver)
        .await
        .expect("the leaver is in it");
    alone_store
        .save_file_info(&file_of(&board))
        .await
        .expect("a file");
    let (in_the_trash, _) = board_of(alone_store, alone_workspace, leaver).await;
    alone_store
        .delete_container(&in_the_trash, leaver)
        .await
        .expect("to the trash");
    let trashed = common::workspace(alone_store).await;
    alone_store
        .add_workspace_member(&trashed, leaver, WorkspaceRole::Admin)
        .await
        .expect("let in");
    board_of(alone_store, &trashed, leaver).await;
    alone_store
        .execute_unchecked(&format!(
            "UPDATE workspaces SET delete_at = {T0} WHERE id = '{trashed}'"
        ))
        .await
        .expect("the workspace is in the trash");
    link_by(alone_store, alone_workspace, leaver, "a link into it").await;
    alone_store
        .execute_unchecked(&format!(
            "INSERT INTO oidc_providers (id, org_id, issuer, client_id, client_secret, \
             auto_join, enabled, create_at, update_at) VALUES ('{}', '{alone}', \
             'https://provider.example', 'client', '', true, true, {T0}, {T0})",
            new_id()
        ))
        .await
        .expect("a provider");
    alone_store
        .execute_unchecked(&format!(
            "INSERT INTO audit_log (id, org_id, actor_id, actor_kind, action, target_id, \
             create_at) VALUES ('{}', '{alone}', '{leaver}', 'person', \
             'oidc_provider_configured', '{alone}', {T0})",
            new_id()
        ))
        .await
        .expect("a record");
}

/// The organization of others, and the test's own, hold the rows they held (`kept`,
/// in that order), but for the leaver's own membership of the test's.
async fn check_the_others_keep(test: &TestStore, third: &str, kept: Vec<Vec<(String, i64)>>) {
    let after = vec![
        rows_of(&test.system(), third).await,
        rows_of(&test.system(), &test.org_id).await,
    ];
    // The leaver's own membership of the test's organization went with the account.
    let mut expected = kept;
    for (table, rows) in &mut expected[1] {
        if [
            "organization_members",
            "workspace_members",
            "container_members",
        ]
        .contains(&table.as_str())
        {
            *rows -= 1;
        }
    }
    assert_eq!(after, expected, "the other organizations keep every row");
    assert!(
        test.count_rows("blocks").await.expect("counted") > 0,
        "what the account wrote in the organization that stays, stays"
    );
}

/// An organization that has nobody but the account goes with it, and everything in
/// it: no column of any table names it afterwards, but what the storage is still to be
/// told, and no table holds more rows than before it was filled. The other
/// organizations of the account, and those of others, keep every row. SQLite's one
/// organization is the instance's own, which is not asked to go: it is taken away
/// here all the same ([`begin_alone`]).
#[tokio::test]
async fn an_organization_that_has_nobody_else_goes_with_the_account() {
    let test = common::setup().await;
    let system = test.system();
    let Alone {
        leaver,
        org: alone,
        workspace: alone_workspace,
        third,
    } = alone_in_an_organization(&test).await;
    let several = third.is_some();
    let alone_store = test.for_org(&alone).expect("an organization");
    let before = rows_of_every_table(&system).await;
    fill(&alone_store, &alone, &alone_workspace, &leaver).await;
    for table in HELD_BY_AN_ORGANIZATION {
        assert!(
            alone_store.count_rows(table).await.expect("counted") > 0,
            "{table} holds rows of it before"
        );
    }
    let kept = match &third {
        Some(third) => vec![
            rows_of(&system, third).await,
            rows_of(&system, &test.org_id).await,
        ],
        None => Vec::new(),
    };

    let standing = test.account_deletion_standing(&leaver).await.expect("read");
    // The instance's own organization stays, whoever is in it.
    let (goes, stays) = if several {
        (&standing.alone_in, &standing.sole_admin_of)
    } else {
        (&standing.sole_admin_of, &standing.alone_in)
    };
    assert_eq!(goes.len(), 1, "{standing:?}");
    assert_eq!(goes[0].id, alone, "{standing:?}");
    assert!(stays.is_empty(), "{standing:?}");
    let counted = alone_store
        .leaving_organization(goes[0].clone())
        .await
        .expect("counted");
    assert_eq!(
        (counted.boards, counted.wikis, counted.files),
        (Some(1), Some(0), Some(1)),
        "what is in use is counted"
    );
    let asked = test
        .request_account_deletion(&leaver, "", T0, T0 + WAIT)
        .await
        .expect("asked");
    assert_eq!(
        matches!(asked, DeletionAsking::Asked(_)),
        several,
        "asked where the organization goes: {asked:?}"
    );

    begin_alone(&test, &leaver, &alone, T0 + WAIT).await;
    let erased = erase(&test, &leaver, Erasing::AfterTheWait, T0 + WAIT).await;

    let removed: Vec<&ErasedFrom> = erased
        .iter()
        .filter(|from| from.removed.is_some())
        .collect();
    assert_eq!(removed.len(), 1, "one organization went: {erased:?}");
    let files = &removed[0].removed.as_ref().expect("removed").files;
    assert_eq!(files.len(), 1, "its file is doomed: {files:?}");
    assert_eq!(
        holding(&system, &alone).await,
        keeps_the_organization(),
        "nothing names the organization but what the storage is to be told"
    );
    for table in HELD_BY_AN_ORGANIZATION {
        // SQLite holds the system's organization too, which has a workspace.
        let systems = i64::from(!several && table == "workspaces");
        assert_eq!(
            alone_store.count_rows(table).await.expect("counted"),
            systems,
            "{table} holds nothing of it afterwards"
        );
    }
    // Nothing of what filled it is left, in a table that names an organization or in
    // one that does not: but for the records of the leaving where the account was a
    // member besides, and what the storage is to be told, no table has grown.
    for (table, rows) in rows_of_every_table(&system).await {
        if ["audit_log", "loose_objects"].contains(&table.as_str()) {
            continue;
        }
        assert!(
            rows <= before[&table],
            "{table} holds {rows} rows, and held {} before the organization was filled",
            before[&table]
        );
    }
    if let Some(third) = &third {
        check_the_others_keep(&test, third, kept).await;
    }
    test.teardown().await;
}

/// Somebody who came into the organization during the wait keeps it: the account is
/// then the only administrator of an organization with other people, and is not
/// erased.
#[tokio::test]
async fn an_organization_somebody_came_into_during_the_wait_stays() {
    if common::db_type() != DbType::Postgres {
        return;
    }
    let test = common::setup().await;
    let system = test.system();
    founder(&test).await;
    let (alone, _) = common::organization(&system, "Alone").await;
    let alone_store = test.for_org(&alone).expect("an organization");
    let leaver = common::user(&test, "leaver").await;
    alone_store
        .add_organization_member(&leaver, OrgRole::Admin)
        .await
        .expect("its administrator");
    assert!(matches!(
        test.request_account_deletion(&leaver, "", T0, T0 + WAIT)
            .await
            .expect("asked"),
        DeletionAsking::Asked(_)
    ));
    let newcomer = common::user(&system, "newcomer").await;
    alone_store
        .add_organization_member(&newcomer, OrgRole::Member)
        .await
        .expect("let in");

    let ErasureStart::Refused(standing) = test
        .begin_account_erasure(&leaver, T0 + WAIT, Erasing::AfterTheWait)
        .await
        .expect("asked")
    else {
        panic!("the only administrator of an organization with a newcomer was erased");
    };
    assert_eq!(standing.sole_admin_of[0].id, alone);
    assert_eq!(
        alone_store
            .count_rows("organizations")
            .await
            .expect("counted"),
        1,
        "the organization stays"
    );
    assert_eq!(
        count(
            &alone_store,
            "SELECT CAST(count(*) AS TEXT) FROM organizations WHERE delete_at = 0"
        )
        .await,
        1,
        "in use"
    );
    test.teardown().await;
}

/// Somebody who comes into an organization after it was put out of use to go, past the
/// question the erasing began with, keeps it: it is in use again, the account leaves
/// it as it leaves any other, and whoever runs the server is to be told that it has
/// no administrator.
#[tokio::test]
async fn an_organization_somebody_came_into_past_the_mark_is_in_use_again() {
    let test = common::setup().await;
    let system = test.system();
    let Alone { leaver, org, .. } = alone_in_an_organization(&test).await;
    let in_use = format!(
        "SELECT CAST(count(*) AS TEXT) FROM organizations \
         WHERE delete_at = 0 AND CAST(id AS TEXT) = '{org}'"
    );
    if common::db_type() == DbType::Postgres {
        assert!(matches!(
            test.request_account_deletion(&leaver, "", T0 - WAIT, T0)
                .await
                .expect("asked"),
            DeletionAsking::Asked(_)
        ));
    }
    begin_alone(&test, &leaver, &org, T0).await;
    assert_eq!(count(&system, &in_use).await, 0, "out of use, to go");
    let newcomer = common::user(&system, "newcomer").await;
    system
        .execute_unchecked(&format!(
            "INSERT INTO organization_members (org_id, user_id, role, create_at) \
             VALUES ('{org}', '{newcomer}', 'member', {T0}) ON CONFLICT DO NOTHING"
        ))
        .await
        .expect("somebody comes in");

    let erased = erase_from_organizations(&test, &leaver, Erasing::AfterTheWait).await;

    let kept: Vec<&ErasedFrom> = erased
        .iter()
        .filter(|from| from.kept_for_newcomer)
        .collect();
    assert_eq!(kept.len(), 1, "the organization is kept: {erased:?}");
    assert_eq!(kept[0].removed, None, "nothing of it is removed");
    assert_eq!(count(&system, &in_use).await, 1, "in use again");
    assert_eq!(
        test.finish_account_erasure(&leaver, T0 + 1)
            .await
            .expect("finished"),
        ErasureEnd::Erased,
        "the account is erased all the same"
    );
    assert_eq!(
        system
            .texts_unchecked(&format!(
                "SELECT CAST(user_id AS TEXT) FROM organization_members \
                 WHERE CAST(org_id AS TEXT) = '{org}'"
            ))
            .await
            .expect("read"),
        [newcomer],
        "the newcomer is in it, alone"
    );
    test.teardown().await;
}

/// The instance's own organization goes with no account, whoever is in it: the
/// organization after it would be the instance's then, and its administrators the
/// instance's. Its only administrator is not erased, alone in it or not.
#[tokio::test]
async fn the_instances_own_organization_does_not_go_with_an_account() {
    let test = common::setup().await;
    let system = test.system();
    let (alone, others) = if common::db_type() == DbType::Postgres {
        // The founder is alone in the test's organization, the oldest; somebody of
        // another administers the instance too.
        let founder = founder(&test).await;
        let (elsewhere, _) = common::organization(&system, "Elsewhere").await;
        let elsewhere_store = test.for_org(&elsewhere).expect("an organization");
        let operator = common::user(&elsewhere_store, "operator").await;
        system
            .execute_unchecked(&format!(
                "UPDATE users SET is_admin = true WHERE id = '{operator}'"
            ))
            .await
            .expect("set");
        (founder, vec![elsewhere])
    } else {
        (alone_in_an_organization(&test).await.leaver, Vec::new())
    };

    let standing = test.account_deletion_standing(&alone).await.expect("read");
    assert!(!standing.last_instance_admin, "{standing:?}");
    assert_eq!(standing.alone_in, [], "it does not go: {standing:?}");
    assert_eq!(standing.sole_admin_of.len(), 1, "{standing:?}");
    assert_eq!(standing.sole_admin_of[0].id, test.org_id, "{standing:?}");
    assert!(matches!(
        test.request_account_deletion(&alone, "", T0, T0 + WAIT)
            .await
            .expect("asked"),
        DeletionAsking::Refused(_)
    ));
    assert!(matches!(
        test.begin_account_erasure(&alone, T0, Erasing::AtOnce)
            .await
            .expect("asked"),
        ErasureStart::Refused(_)
    ));
    // The organization the instance's administrators are of is the one it was.
    let in_use = system
        .texts_unchecked(
            "SELECT CAST(id AS TEXT) FROM organizations \
             WHERE delete_at = 0 AND is_system = false ORDER BY create_at, id",
        )
        .await
        .expect("read");
    assert_eq!(in_use[0], test.org_id, "the oldest in use");
    assert_eq!(in_use.len(), 1 + others.len(), "none went");
    test.teardown().await;
}

/// An organization that is out of use to go with one account is not taken along, nor
/// put back in use, by the erasing of another that has something left in it: it is
/// the first's to go with.
#[tokio::test]
async fn an_organization_goes_with_the_account_it_was_put_out_of_use_for() {
    if common::db_type() != DbType::Postgres {
        return;
    }
    let test = common::setup().await;
    let system = test.system();
    let Alone {
        leaver,
        org,
        workspace,
        ..
    } = alone_in_an_organization(&test).await;
    let org_store = test.for_org(&org).expect("an organization");
    let (board, _) = board_of(&org_store, &workspace, &leaver).await;
    // Somebody of another organization kept a role on a board of this one.
    let other = common::user(&test, "other").await;
    system
        .execute_unchecked(&format!(
            "INSERT INTO container_members (container_id, user_id, org_id, scheme_admin, \
             scheme_editor, scheme_commenter, scheme_viewer) \
             VALUES ('{board}', '{other}', '{org}', false, true, false, false)"
        ))
        .await
        .expect("a role is left");
    assert!(matches!(
        test.request_account_deletion(&leaver, "", T0 - WAIT, T0)
            .await
            .expect("asked"),
        DeletionAsking::Asked(_)
    ));
    begin_alone(&test, &leaver, &org, T0).await;
    let out_of_use = format!(
        "SELECT CAST(count(*) AS TEXT) FROM organizations \
         WHERE delete_at > 0 AND CAST(id AS TEXT) = '{org}'"
    );

    let of_other = erase(&test, &other, Erasing::AtOnce, T0 + 1).await;

    assert!(
        of_other
            .iter()
            .all(|from| from.removed.is_none() && !from.kept_for_newcomer),
        "the other's erasing neither takes the organization nor keeps it: {of_other:?}"
    );
    assert_eq!(count(&system, &out_of_use).await, 1, "still to go");
    let of_leaver = erase(&test, &leaver, Erasing::AfterTheWait, T0 + 2).await;
    assert_eq!(
        of_leaver
            .iter()
            .filter(|from| from.removed.is_some())
            .count(),
        1,
        "it goes with the account it was put out of use for: {of_leaver:?}"
    );
    assert_eq!(
        holding(&system, &org).await,
        NOBODY,
        "nothing names the organization: it held no file"
    );
    test.teardown().await;
}

/// What is left of an erased account is answered to whoever asks by its identifier,
/// whether they saw the account before or not: a mention of it in what they read is a
/// deleted user's. By name it is found by nobody; an account in use that the seer
/// does not see is still not answered.
#[tokio::test]
async fn an_erased_account_is_answered_by_its_identifier_to_whoever_asks() {
    let test = common::setup().await;
    let seer = founder(&test).await;
    let leaver = person(&test).await;
    // Somebody the seer shares no workspace with, and wrote nothing the seer reads.
    let elsewhere = common::workspace(&test).await;
    let unseen = common::user(&test, "unseen").await;
    test.execute_unchecked(&format!(
        "DELETE FROM workspace_members WHERE user_id = '{unseen}'"
    ))
    .await
    .expect("out of the default workspace");
    test.add_workspace_member(&elsewhere, &unseen, WorkspaceRole::Member)
        .await
        .expect("into another");
    erase(&test, &leaver, Erasing::AtOnce, T0).await;

    let sight = |looking| Sight::InWorkspace {
        seer_id: &seer,
        workspace_id: &test.workspace_id,
        reach: Reach::Workspace,
        looking,
    };
    let asked = [leaver.clone(), unseen.clone()];
    let found = test
        .get_users_in_sight(sight(Looking::ByName), &asked)
        .await
        .expect("asked");
    assert_eq!(found.len(), 1, "the one that was erased alone: {found:?}");
    assert_eq!(found[0].id, leaver, "by its identifier");
    assert_eq!(
        (
            found[0].username.as_str(),
            found[0].email.as_str(),
            found[0].display_name.as_str()
        ),
        ("", "", ""),
        "and nothing else"
    );
    assert_eq!(
        test.search_users_in_sight(sight(Looking::AmongPeople), "")
            .await
            .expect("searched")
            .iter()
            .filter(|user| user.id == leaver)
            .count(),
        0,
        "among the people it is not"
    );
    if common::db_type() == DbType::Postgres {
        // Across the wall too, and there alone: somebody of another organization is
        // answered what is left of the erased account, and nobody of this one.
        let system = test.system();
        let (other_org, other_workspace) = common::organization(&system, "Other").await;
        let other_store = test.for_org(&other_org).expect("an organization");
        let outsider = common::user(&other_store, "outsider").await;
        let from_outside = Sight::InWorkspace {
            seer_id: &outsider,
            workspace_id: &other_workspace,
            reach: Reach::Workspace,
            looking: Looking::ByName,
        };
        let asked = [leaver.clone(), unseen, seer.clone()];
        let found = other_store
            .get_users_in_sight(from_outside, &asked)
            .await
            .expect("asked");
        assert_eq!(
            found
                .iter()
                .map(|user| user.id.as_str())
                .collect::<Vec<_>>(),
            [leaver.as_str()],
            "what is left of the erased account, and nobody in use"
        );
        assert_eq!(found[0].username, "", "no name");
    }
    test.teardown().await;
}
