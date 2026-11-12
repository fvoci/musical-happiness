//! Card relations: a property of the `type` `relation` whose value names cards of the
//! same board. Every way of writing a card checks what a value names; a card in the
//! trash may be named, and a card of a board the writer cannot see is answered as one
//! that is not there.

use fvoci_model::{Block, Board, BoardType, Card, new_id};
use reqwest::StatusCode;
use serde_json::{Value, json};

use crate::common::{ApiResponse, Client, Setup, TEST_WORKSPACE_ID, TestServer};

const RELATION: &str = "rel";

/// Where a card patch's property values are.
const AT: &str = "/updatedProperties";

/// What a value that names a card not on the board is refused with.
const REASON: &str = "relation_card_not_found";

/// A private board of the first user with a relation property and a text property, and
/// cards on it.
struct Related {
    board: Board,
    cards: Vec<Card>,
}

async fn related_board(client: &Client, cards: usize) -> Related {
    let board: Board = client
        .post(
            "/boards",
            &json!({
                "workspaceId": TEST_WORKSPACE_ID, "type": "P", "title": "Related",
                "cardProperties": [
                    {"id": RELATION, "name": "After", "type": "relation", "options": []},
                    {"id": "note", "name": "Note", "type": "text", "options": []},
                ],
            }),
        )
        .await
        .created_json();
    let mut made = Vec::with_capacity(cards);
    for index in 0..cards {
        made.push(
            client
                .post(
                    &format!("/boards/{}/cards", board.id),
                    &json!({"title": format!("Card {index}")}),
                )
                .await
                .created_json(),
        );
    }
    Related { board, cards: made }
}

async fn patch_relation(client: &Client, card_id: &str, value: Value) -> ApiResponse {
    client
        .patch(
            &format!("/cards/{card_id}"),
            &json!({"updatedProperties": {RELATION: value}}),
        )
        .await
}

async fn relation_of(client: &Client, card_id: &str) -> Value {
    let card: Card = client.get(&format!("/cards/{card_id}")).await.ok_json();
    card.properties
        .get(RELATION)
        .cloned()
        .unwrap_or(Value::Null)
}

/// Checks a refusal of a relation value: 400, the reason, and where in the request.
#[track_caller]
fn check_refused(response: &ApiResponse, reason: &str, place: &str) {
    assert_eq!(
        response.status,
        StatusCode::BAD_REQUEST,
        "{reason}: {}",
        response.text()
    );
    assert_eq!(response.error_reason(), reason, "{}", response.text());
    assert_eq!(
        response.problem_type(),
        format!("/problems/{}", reason.replace('_', "-")),
        "the problem of the reason"
    );
    assert_eq!(response.refused_at(), place, "{reason}");
}

/// The answer without what differs from one request to another.
fn comparable(response: &ApiResponse) -> (StatusCode, Value) {
    let mut body = response.value();
    if let Value::Object(members) = &mut body {
        members.remove("requestId");
        members.remove("instance");
    }
    (response.status, body)
}

/// A card of a board the first user cannot see: the second user's private board.
async fn hidden_card(th: &TestServer) -> String {
    let board: Board = th
        .client2
        .post(
            "/boards",
            &json!({"workspaceId": TEST_WORKSPACE_ID, "type": "P", "title": "Hidden"}),
        )
        .await
        .created_json();
    let card: Card = th
        .client2
        .post(
            &format!("/boards/{}/cards", board.id),
            &json!({"title": "x"}),
        )
        .await
        .created_json();
    card.id
}

/// A view of the board: a block of it that is no card.
async fn view_of(client: &Client, board_id: &str) -> String {
    let created: Vec<Block> = client
        .post(
            &format!("/boards/{board_id}/blocks"),
            &json!([{
                "id": "v", "boardId": board_id, "parentId": board_id, "type": "view",
                "title": "Table", "schema": 1, "createAt": 1, "updateAt": 1,
                "fields": {"viewType": "table"},
            }]),
        )
        .await
        .created_json();
    created[0].id.clone()
}

fn many_ids(count: usize) -> Value {
    Value::Array((0..count).map(|_| json!(new_id())).collect())
}

#[tokio::test]
async fn a_relation_names_other_cards_of_the_board_and_the_rest_is_refused_by_name() {
    let th = Setup::native().start().await.init_basic().await;
    let Related { board, cards } = related_board(&th.client, 3).await;
    let (first, second, third) = (&cards[0].id, &cards[1].id, &cards[2].id);
    let hidden = hidden_card(&th).await;
    let view = view_of(&th.client, &board.id).await;

    let cases = [
        ("a text", json!(second), "relation_not_a_list"),
        (
            "the card itself",
            json!([second, first]),
            "relation_to_itself",
        ),
        (
            "a card twice",
            json!([second, third, second]),
            "relation_repeated",
        ),
        ("51 cards", many_ids(51), "relation_too_many"),
        ("a view", json!([view]), "relation_not_a_card"),
        (
            "no card at all",
            json!([new_id()]),
            "relation_card_not_found",
        ),
        (
            "not an identifier",
            json!(["c1"]),
            "relation_card_not_found",
        ),
        (
            "a card of a hidden board",
            json!([hidden]),
            "relation_card_not_found",
        ),
    ];
    for (what, value, reason) in cases {
        let response = patch_relation(&th.client, first, value).await;
        check_refused(&response, reason, AT);
        assert!(
            response.error_message().contains(&format!("{RELATION:?}")),
            "{what}: the words name the property: {}",
            response.text()
        );
    }
    let limit = patch_relation(&th.client, first, many_ids(51)).await;
    assert_eq!(limit.value()["limit"], json!(50), "the limit is told");
    assert_eq!(
        relation_of(&th.client, first).await,
        Value::Null,
        "nothing was stored"
    );

    let stored: Card = patch_relation(&th.client, first, json!([third, second]))
        .await
        .ok_json();
    assert_eq!(
        stored.properties[RELATION],
        json!([third, second]),
        "in its order"
    );
    patch_relation(&th.client, first, Value::Null)
        .await
        .check_ok();
    assert_eq!(
        relation_of(&th.client, first).await,
        Value::Null,
        "null clears it"
    );
    th.teardown().await;
}

#[tokio::test]
async fn a_card_of_a_hidden_board_is_answered_as_one_that_is_not_there() {
    let th = Setup::native().start().await.init_basic().await;
    let Related { board, cards } = related_board(&th.client, 1).await;
    let hidden = hidden_card(&th).await;
    // Of the same length, so that the words could not differ by it.
    let none = new_id();

    for path in [format!("/cards/{}", cards[0].id), String::new()] {
        let answer = |id: &str| {
            let value = json!([id]);
            let client = &th.client;
            let path = path.clone();
            let board_id = board.id.clone();
            async move {
                if path.is_empty() {
                    client
                        .post(
                            &format!("/boards/{board_id}/cards"),
                            &json!({"title": "new", "properties": {RELATION: value}}),
                        )
                        .await
                } else {
                    client
                        .patch(&path, &json!({"updatedProperties": {RELATION: value}}))
                        .await
                }
            }
        };
        let of_hidden = answer(&hidden).await;
        let of_none = answer(&none).await;
        assert_eq!(
            of_hidden.status,
            StatusCode::BAD_REQUEST,
            "{}",
            of_hidden.text()
        );
        assert_eq!(
            comparable(&of_hidden),
            comparable(&of_none),
            "the answers are the same: {path}"
        );
    }
    th.teardown().await;
}

#[tokio::test]
async fn a_card_in_the_trash_may_be_named_and_one_gone_for_good_may_not() {
    let th = Setup::native().start().await.init_basic().await;
    let Related { board, cards } = related_board(&th.client, 3).await;
    let (first, deleted, purged) = (&cards[0].id, &cards[1].id, &cards[2].id);
    for card in [deleted, purged] {
        th.client
            .delete(&format!("/boards/{}/blocks/{card}", board.id))
            .await
            .check_no_content();
    }
    patch_relation(&th.client, first, json!([deleted]))
        .await
        .check_ok();
    // Gone for good: the purge removes what was deleted before now.
    th.client
        .post_empty(&format!("/boards/{}/blocks/{deleted}/undelete", board.id))
        .await
        .check_ok();
    th.app
        .store()
        .purge_trash(i64::MAX)
        .await
        .expect("the trash is purged");
    check_refused(
        &patch_relation(&th.client, first, json!([deleted, purged])).await,
        "relation_card_not_found",
        "/updatedProperties",
    );
    assert_eq!(
        relation_of(&th.client, first).await,
        json!([deleted]),
        "the card that came back is named still"
    );
    th.teardown().await;
}

#[tokio::test]
async fn every_way_of_writing_a_card_checks_its_relations() {
    let th = Setup::native().start().await.init_basic().await;
    let Related { board, cards } = related_board(&th.client, 1).await;
    let card = &cards[0].id;
    let hidden = hidden_card(&th).await;
    let refused = json!([hidden]);

    let created = th
        .client
        .post(
            &format!("/boards/{}/cards", board.id),
            &json!({"title": "new", "properties": {RELATION: refused}}),
        )
        .await;
    check_refused(&created, REASON, "/properties");

    let mut block = json!({
        "id": "c", "boardId": board.id, "parentId": board.id, "type": "card", "title": "c",
        "schema": 1, "createAt": 1, "updateAt": 1, "fields": {"properties": {RELATION: refused}},
    });
    let view = json!({
        "id": "v", "boardId": board.id, "parentId": board.id, "type": "view", "title": "v",
        "schema": 1, "createAt": 1, "updateAt": 1, "fields": {},
    });
    let blocks = th
        .client
        .post(
            &format!("/boards/{}/blocks", board.id),
            &json!([view, block]),
        )
        .await;
    check_refused(&blocks, REASON, "/1/fields");

    let patched = th
        .client
        .patch(
            &format!("/boards/{}/blocks/{card}", board.id),
            &json!({"updatedFields": {"properties": {RELATION: refused}}}),
        )
        .await;
    check_refused(&patched, REASON, "/updatedFields");

    let batch = th
        .client
        .patch(
            &format!("/boards/{}/blocks", board.id),
            &json!({
                "blockIds": [card],
                "blockPatches": [{"updatedFields": {"properties": {RELATION: refused}}}],
            }),
        )
        .await;
    check_refused(&batch, REASON, "/blockPatches/0/updatedFields");

    let both = th
        .client
        .patch(
            "/boards-and-blocks",
            &json!({
                "boardIds": [board.id], "boardPatches": [{}],
                "blockIds": [card],
                "blockPatches": [{"updatedFields": {"properties": {RELATION: refused}}}],
            }),
        )
        .await;
    check_refused(&both, REASON, "/blockPatches/0/updatedFields");

    block["boardId"] = json!("b");
    block["parentId"] = json!("b");
    let new_board = th
        .client
        .post(
            "/boards-and-blocks",
            &json!({
                "boards": [{
                    "id": "b", "workspaceId": TEST_WORKSPACE_ID, "type": "P", "title": "new",
                    "cardProperties": [{"id": RELATION, "type": "relation"}],
                }],
                "blocks": [block],
            }),
        )
        .await;
    check_refused(&new_board, REASON, "/blocks/0/fields");

    assert_eq!(
        relation_of(&th.client, card).await,
        Value::Null,
        "nothing was stored"
    );
    th.teardown().await;
}

#[tokio::test]
async fn new_cards_of_one_request_may_name_each_other() {
    let th = Setup::native().start().await.init_basic().await;
    let Related { board, cards } = related_board(&th.client, 1).await;
    let there = &cards[0].id;
    let card = |id: &str, names: Value| {
        json!({
            "id": id, "boardId": board.id, "parentId": board.id, "type": "card", "title": id,
            "schema": 1, "createAt": 1, "updateAt": 1, "fields": {"properties": {RELATION: names}},
        })
    };
    let created: Vec<Block> = th
        .client
        .post(
            &format!("/boards/{}/blocks", board.id),
            &json!([card("a", json!(["b", there])), card("b", json!(["a"]))]),
        )
        .await
        .created_json();
    let (a, b) = (&created[0], &created[1]);
    assert_eq!(a.fields["properties"][RELATION], json!([b.id, there]));
    assert_eq!(b.fields["properties"][RELATION], json!([a.id]));

    let made: Value = th
        .client
        .post(
            "/boards-and-blocks",
            &json!({
                "boards": [{
                    "id": "nb", "workspaceId": TEST_WORKSPACE_ID, "type": "P", "title": "new",
                    "cardProperties": [{"id": RELATION, "type": "relation"}],
                }],
                "blocks": [
                    {"id": "x", "boardId": "nb", "parentId": "nb", "type": "card", "title": "x",
                     "schema": 1, "createAt": 1, "updateAt": 1,
                     "fields": {"properties": {RELATION: ["y"]}}},
                    {"id": "y", "boardId": "nb", "parentId": "nb", "type": "card", "title": "y",
                     "schema": 1, "createAt": 1, "updateAt": 1,
                     "fields": {"properties": {RELATION: ["x"]}}},
                ],
            }),
        )
        .await
        .created_json();
    let blocks = made["blocks"].as_array().expect("blocks");
    assert_eq!(
        blocks[0]["fields"]["properties"][RELATION],
        json!([blocks[1]["id"]])
    );
    assert_eq!(
        blocks[1]["fields"]["properties"][RELATION],
        json!([blocks[0]["id"]])
    );
    th.teardown().await;
}

#[tokio::test]
async fn a_value_stored_before_the_rules_does_not_stop_other_changes() {
    let th = Setup::native().start().await.init_basic().await;
    let Related { cards, .. } = related_board(&th.client, 2).await;
    let (card, other) = (&cards[0].id, &cards[1].id);
    let missing = new_id();
    let mut stale = vec![json!(card), json!(missing), json!(missing)];
    stale.extend((0..50).map(|_| json!(new_id())));
    let fields = json!({"properties": {RELATION: stale, "note": "before"}});
    th.app
        .store()
        .set_block_fields_unchecked(card, fields.as_object().expect("an object"))
        .await
        .expect("written as an older server would have");

    let patched: Card = th
        .client
        .patch(
            &format!("/cards/{card}"),
            &json!({"title": "renamed", "updatedProperties": {"note": "after"}}),
        )
        .await
        .ok_json();
    assert_eq!(patched.properties[RELATION], json!(stale), "left as it was");
    assert_eq!(patched.properties["note"], json!("after"));

    // Written, it is checked; what it named before is not asked for again.
    check_refused(
        &patch_relation(&th.client, card, json!([missing, missing, other])).await,
        "relation_repeated",
        "/updatedProperties",
    );
    // Over the limit it may lose cards, one at a time, and not take in another.
    check_refused(
        &patch_relation(
            &th.client,
            card,
            json!([&stale[3..], &[json!(other)]].concat()),
        )
        .await,
        "relation_too_many",
        "/updatedProperties",
    );
    patch_relation(&th.client, card, json!(stale[2..]))
        .await
        .check_ok();
    patch_relation(&th.client, card, json!([missing, other]))
        .await
        .check_ok();
    check_refused(
        &patch_relation(&th.client, card, json!([missing, other, new_id()])).await,
        "relation_card_not_found",
        "/updatedProperties",
    );
    th.teardown().await;
}

#[tokio::test]
async fn a_change_of_the_property_kind_leaves_the_values_alone() {
    let th = Setup::native().start().await.init_basic().await;
    let Related { board, cards } = related_board(&th.client, 2).await;
    let (card, other) = (&cards[0].id, &cards[1].id);
    patch_relation(&th.client, card, json!([other]))
        .await
        .check_ok();
    let path = format!("/boards/{}", board.id);
    for kind in ["text", "relation"] {
        th.client
            .patch(
                &path,
                &json!({"updatedCardProperties": [{"id": RELATION, "name": "After", "type": kind}]}),
            )
            .await
            .check_ok();
        assert_eq!(
            relation_of(&th.client, card).await,
            json!([other]),
            "as {kind}"
        );
    }
    th.teardown().await;
}

/// A relation names cards of its own card's board, also where the writer sees another.
#[tokio::test]
async fn the_board_of_a_relation_is_the_board_of_the_card() {
    let th = Setup::native().start().await.init_basic().await;
    let Related { cards, .. } = related_board(&th.client, 1).await;
    let other = th.create_board(TEST_WORKSPACE_ID, BoardType::Private).await;
    th.client
        .patch(
            &format!("/boards/{}", other.id),
            &json!({"updatedCardProperties": [{"id": RELATION, "type": "relation"}]}),
        )
        .await
        .check_ok();
    let card: Card = th
        .client
        .post(
            &format!("/boards/{}/cards", other.id),
            &json!({"title": "x"}),
        )
        .await
        .created_json();
    // The user sees both boards; a card of one is no card of the other.
    check_refused(
        &patch_relation(&th.client, &card.id, json!([cards[0].id])).await,
        "relation_card_not_found",
        "/updatedProperties",
    );
    th.teardown().await;
}
