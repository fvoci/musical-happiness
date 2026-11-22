//! An account leaving an organization at its own asking: nothing of it is left there,
//! what it wrote and the organization's word of who made it stay, what stands in the way
//! changes nothing, and of two administrators who leave at once one stays.

use fvoci_model::{
    AuditAction, AuditRecordActor, Block, BlockType, Container, ContainerType, OrgRole,
    SubscriberType, Subscription, WorkspaceRole, new_id,
};
use fvoci_store::{
    AuditPlace, DbType, Departure, DepartureRefused, Joining, NewLink, Paging, Store, Tenant,
};

use crate::common::{self, TestStore};
use crate::lock_gate::{Gate, ended};

/// A time in milliseconds that the tests count from.
const T0: i64 = 1_800_000_000_000;

/// Every record, on one page.
const EVERY: Paging = Paging {
    page: 0,
    per_page: 0,
};

/// The tables that hold who is where, and what they follow, counted together: what a
/// refused leaving leaves as it was.
const PLACES: [&str; 7] = [
    "organization_members",
    "workspace_members",
    "container_members",
    "team_members",
    "subscriptions",
    "invitations",
    "audit_log",
];

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

/// The user follows the block.
async fn follow(store: &Store, block_id: &str, user_id: &str) {
    let subscription = Subscription {
        block_type: BlockType::Card,
        block_id: block_id.to_owned(),
        subscriber_type: SubscriberType::User,
        subscriber_id: user_id.to_owned(),
        notified_at: 0,
        create_at: 0,
        delete_at: 0,
    };
    store
        .create_subscription(subscription)
        .await
        .expect("followed");
}

/// A link into the workspace made by `user_id`, under the digest.
async fn link_by(store: &Store, workspace_id: &str, user_id: &str, digest: &str) {
    let link = NewLink {
        workspace_id,
        guest_of: None,
        token_hash: digest,
        created_by: user_id,
        now: T0,
        expires_at: i64::MAX,
        max_uses: None,
        mailed: None,
    };
    store
        .create_invitation(&link)
        .await
        .expect("the link is made");
}

/// Makes the user what `role` says in the handle's organization, past the routes.
async fn set_org_role(test: &TestStore, org_id: &str, user_id: &str, role: OrgRole) {
    test.execute_unchecked(&format!(
        "UPDATE organization_members SET role = '{}' WHERE user_id = '{user_id}' \
         AND org_id = '{org_id}'",
        role.as_str()
    ))
    .await
    .expect("the role is set");
}

/// How many rows of the organization `org_id` name the user, in each table where a
/// person has a place, as `table: count`.
async fn places_of(test: &TestStore, org_id: &str, user_id: &str) -> Vec<String> {
    let of_org_boards = format!(
        "SELECT c.id FROM containers AS c JOIN workspaces AS w ON w.id = c.workspace_id \
         WHERE w.org_id = '{org_id}'"
    );
    let queries = [
        (
            "organization_members",
            format!("org_id = '{org_id}' AND user_id = '{user_id}'"),
        ),
        (
            "workspace_members",
            format!(
                "user_id = '{user_id}' AND workspace_id IN \
                 (SELECT id FROM workspaces WHERE org_id = '{org_id}')"
            ),
        ),
        (
            "container_members",
            format!("user_id = '{user_id}' AND container_id IN ({of_org_boards})"),
        ),
        (
            "team_members",
            format!(
                "user_id = '{user_id}' AND team_id IN \
                 (SELECT t.id FROM teams AS t JOIN workspaces AS w ON w.id = t.workspace_id \
                 WHERE w.org_id = '{org_id}')"
            ),
        ),
        (
            "subscriptions",
            format!(
                "subscriber_id = '{user_id}' AND block_id IN \
                 (SELECT id FROM blocks WHERE container_id IN ({of_org_boards}))"
            ),
        ),
    ];
    let system = test.system();
    let mut counted = Vec::new();
    for (table, condition) in queries {
        let found = system
            .texts_unchecked(&format!(
                "SELECT CAST(count(*) AS TEXT) FROM {table} WHERE {condition}"
            ))
            .await
            .expect("counted");
        counted.push(format!("{table}: {}", found[0]));
    }
    counted
}

/// How many rows each table of [`PLACES`] has, in every organization.
async fn counts(test: &TestStore) -> Vec<i64> {
    let system = test.system();
    let mut counted = Vec::new();
    for table in PLACES {
        counted.push(system.count_rows(table).await.expect("counted"));
    }
    counted
}

/// The text of one cell, read past the wall.
async fn one(test: &TestStore, query: &str) -> String {
    let found = test.system().texts_unchecked(query).await.expect("read");
    found.into_iter().next().unwrap_or_default()
}

/// The leaver of [`placed_everywhere`], what they have in the organization, and its
/// founder.
struct Placed {
    founder: String,
    leaver: String,
    /// A workspace the leaver is in as the organization's administrator alone.
    joined: String,
    /// A board the leaver alone administers, and a card of it they wrote.
    theirs: String,
    their_card: String,
    /// A board of the founder's.
    founders: String,
}

/// An administrator of the organization beside its founder, who made it, with a place
/// of every kind there: a board of theirs with a card they wrote and follow, a card of
/// the founder's followed without a role on its board, as a page of an open wiki is, a
/// team, a workspace joined as the organization's administrator, and two links they
/// made, one used.
async fn placed_everywhere(test: &TestStore) -> Placed {
    let founder = common::user(test, "founder").await;
    let leaver = common::user(test, "leaver").await;
    let workspace = test.workspace_id.clone();
    set_org_role(test, &test.org_id, &leaver, OrgRole::Admin).await;
    test.execute_unchecked(&format!(
        "UPDATE organizations SET created_by = '{leaver}' WHERE id = '{}'",
        test.org_id
    ))
    .await
    .expect("made by the leaver");
    let (theirs, their_card) = board_of(test, &workspace, &leaver).await;
    let (founders, founders_card) = board_of(test, &workspace, &founder).await;
    follow(test, &their_card, &leaver).await;
    follow(test, &founders_card, &leaver).await;
    let team = test
        .create_team(&workspace, "Design", &founder)
        .await
        .expect("a team");
    test.add_team_member(&workspace, &team, &leaver, &founder)
        .await
        .expect("in the team");
    let joined = common::workspace(test).await;
    assert_eq!(
        test.join_workspace_as_org_admin(&joined, &leaver)
            .await
            .expect("joined"),
        Joining::Joined,
        "the leaver joins a workspace as the organization's administrator"
    );
    link_by(test, &workspace, &leaver, "unused-link").await;
    link_by(test, &workspace, &leaver, "used-link").await;
    test.execute_unchecked("UPDATE invitations SET uses = 1 WHERE token_hash = 'used-link'")
        .await
        .expect("used");
    Placed {
        founder,
        leaver,
        joined,
        theirs,
        their_card,
        founders,
    }
}

/// What stays of the leaver of [`placed_everywhere`] who left: the link they made that
/// was used, what they wrote, the organization's word of who made it, and the founder's
/// board as it was.
async fn what_stays(test: &TestStore, placed: &Placed) {
    let Placed {
        founder,
        leaver,
        their_card,
        founders,
        ..
    } = placed;
    let links = test
        .system()
        .texts_unchecked(&format!(
            "SELECT token_hash FROM invitations WHERE created_by = '{leaver}' ORDER BY 1"
        ))
        .await
        .expect("read");
    assert_eq!(links, ["used-link"], "the unused link goes");
    let written = format!("SELECT CAST(created_by AS TEXT) FROM blocks WHERE id = '{their_card}'");
    assert_eq!(
        &one(test, &written).await,
        leaver,
        "what they wrote stays theirs"
    );
    let made = format!(
        "SELECT CAST(created_by AS TEXT) FROM organizations WHERE id = '{}'",
        test.org_id
    );
    assert_eq!(
        &one(test, &made).await,
        leaver,
        "the organization is still of their making"
    );
    let administered = format!(
        "SELECT CAST(count(*) AS TEXT) FROM container_members \
         WHERE container_id = '{founders}' AND user_id = '{founder}' AND scheme_admin"
    );
    assert_eq!(
        one(test, &administered).await,
        "1",
        "a board of somebody else's is as it was"
    );
}

/// A member who leaves has no place, role, team, joining, follow or unused link left in
/// the organization, and keeps those of another; what it wrote stays, a board it alone
/// administered passes on, a used link it made stays, and the organization still has it
/// as its maker. The organization's reads alone are asked anew.
#[tokio::test]
async fn whoever_leaves_has_nothing_left_in_the_organization_but_what_they_wrote() {
    let test = common::setup().await;
    let placed = placed_everywhere(&test).await;
    let Placed {
        founder,
        leaver,
        joined,
        theirs,
        ..
    } = &placed;
    // Another organization the leaver is in: their own space.
    let other = one(
        &test,
        &format!("SELECT CAST(id AS TEXT) FROM organizations WHERE owner_id = '{leaver}'"),
    )
    .await;
    let other_store = test.for_org(&other).expect("an organization");
    let before = (
        test.visibility_version().await,
        other_store.visibility_version().await,
    );
    let theirs_elsewhere = places_of(&test, &other, leaver).await;

    let left = match test.leave_organization(leaver).await.expect("asked") {
        Departure::Left(left) => left,
        other => panic!("the leaver leaves: {other:?}"),
    };

    let mut workspaces = vec![test.workspace_id.clone(), joined.clone()];
    workspaces.sort_unstable();
    assert_eq!(left.left, workspaces, "the joined workspace is left too");
    let handed: Vec<(&str, &str)> = left
        .handed_over
        .iter()
        .map(|passed| (passed.container_id.as_str(), passed.heir_id.as_str()))
        .collect();
    assert_eq!(handed, [(theirs.as_str(), founder.as_str())]);
    assert_eq!(left.removed, None);
    assert!(!left.kept_for_newcomer);
    assert_eq!(
        places_of(&test, &test.org_id, leaver).await,
        [
            "organization_members: 0",
            "workspace_members: 0",
            "container_members: 0",
            "team_members: 0",
            "subscriptions: 0",
        ]
    );
    assert_eq!(
        places_of(&test, &other, leaver).await,
        theirs_elsewhere,
        "another organization keeps them"
    );
    what_stays(&test, &placed).await;
    let records = test
        .audit_records(AuditPlace::Workspace(joined), EVERY)
        .await
        .expect("read");
    let ended = records
        .iter()
        .find(|record| record.action == AuditAction::OrgAdminLeft)
        .expect("the end of the joining is recorded where it was joined");
    assert!(
        matches!(&ended.actor, AuditRecordActor::Person(person) if &person.id == leaver),
        "as the leaver's: {:?}",
        ended.actor
    );
    let after = (
        test.visibility_version().await,
        other_store.visibility_version().await,
    );
    assert_ne!(after.0, before.0, "the organization's reads are asked anew");
    assert_eq!(after.1, before.1, "another's are not");
    test.teardown().await;
}

/// Makes the user an administrator of the instance, past the routes.
async fn administers_the_instance(test: &TestStore, user_id: &str) {
    test.execute_unchecked(&format!(
        "UPDATE users SET is_admin = true WHERE id = '{user_id}'"
    ))
    .await
    .expect("an administrator of the instance");
}

/// Whether the user administers the instance, past the routes.
async fn runs_the_instance(test: &TestStore, user_id: &str) -> bool {
    let query = format!(
        "SELECT CAST(count(*) AS TEXT) FROM users WHERE is_admin = true AND id = '{user_id}'"
    );
    one(test, &query).await == "1"
}

/// An administrator of the instance's own organization, who administers the instance as
/// one, leaves it and administers the instance no more; its founder still does. A
/// workspace they were the only administrator of is told as one without any.
#[tokio::test]
async fn who_leaves_the_instances_organization_administers_the_instance_no_more() {
    let test = common::setup().await;
    let founder = common::user(&test, "founder").await;
    let leaver = common::user(&test, "leaver").await;
    set_org_role(&test, &test.org_id, &leaver, OrgRole::Admin).await;
    administers_the_instance(&test, &leaver).await;
    let theirs = common::workspace(&test).await;
    test.add_workspace_member(&theirs, &leaver, WorkspaceRole::Admin)
        .await
        .expect("the leaver administers a workspace alone");

    let left = match test.leave_organization(&leaver).await.expect("asked") {
        Departure::Left(left) => left,
        other => panic!("the leaver leaves: {other:?}"),
    };

    assert!(
        !runs_the_instance(&test, &leaver).await,
        "who left administers the instance no more"
    );
    assert!(
        runs_the_instance(&test, &founder).await,
        "the founder still does"
    );
    assert_eq!(left.workspaces_without_administrator, [theirs]);
    test.teardown().await;
}

/// The instance's only administrator does not leave its own organization, the only
/// administrator of one that has other people does not leave it, nor its only person,
/// whatever they are in it: nothing changes. The instance comes first, as a refused
/// deletion is told, and then the organization's administrator.
#[tokio::test]
async fn what_stands_in_the_way_of_leaving_changes_nothing() {
    let test = common::setup().await;
    let founder = common::user(&test, "founder").await;
    let mut asked = Vec::new();
    let mut ask = async |user_id: &str, people: &str| {
        let before = (counts(&test).await, test.visibility_version().await);
        let departure = test.leave_organization(user_id).await.expect("asked");
        let after = (counts(&test).await, test.visibility_version().await);
        assert_eq!(after, before, "{people}: nothing changes");
        asked.push((people.to_owned(), departure));
    };

    ask(&founder, "the instance's administrator, alone").await;
    let colleague = common::user(&test, "colleague").await;
    ask(&founder, "the instance's administrator").await;
    // Somebody else runs the server, from outside the organization.
    let operator = common::user(&test.scoped(Tenant::Unset), "operator").await;
    administers_the_instance(&test, &operator).await;
    ask(&founder, "the only administrator").await;
    assert!(matches!(
        test.leave_organization(&colleague).await.expect("asked"),
        Departure::Left(_)
    ));
    ask(&founder, "the only person, its administrator").await;
    set_org_role(&test, &test.org_id, &founder, OrgRole::Member).await;
    ask(&founder, "the only person, a member").await;

    let refused = |reason| Departure::Refused(reason);
    let told: Vec<(&str, &Departure)> = asked
        .iter()
        .map(|(people, departure)| (people.as_str(), departure))
        .collect();
    assert_eq!(
        told,
        [
            (
                "the instance's administrator, alone",
                &refused(DepartureRefused::LastInstanceAdmin)
            ),
            (
                "the instance's administrator",
                &refused(DepartureRefused::LastInstanceAdmin)
            ),
            (
                "the only administrator",
                &refused(DepartureRefused::SoleAdmin)
            ),
            (
                "the only person, its administrator",
                &refused(DepartureRefused::Alone)
            ),
            (
                "the only person, a member",
                &refused(DepartureRefused::Alone)
            ),
        ]
    );
    test.teardown().await;
}

/// Of two administrators who leave an organization that has other people in it at
/// once, one leaves and the other, asked once the first is out, is its only
/// administrator and stays. On PostgreSQL the first is stopped as it leaves the
/// organization, the second is started and seen to wait for it, and then the first goes
/// on; on SQLite, whose writers take turns, the two run one after the other.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn of_two_administrators_who_leave_at_once_one_stays() {
    let test = common::setup().await;
    let first = common::user(&test, "first").await;
    let second = common::user(&test, "second").await;
    common::user(&test, "member").await;
    set_org_role(&test, &test.org_id, &second, OrgRole::Admin).await;
    // Neither is the instance's only administrator: what is asked is the organization's.
    let operator = common::user(&test.scoped(Tenant::Unset), "operator").await;
    administers_the_instance(&test, &operator).await;

    let (first_left, second_left) = if common::db_type() == DbType::Postgres {
        let mut gate = Gate::before(
            &test,
            "DELETE",
            "organization_members",
            &format!("OLD.user_id = '{first}'"),
        )
        .await;
        let leaver = test.store.clone();
        let other = gate.second_store(&test).await;
        let who = first.clone();
        let leaving = tokio::spawn(async move { leaver.leave_organization(&who).await });
        let held = gate.stopped(&leaving).await;
        let who = second.clone();
        let also = tokio::spawn(async move { other.leave_organization(&who).await });
        gate.waits_for(held, &also).await;
        gate.open().await;
        (ended(leaving).await, ended(also).await)
    } else {
        (
            test.leave_organization(&first).await,
            test.leave_organization(&second).await,
        )
    };

    assert!(
        matches!(first_left.expect("asked"), Departure::Left(_)),
        "the first leaves"
    );
    assert_eq!(
        second_left.expect("asked"),
        Departure::Refused(DepartureRefused::SoleAdmin),
        "the second is its only administrator once the first is out"
    );
    let admins = one(
        &test,
        &format!(
            "SELECT CAST(count(*) AS TEXT) FROM organization_members \
             WHERE org_id = '{}' AND role = 'admin'",
            test.org_id
        ),
    )
    .await;
    assert_eq!(admins, "1", "the organization keeps an administrator");
    test.teardown().await;
}

/// Who is no member of the organization has nothing to leave, a guest of one of its
/// workspaces neither, and nobody leaves an own space: nothing changes.
#[tokio::test]
async fn nobody_leaves_what_they_are_no_member_of() {
    let test = common::setup().await;
    common::user(&test, "founder").await;
    let unset = test.scoped(Tenant::Unset);
    let stranger = common::user(&unset, "stranger").await;
    let guest = common::user(&unset, "guest").await;
    test.add_workspace_member(&test.workspace_id, &guest, WorkspaceRole::Guest)
        .await
        .expect("a guest");
    let space = one(
        &test,
        &format!("SELECT CAST(id AS TEXT) FROM organizations WHERE owner_id = '{stranger}'"),
    )
    .await;
    let space_store = test.for_org(&space).expect("an organization");
    let before = counts(&test).await;

    let answers = [
        test.leave_organization(&stranger).await.expect("asked"),
        test.leave_organization(&guest).await.expect("asked"),
        space_store
            .leave_organization(&stranger)
            .await
            .expect("asked"),
    ];

    assert_eq!(
        answers,
        [
            Departure::NotAMember,
            Departure::NotAMember,
            Departure::NotAMember
        ]
    );
    assert_eq!(counts(&test).await, before, "nothing changes");
    test.teardown().await;
}
