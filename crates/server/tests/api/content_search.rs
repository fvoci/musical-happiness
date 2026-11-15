//! The search of what cards say (`searchWorkspaceContent`), against a faked search
//! server: what it answers is checked against the database one by one, so an index
//! that is behind what is stored shows nobody what they do not read now.

use std::time::{Duration, Instant};

use fvoci_model::{Block, BlockType, Board, BoardMember, BoardType, Card, SearchHit, new_id};
use fvoci_store::DbType;
use fvoci_store::testing::db_type_of_the_tests;
use reqwest::StatusCode;
use serde_json::{Value, json};

use crate::boards::{delete_member, editor, member, new_board};
use crate::common::{
    ApiResponse, Client, HIDDEN_WORKSPACE_ID, ROOT_WS, Setup, TEST_WORKSPACE_ID, TEST_WS,
    TestServer, UNKNOWN_ID,
};
use crate::fake_search::{FakeSearch, Flavor, document};
use crate::workspaces::another_organization;

/// The name of the index a server makes unless it is told another.
pub(crate) const INDEX: &str = "fvoci";

/// A server that searches through `fake`, with its two users, once it has seen to
/// its index there.
pub(crate) async fn started(fake: &FakeSearch) -> TestServer {
    let th = Setup::native()
        .search(fake.settings())
        .start()
        .await
        .init_basic()
        .await;
    fvoci_server::search::tend(&th.app).await;
    th
}

fn body(text: &str) -> Value {
    json!({"type": "doc", "content": [
        {"type": "paragraph", "content": [{"type": "text", "text": text}]},
    ]})
}

/// A card made through the API, with a body of one paragraph.
pub(crate) async fn new_card(client: &Client, board_id: &str, title: &str, text: &str) -> Card {
    client
        .post(
            &format!("{TEST_WS}/boards/{board_id}/cards"),
            &json!({"title": title, "body": body(text)}),
        )
        .await
        .created_json()
}

/// A card stored past the API, on behalf of nobody, on any board.
async fn stored_card(store: &fvoci_store::Store, board_id: &str, title: &str) -> String {
    let mut block = Block {
        id: new_id(),
        board_id: board_id.to_owned(),
        block_type: BlockType::Card,
        title: title.to_owned(),
        ..Block::default()
    };
    store
        .insert_block(&mut block, "")
        .await
        .expect("the card is stored");
    block.id
}

/// A value as a query writes it: everything but letters and digits percent-encoded.
fn encoded(value: &str) -> String {
    value
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

/// The search of a workspace with these parameters.
async fn search_with(client: &Client, workspace: &str, parameters: &[(&str, &str)]) -> ApiResponse {
    let query: Vec<String> = parameters
        .iter()
        .map(|(name, value)| format!("{name}={}", encoded(value)))
        .collect();
    client
        .get(&format!("{workspace}/search?{}", query.join("&")))
        .await
}

pub(crate) async fn search(client: &Client, workspace: &str, query: &str) -> ApiResponse {
    search_with(client, workspace, &[("q", query)]).await
}

/// The identifiers a search found, sorted.
fn found(response: &ApiResponse) -> Vec<String> {
    let hits: Vec<SearchHit> = response.ok_json();
    let mut ids: Vec<String> = hits.into_iter().map(|hit| hit.id).collect();
    ids.sort();
    ids
}

fn sorted(ids: &[&str]) -> Vec<String> {
    let mut ids: Vec<String> = ids.iter().map(|id| (*id).to_owned()).collect();
    ids.sort();
    ids
}

/// The organization of the test server.
fn org_of(th: &TestServer) -> String {
    th.app
        .store()
        .tenant()
        .org_id()
        .expect("the test server's organization")
}

/// Plants the document of a card as the server would index it now: the search server
/// then answers every search with it.
async fn plant_current(th: &TestServer, fake: &FakeSearch, card_id: &str) {
    let stored = th
        .app
        .store()
        .search_documents(&[card_id.to_owned()])
        .await
        .expect("the document is read");
    let stored = stored.first().expect("the card has a document");
    fake.plant(
        card_id,
        document(
            &stored.org_id,
            &stored.workspace_id,
            &stored.container_id,
            &stored.title,
            &stored.text,
        ),
    );
}

/// Plants a document that claims to be of the test workspace and of `board_id`,
/// whatever the database says of the identifier.
fn plant_claiming(th: &TestServer, fake: &FakeSearch, id: &str, board_id: &str) {
    fake.plant(
        id,
        document(
            &org_of(th),
            TEST_WORKSPACE_ID,
            board_id,
            "환불 규정",
            "환불규정을 따른다",
        ),
    );
}

#[tokio::test]
async fn without_a_search_server_bodies_are_not_searched() {
    let th = Setup::native().start().await.init_basic().await;
    fvoci_server::search::tend(&th.app).await;

    let config = th.anonymous().get("/client-config").await.value();
    let answer = search(&th.client, TEST_WS, "환불").await;
    let elsewhere = search(&th.client, &format!("/w/{UNKNOWN_ID}"), "환불").await;
    th.teardown().await;

    assert_eq!(config["searchEnabled"], false, "{config}");
    answer.check_status(StatusCode::NOT_FOUND);
    let problem = answer.check_problem();
    assert_eq!(problem["reason"], "search_not_configured", "{problem}");
    assert_eq!(problem["type"], "/problems/search-not-configured");
    // A workspace that is not the user's is not there, as for any other route.
    elsewhere.check_status(StatusCode::NOT_FOUND);
    assert_eq!(elsewhere.check_problem().get("reason"), None);
}

#[tokio::test]
async fn a_search_answers_a_card_as_the_database_has_it() {
    let fake = FakeSearch::start().await;
    fake.require_authorization("Basic ZnZvY2k6c2VjcmV0LXBhc3M=");
    let mut settings = fake.settings();
    "fvoci".clone_into(&mut settings.search_username);
    "secret-pass".clone_into(&mut settings.search_password);
    let th = Setup::native()
        .search(settings)
        .start()
        .await
        .init_basic()
        .await;
    fvoci_server::search::tend(&th.app).await;
    let board: Board = th
        .client
        .create_board(&new_board(TEST_WORKSPACE_ID, BoardType::Open, "Support"))
        .await
        .created_json();
    let card = new_card(&th.client, &board.id, "고객 응대", "환불규정을 따른다").await;
    // The index holds the card under a title it had before.
    fake.plant(
        &card.id,
        document(
            &org_of(&th),
            TEST_WORKSPACE_ID,
            &board.id,
            "옛 제목",
            "환불규정을 따른다",
        ),
    );
    let stale = search(&th.client, TEST_WS, "환불").await;
    fake.clear_planted();
    plant_current(&th, &fake, &card.id).await;
    plant_current(&th, &fake, &card.id).await;

    let config = th.anonymous().get("/client-config").await.value();
    let answer = search(&th.client, TEST_WS, "환불").await;
    let empty = search(&th.client, TEST_WS, "  ").await;
    let searches = fake.recorded_at("/_search");
    let definition = fake.index_definition(INDEX);
    th.teardown().await;

    assert_eq!(config["searchEnabled"], true, "{config}");
    let stale: Vec<SearchHit> = stale.ok_json();
    assert_eq!(stale.len(), 1, "{stale:?}");
    assert_eq!(stale[0].title, "고객 응대", "the title is the database's");
    assert!(
        stale[0].snippet.is_empty(),
        "what was indexed from another title and text is not shown: {stale:?}"
    );
    let hits: Vec<SearchHit> = answer.ok_json();
    assert_eq!(hits.len(), 1, "the card once, planted twice: {hits:?}");
    assert_eq!(hits[0].id, card.id);
    assert_eq!(hits[0].container_id, board.id);
    assert_eq!(hits[0].title, "고객 응대");
    let snippet: Vec<(&str, bool)> = hits[0]
        .snippet
        .iter()
        .map(|part| (part.text.as_str(), part.matched))
        .collect();
    assert_eq!(snippet, [("환불", true), ("규정을 따른다", false)]);
    assert_eq!(
        empty.ok_json::<Vec<SearchHit>>(),
        [],
        "nothing is asked for"
    );

    assert_eq!(searches.len(), 2, "an empty search asks nothing");
    let filter = &searches[0].body["query"]["bool"]["filter"];
    assert_eq!(filter[0], json!({"term": {"orgId": stale_org(filter)}}));
    assert_eq!(
        filter[1],
        json!({"term": {"workspaceId": TEST_WORKSPACE_ID}})
    );
    assert_eq!(filter[2], json!({"terms": {"containerId": [board.id]}}));
    assert_eq!(searches[0].body["track_total_hits"], false);
    assert_eq!(searches[0].body["_source"], json!(["digest"]));
    let definition = definition.expect("the server made its index");
    assert_eq!(
        definition["settings"]["analysis"]["tokenizer"]["fvoci_nori"]["type"],
        "nori_tokenizer"
    );
    assert_eq!(definition["mappings"]["_meta"]["fvoci"]["version"], 4);
}

/// The organization a filter names: some identifier, and not none.
fn stale_org(filter: &Value) -> String {
    let org = filter[0]["term"]["orgId"]
        .as_str()
        .expect("an organization");
    assert!(fvoci_model::is_id(org), "{org}");
    org.to_owned()
}

/// A private board of the test workspace with the user of `client` as its admin.
async fn private_board(client: &Client, title: &str) -> Board {
    client
        .create_board(&new_board(TEST_WORKSPACE_ID, BoardType::Private, title))
        .await
        .created_json()
}

/// Makes a user that of a role in the test workspace.
async fn set_role(th: &TestServer, user_id: &str, role: &str) {
    th.app
        .store()
        .execute_unchecked(&format!(
            "UPDATE workspace_members SET role = '{role}' WHERE user_id = '{user_id}' \
             AND workspace_id = '{TEST_WORKSPACE_ID}'"
        ))
        .await
        .expect("the user's role in the test workspace is set");
}

/// A private board that `user_id` reads through a team and is no member of, and a
/// card of it: the board's and the card's identifiers.
async fn card_read_through_a_team(th: &TestServer, user_id: &str) -> (String, String) {
    let board = private_board(&th.client, "Teamed").await;
    let team: fvoci_model::Team = th
        .client
        .post(&format!("{TEST_WS}/teams"), &json!({"name": "Support"}))
        .await
        .ok_json();
    th.client
        .put(
            &format!("{TEST_WS}/teams/{}/members/{user_id}", team.id),
            &json!({}),
        )
        .await
        .check_no_content();
    th.client
        .put(
            &format!("{TEST_WS}/boards/{}/teams/{}", board.id, team.id),
            &json!({"role": "viewer"}),
        )
        .await
        .check_no_content();
    let card = new_card(&th.client, &board.id, "팀의 카드", "환불").await;
    (board.id, card.id)
}

/// A board that went to the trash whole, and a card of it.
async fn card_of_a_board_in_the_trash(th: &TestServer) -> (String, String) {
    let board = private_board(&th.client, "Gone").await;
    let card = new_card(&th.client, &board.id, "지운 보드의 카드", "환불").await;
    th.client
        .delete(&format!("{TEST_WS}/boards/{}", board.id))
        .await
        .check_no_content();
    (board.id, card.id)
}

/// Two cards of a board that were deleted: one in the trash, and one that went from
/// there for good.
async fn cards_that_went(th: &TestServer, board_id: &str) -> (String, String) {
    let trashed = new_card(&th.client, board_id, "지운 카드", "환불").await;
    let purged = new_card(&th.client, board_id, "사라진 카드", "환불").await;
    for card in [&trashed, &purged] {
        th.client
            .delete(&format!("{TEST_WS}/boards/{board_id}/blocks/{}", card.id))
            .await
            .check_no_content();
    }
    // The other card of the trash stays: only the one is removed for good.
    th.app
        .store()
        .execute_unchecked(&format!(
            "DELETE FROM deleted_blocks WHERE id = '{}'",
            purged.id
        ))
        .await
        .expect("the card is gone for good");
    (trashed.id, purged.id)
}

/// What is stored and has no document for a searcher of the test workspace: a card of
/// a workspace they are not in, a card of a template, and a comment on `card_id` of the
/// board `open`.
async fn blocks_without_a_document(
    th: &TestServer,
    open: &Board,
    card_id: &str,
    author: &str,
) -> Vec<String> {
    let store = th.app.store();
    let hidden_owner = th.stranger_in(HIDDEN_WORKSPACE_ID).await;
    let hidden = crate::boards::board_with_admin(
        th,
        new_board(HIDDEN_WORKSPACE_ID, BoardType::Open, "Hidden"),
        &hidden_owner,
    )
    .await;
    let hidden_card = stored_card(store, &hidden.id, "다른 워크스페이스").await;
    let template: Board = th
        .client
        .create_board(&Board {
            is_template: true,
            ..new_board(TEST_WORKSPACE_ID, BoardType::Open, "Template")
        })
        .await
        .created_json();
    let template_card = new_card(&th.client, &template.id, "템플릿 카드", "환불").await;
    let mut comment = Block {
        id: new_id(),
        board_id: open.id.clone(),
        parent_id: card_id.to_owned(),
        block_type: BlockType::Comment,
        title: "환불 댓글".to_owned(),
        ..Block::default()
    };
    store
        .insert_block(&mut comment, author)
        .await
        .expect("the comment is stored");
    vec![hidden_card, template_card.id, comment.id]
}

/// What the index holds of each of these, an honest index would not answer the
/// searcher with: the search server here answers every search with all of them.
#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "one index that is wrong in every way, and each reader of it"
)]
async fn a_stale_index_shows_nobody_what_they_do_not_read_now() {
    let fake = FakeSearch::start().await;
    let th = started(&fake).await;
    let user1 = th.client.me().await.id;
    let user2 = th.client2.me().await.id;
    let guest_client = th.add_user("guest").await;
    let guest = guest_client.me().await.id;
    set_role(&th, &guest, "guest").await;
    let stranger = th.stranger_in(TEST_WORKSPACE_ID).await;
    let store = th.app.store();
    // The first user administers the test workspace: they manage every board of it,
    // and read those they have a role on.
    set_role(&th, &user1, "admin").await;
    // A board of the first user in the root workspace: a search there asks the index.
    th.create_board(crate::common::ROOT_WORKSPACE_ID, BoardType::Private)
        .await;

    // An open board, and a card of it that every member reads.
    let open: Board = th
        .client
        .create_board(&new_board(TEST_WORKSPACE_ID, BoardType::Open, "Open"))
        .await
        .created_json();
    let open_card = new_card(&th.client, &open.id, "열린 카드", "환불규정을 따른다").await;
    th.client2
        .post_empty(&format!("{TEST_WS}/boards/{}/join", open.id))
        .await
        .check_ok();
    // An open board the second user has not joined: theirs to join, and not to read.
    let unjoined: Board = th
        .client
        .create_board(&new_board(TEST_WORKSPACE_ID, BoardType::Open, "Unjoined"))
        .await
        .created_json();
    let unjoined_card = new_card(&th.client, &unjoined.id, "남의 열린 카드", "환불").await;
    // A private board of the first user alone.
    let private = private_board(&th.client, "Private").await;
    let private_card = new_card(&th.client, &private.id, "비공개 카드", "환불").await;
    // A private board the first user, an administrator of the workspace, is not on.
    let managed = crate::boards::board_with_admin(
        &th,
        new_board(TEST_WORKSPACE_ID, BoardType::Private, "Managed"),
        &stranger,
    )
    .await;
    let managed_card = stored_card(store, &managed.id, "남의 카드").await;
    // A private board the guest was invited to.
    let invited = private_board(&th.client, "Invited").await;
    th.client
        .add_member(&BoardMember {
            scheme_viewer: true,
            ..member(&invited.id, &guest)
        })
        .await
        .check_ok();
    let invited_card = new_card(&th.client, &invited.id, "초대된 카드", "환불").await;
    // A private board the second user was on when it was indexed, and is not now.
    let left = private_board(&th.client, "Left").await;
    th.client
        .add_member(&editor(&left.id, &user2))
        .await
        .check_ok();
    let left_card = new_card(&th.client, &left.id, "떠난 카드", "환불").await;
    delete_member(&th.client, &member(&left.id, &user2))
        .await
        .check_no_content();
    let (teamed, teamed_card) = card_read_through_a_team(&th, &user2).await;
    let (gone, gone_card) = card_of_a_board_in_the_trash(&th).await;
    let (trashed, purged) = cards_that_went(&th, &open.id).await;
    let no_documents = blocks_without_a_document(&th, &open, &open_card.id, &user1).await;

    let mut planted = vec![
        (open_card.id.clone(), open.id.clone()),
        // Twice, and once more as of a board it is not on.
        (open_card.id.clone(), open.id.clone()),
        (unjoined_card.id.clone(), unjoined.id.clone()),
        (private_card.id.clone(), private.id.clone()),
        (private_card.id.clone(), open.id.clone()),
        (managed_card.clone(), managed.id.clone()),
        (managed_card.clone(), open.id.clone()),
        (invited_card.id.clone(), invited.id.clone()),
        (left_card.id.clone(), left.id.clone()),
        (teamed_card.clone(), teamed),
        (gone_card.clone(), gone),
        (gone_card, open.id.clone()),
        (trashed, open.id.clone()),
        (purged, open.id.clone()),
        (UNKNOWN_ID.to_owned(), open.id.clone()),
        ("not-an-identifier".to_owned(), open.id.clone()),
        ("' OR 1=1 --".to_owned(), open.id.clone()),
    ];
    planted.extend(no_documents.into_iter().map(|id| (id, open.id.clone())));
    // Another organization's card, where the database holds more than one.
    if db_type_of_the_tests() == DbType::Postgres {
        let (theirs, _, their_board) = another_organization(&th).await;
        let their_card = stored_card(&theirs, &their_board, "남의 조직").await;
        planted.push((their_card, open.id.clone()));
    }
    for (id, board_id) in &planted {
        plant_claiming(&th, &fake, id, board_id);
    }

    let admin = search(&th.client, TEST_WS, "환불").await;
    let member_found = search(&th.client2, TEST_WS, "환불").await;
    let guest_found = search(&guest_client, TEST_WS, "환불").await;
    let in_root = search(&th.client, ROOT_WS, "환불").await;
    let asked = fake.recorded_at("/_search").len();
    th.teardown().await;

    assert_eq!(
        found(&admin),
        sorted(&[
            &open_card.id,
            &unjoined_card.id,
            &private_card.id,
            &invited_card.id,
            &left_card.id,
            &teamed_card
        ]),
        "the first user reads their boards, and no board they only manage"
    );
    assert_eq!(
        found(&member_found),
        sorted(&[&open_card.id, &teamed_card]),
        "the second user reads the open board they joined and a team's board, and no board \
         they left"
    );
    assert_eq!(
        found(&guest_found),
        sorted(&[&invited_card.id]),
        "a guest reads the board they were invited to, and no open board"
    );
    assert_eq!(
        found(&in_root),
        Vec::<String>::new(),
        "nothing of another workspace, whatever the index says"
    );
    // The hits say what the database says: the board the card is on, not the one the
    // index claimed.
    let hits: Vec<SearchHit> = admin.ok_json();
    let private_hit = hits
        .iter()
        .find(|hit| hit.id == private_card.id)
        .expect("found");
    assert_eq!(private_hit.container_id, private.id);
    assert_eq!(private_hit.title, "비공개 카드");
    assert_eq!(asked, 4, "the search server was asked every time");
}

#[tokio::test]
async fn a_search_server_that_is_down_or_slow_is_a_503_and_holds_nothing_else() {
    let fake = FakeSearch::start().await;
    let th = started(&fake).await;
    let board: Board = th
        .client
        .create_board(&new_board(TEST_WORKSPACE_ID, BoardType::Open, "Open"))
        .await
        .created_json();

    fake.go_down();
    let down = search(&th.client, TEST_WS, "환불").await;
    let ready = th.anonymous().get("/ready").await;
    // Writing goes on as ever.
    let written = Instant::now();
    new_card(&th.client, &board.id, "카드", "환불").await;
    let writing_took = written.elapsed();
    fake.come_up();
    fake.set_delay(Duration::from_secs(12));
    let asked = Instant::now();
    let slow = search(&th.client, TEST_WS, "환불").await;
    let waited = asked.elapsed();
    fake.set_delay(Duration::ZERO);
    let back = search(&th.client, TEST_WS, "환불").await;
    th.teardown().await;

    for refused in [&down, &slow] {
        refused.check_status(StatusCode::SERVICE_UNAVAILABLE);
        let problem = refused.check_problem();
        assert_eq!(problem["reason"], "search_unavailable", "{problem}");
        assert_eq!(problem["type"], "/problems/search-unavailable");
    }
    ready.check_ok();
    assert!(writing_took < Duration::from_secs(5), "{writing_took:?}");
    // Three seconds, and not the twelve the search server takes to answer.
    assert!(
        waited >= Duration::from_secs(3) && waited < Duration::from_secs(10),
        "a search waits three seconds for the search server: {waited:?}"
    );
    back.check_ok();
}

#[tokio::test]
async fn a_search_server_without_nori_leaves_the_server_up_and_the_search_off() {
    let fake = FakeSearch::start_as(Flavor::Elasticsearch).await;
    fake.without_nori();
    let th = started(&fake).await;

    let config = th.anonymous().get("/client-config").await.value();
    let answer = search(&th.client, TEST_WS, "환불").await;
    let me = th.client.get("/users/me").await;
    let made = fake.index_definition(INDEX);
    th.teardown().await;

    assert_eq!(config["searchEnabled"], false, "{config}");
    answer.check_status(StatusCode::NOT_FOUND);
    assert_eq!(answer.check_problem()["reason"], "search_not_configured");
    me.check_ok();
    assert_eq!(made, None, "no index without the analyser");
}

#[tokio::test]
async fn an_index_the_server_did_not_make_is_left_as_it_is() {
    let fake = FakeSearch::start().await;
    let theirs = json!({"mappings": {"properties": {"title": {"type": "text"}}}});
    fake.put_index(INDEX, theirs.clone());
    let th = started(&fake).await;

    let config = th.anonymous().get("/client-config").await.value();
    let answer = search(&th.client, TEST_WS, "환불").await;
    let left = fake.index_definition(INDEX);
    let requests = fake.recorded();
    th.teardown().await;

    assert_eq!(config["searchEnabled"], false, "{config}");
    answer.check_status(StatusCode::NOT_FOUND);
    assert_eq!(left, Some(theirs), "the index is as it was");
    assert!(
        requests.iter().all(|request| request.method == "GET"),
        "nothing was written or removed: {requests:?}"
    );
}

#[tokio::test]
async fn an_index_made_from_another_definition_is_made_again() {
    let fake = FakeSearch::start().await;
    fake.put_index(
        INDEX,
        json!({"mappings": {"_meta": {"fvoci": {"version": 1}}, "properties": {}}}),
    );
    let th = started(&fake).await;

    let config = th.anonymous().get("/client-config").await.value();
    let definition = fake.index_definition(INDEX).expect("an index");
    let requests: Vec<(String, String)> = fake
        .recorded()
        .into_iter()
        .map(|request| (request.method, request.path))
        .collect();
    th.teardown().await;

    assert_eq!(config["searchEnabled"], true, "{config}");
    assert_eq!(definition["mappings"]["_meta"]["fvoci"]["version"], 4);
    let path = format!("/{INDEX}");
    assert_eq!(
        requests,
        [
            ("GET".to_owned(), path.clone()),
            ("DELETE".to_owned(), path.clone()),
            ("PUT".to_owned(), path.clone()),
            ("GET".to_owned(), path),
        ],
        "described, removed, made, and asked which index it is now"
    );
}

#[tokio::test]
async fn a_search_server_that_comes_up_later_gets_its_index_then() {
    let fake = FakeSearch::start().await;
    fake.go_down();
    let th = started(&fake).await;
    th.create_board(TEST_WORKSPACE_ID, BoardType::Open).await;

    let before = search(&th.client, TEST_WS, "환불").await;
    let config = th.anonymous().get("/client-config").await.value();
    fake.come_up();
    // The upkeep leaves a server that did not answer alone for a second.
    tokio::time::sleep(Duration::from_millis(1100)).await;
    fvoci_server::search::tend(&th.app).await;
    let after = search(&th.client, TEST_WS, "환불").await;
    let made = fake.index_definition(INDEX);
    th.teardown().await;

    before.check_status(StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(config["searchEnabled"], true, "configured, and not refused");
    after.check_ok();
    assert!(
        made.is_some(),
        "the index was made once the server answered"
    );
}

#[tokio::test]
async fn what_a_search_asks_for_is_checked_before_anything_is_asked() {
    let fake = FakeSearch::start().await;
    let th = started(&fake).await;
    let get = |parameters: &'static [(&'static str, &'static str)]| {
        search_with(&th.client, TEST_WS, parameters)
    };

    let too_long = "가".repeat(201);
    let long = search(&th.client, TEST_WS, &too_long).await;
    let longest = search(&th.client, TEST_WS, &"가".repeat(200)).await;
    let kind = get(&[("q", "x"), ("kind", "comment")]).await;
    let card = get(&[("q", "x"), ("kind", "card")]).await;
    let limits = [
        get(&[("q", "x"), ("limit", "0")]).await,
        get(&[("q", "x"), ("limit", "51")]).await,
        get(&[("q", "x"), ("limit", "-1")]).await,
        get(&[("q", "x"), ("limit", "many")]).await,
    ];
    let most = get(&[("q", "x"), ("limit", "50")]).await;
    let anonymous = search(&th.anonymous(), TEST_WS, "x").await;
    let stranger = th.add_user("outsider").await;
    th.app
        .store()
        .remove_workspace_member(TEST_WORKSPACE_ID, &stranger.me().await.id, "")
        .await
        .expect("the user leaves the test workspace");
    let outside = search(&stranger, TEST_WS, "x").await;
    let asked = fake.recorded_at("/_search");
    th.teardown().await;

    long.check_bad_request();
    assert_eq!(long.check_problem()["errors"][0]["parameter"], "q");
    longest.check_ok();
    kind.check_bad_request();
    assert_eq!(kind.check_problem()["errors"][0]["parameter"], "kind");
    card.check_ok();
    for limit in &limits {
        limit.check_bad_request();
        assert_eq!(limit.check_problem()["errors"][0]["parameter"], "limit");
    }
    most.check_ok();
    anonymous.check_unauthorized();
    outside.check_not_found();
    // A user with no board to read asks the search server nothing either.
    assert_eq!(asked.len(), 0, "{asked:?}");
}

#[tokio::test]
async fn a_user_searches_only_so_often() {
    let fake = FakeSearch::start().await;
    let mut settings = fake.settings();
    settings.search_requests_per_second = 1;
    let th = Setup::native()
        .search(settings)
        .start()
        .await
        .init_basic()
        .await;
    fvoci_server::search::tend(&th.app).await;

    // One a second, and two at once after a pause: of eight in a row, the first is
    // answered and some are not, however slowly the machine gets through them.
    let mut answers = Vec::new();
    for _ in 0..8 {
        answers.push(search(&th.client, TEST_WS, "환불").await);
    }
    let other = search(&th.client2, TEST_WS, "환불").await;
    th.teardown().await;

    answers[0].check_ok();
    let limited: Vec<&ApiResponse> = answers
        .iter()
        .filter(|answer| answer.status == StatusCode::TOO_MANY_REQUESTS)
        .collect();
    assert!(!limited.is_empty(), "some were told to wait");
    assert_eq!(limited[0].header("retry-after"), "1");
    assert!(
        answers.iter().all(|answer| answer.status == StatusCode::OK
            || answer.status == StatusCode::TOO_MANY_REQUESTS),
        "answered, or told to wait"
    );
    other.check_ok();
}

#[tokio::test]
async fn a_user_of_more_boards_than_a_request_lists_is_searched_for_in_the_workspace() {
    let fake = FakeSearch::start().await;
    let th = started(&fake).await;
    let user1 = th.client.me().await.id;
    let mut last = String::new();
    for number in 0..1001 {
        let mut board = new_board(TEST_WORKSPACE_ID, BoardType::Private, &format!("b{number}"));
        board.id = new_id();
        th.app
            .store()
            .insert_board_with_admin(&mut board, &user1)
            .await
            .expect("a board is stored");
        last = board.id;
    }
    let mine = new_card(&th.client, &last, "내 카드", "환불").await;
    plant_current(&th, &fake, &mine.id).await;
    // Of the same workspace, on a board the user is not on.
    let stranger = th.stranger_in(TEST_WORKSPACE_ID).await;
    let theirs = crate::boards::board_with_admin(
        &th,
        new_board(TEST_WORKSPACE_ID, BoardType::Private, "Theirs"),
        &stranger,
    )
    .await;
    let their_card = stored_card(th.app.store(), &theirs.id, "남의 카드").await;
    plant_claiming(&th, &fake, &their_card, &theirs.id);

    let answer = search_with(&th.client, TEST_WS, &[("q", "환불"), ("limit", "7")]).await;
    let asked = fake.recorded_at("/_search");
    th.teardown().await;

    assert_eq!(found(&answer), [mine.id]);
    let filter = asked[0].body["query"]["bool"]["filter"]
        .as_array()
        .expect("a filter");
    assert_eq!(
        filter.len(),
        2,
        "the organization and the workspace: {filter:?}"
    );
    assert_eq!(
        filter[1],
        json!({"term": {"workspaceId": TEST_WORKSPACE_ID}})
    );
    assert_eq!(asked[0].body["size"], 35, "five times what was asked for");
}

#[tokio::test]
async fn an_index_that_went_is_made_again_and_a_partial_answer_is_none() {
    let fake = FakeSearch::start().await;
    let th = started(&fake).await;
    th.create_board(TEST_WORKSPACE_ID, BoardType::Open).await;

    fake.remove_index(INDEX);
    let gone = search(&th.client, TEST_WS, "환불").await;
    fvoci_server::search::tend(&th.app).await;
    let back = search(&th.client, TEST_WS, "환불").await;
    let made = fake.index_definition(INDEX);
    fake.answer_in_part();
    let partial = search(&th.client, TEST_WS, "환불").await;
    th.teardown().await;

    gone.check_status(StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(gone.check_problem()["reason"], "search_unavailable");
    back.check_ok();
    assert!(made.is_some(), "the upkeep made the index again");
    partial.check_status(StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn an_index_a_newer_server_made_is_left_as_it_is() {
    let fake = FakeSearch::start().await;
    let theirs = json!({"mappings": {"_meta": {"fvoci": {"version": 99}}, "properties": {}}});
    fake.put_index(INDEX, theirs.clone());
    let th = started(&fake).await;

    let config = th.anonymous().get("/client-config").await.value();
    let left = fake.index_definition(INDEX);
    let requests = fake.recorded();
    th.teardown().await;

    assert_eq!(config["searchEnabled"], false, "{config}");
    assert_eq!(left, Some(theirs), "the index is as it was");
    assert!(
        requests.iter().all(|request| request.method == "GET"),
        "nothing was written or removed: {requests:?}"
    );
}
