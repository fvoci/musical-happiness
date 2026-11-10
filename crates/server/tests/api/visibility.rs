//! What a user cannot see answers as what does not exist.
//!
//! For every route that names a board, a block, a card, a file, a category or a
//! subscription, a user who has no business with it gets the same answer as for an
//! identifier nobody has: the same status, the same code, the same message. Nothing in
//! an answer tells the two apart, so nothing is learned about what others have.

use fvoci_model::{Block, BlockType, BoardType, Category, IdKind, new_id};
use reqwest::multipart::{Form, Part};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

use crate::common::{ApiResponse, Client, Setup, TEST_TEAM_ID, TestServer};

/// What exists and is hidden from the second user, and what stands in for it in the
/// request that names nothing.
struct Hidden {
    th: TestServer,
    /// The identifiers of what the first user has, each with one nobody has.
    board: [String; 2],
    card: [String; 2],
    text: [String; 2],
    file: [String; 2],
    category: [String; 2],
    owner: [String; 2],
    /// The second user, who sees none of it.
    outsider: String,
}

impl Hidden {
    async fn start() -> Self {
        let th = Setup::local_mode().start().await.init_basic().await;
        let owner = th.client.me().await;
        let outsider = th.client2.me().await.id;
        let board = th.create_board(TEST_TEAM_ID, BoardType::Private).await;
        let block = |block_type: BlockType, parent_id: &str| Block {
            id: new_id(IdKind::Block),
            board_id: board.id.clone(),
            parent_id: parent_id.to_owned(),
            block_type,
            create_at: 1,
            update_at: 1,
            ..Block::default()
        };
        let mut created: Vec<Block> = th
            .client
            .insert_blocks(&board.id, &[block(BlockType::Card, &board.id)])
            .await
            .ok_json();
        let card = created.pop().expect("the card is created");
        let mut created: Vec<Block> = th
            .client
            .insert_blocks(&board.id, &[block(BlockType::Text, &card.id)])
            .await
            .ok_json();
        let text = created.pop().expect("the text is created");

        let part = Part::bytes(b"hidden".to_vec()).file_name("hidden.txt");
        let request = th
            .client
            .request(
                Method::POST,
                &format!("/teams/{TEST_TEAM_ID}/{}/files", board.id),
            )
            .multipart(Form::new().part("file", part));
        let uploaded = Client::send(request).await;
        uploaded.check_ok();
        let file = uploaded.value()["fileId"]
            .as_str()
            .expect("the upload names the file")
            .to_owned();

        // Listing creates the default category; then one of the owner's own.
        th.client
            .get(&format!("/teams/{TEST_TEAM_ID}/categories"))
            .await
            .check_ok();
        let category: Category = th
            .client
            .post(
                &format!("/teams/{TEST_TEAM_ID}/categories"),
                &json!({"name": "Hidden", "userID": owner.id, "teamID": TEST_TEAM_ID}),
            )
            .await
            .ok_json();
        th.client
            .post(
                "/subscriptions",
                &json!({
                    "blockType": "card",
                    "blockId": card.id,
                    "subscriberType": "user",
                    "subscriberId": owner.id,
                }),
            )
            .await
            .check_ok();
        th.client
            .post(
                &format!("/boards/{}/sharing", board.id),
                &json!({"enabled": true, "token": "a-read-token"}),
            )
            .await
            .check_ok();

        Self {
            board: [board.id, new_id(IdKind::Board)],
            card: [card.id, new_id(IdKind::Card)],
            text: [text.id, new_id(IdKind::Block)],
            file: [file, format!("{}.txt", new_id(IdKind::None))],
            category: [category.id, new_id(IdKind::None)],
            owner: [owner.id, new_id(IdKind::User)],
            outsider,
            th,
        }
    }
}

/// One request, said once for what is hidden and once for what does not exist.
struct Case {
    method: Method,
    /// The path, with the existing identifiers or the missing ones.
    path: fn(&Hidden, usize) -> String,
    /// The JSON body, likewise; `null` for a request without one.
    body: fn(&Hidden, usize) -> Value,
}

fn no_body(_: &Hidden, _: usize) -> Value {
    Value::Null
}

fn title(_: &Hidden, _: usize) -> Value {
    json!({"title": "by an outsider"})
}

fn member(_: &Hidden, _: usize) -> Value {
    json!({"userId": "somebody", "schemeEditor": true})
}

#[expect(
    clippy::too_many_lines,
    reason = "a table with one entry per route, not logic"
)]
fn cases() -> Vec<Case> {
    let case = |method: Method,
                path: fn(&Hidden, usize) -> String,
                body: fn(&Hidden, usize) -> Value| Case { method, path, body };
    vec![
        // The board itself.
        case(
            Method::GET,
            |h, i| format!("/boards/{}", h.board[i]),
            no_body,
        ),
        case(
            Method::PATCH,
            |h, i| format!("/boards/{}", h.board[i]),
            title,
        ),
        case(
            Method::DELETE,
            |h, i| format!("/boards/{}", h.board[i]),
            no_body,
        ),
        case(
            Method::POST,
            |h, i| format!("/boards/{}/duplicate", h.board[i]),
            no_body,
        ),
        case(
            Method::POST,
            |h, i| format!("/boards/{}/undelete", h.board[i]),
            no_body,
        ),
        case(
            Method::GET,
            |h, i| format!("/boards/{}/archive/export", h.board[i]),
            no_body,
        ),
        // Its members.
        case(
            Method::GET,
            |h, i| format!("/boards/{}/members", h.board[i]),
            no_body,
        ),
        case(
            Method::POST,
            |h, i| format!("/boards/{}/members", h.board[i]),
            member,
        ),
        case(
            Method::PUT,
            |h, i| format!("/boards/{}/members/{}", h.board[i], h.owner[i]),
            member,
        ),
        case(
            Method::DELETE,
            |h, i| format!("/boards/{}/members/{}", h.board[i], h.owner[i]),
            no_body,
        ),
        case(
            Method::POST,
            |h, i| format!("/boards/{}/join", h.board[i]),
            no_body,
        ),
        case(
            Method::POST,
            |h, i| format!("/boards/{}/leave", h.board[i]),
            no_body,
        ),
        // Its sharing.
        case(
            Method::GET,
            |h, i| format!("/boards/{}/sharing", h.board[i]),
            no_body,
        ),
        case(
            Method::POST,
            |h, i| format!("/boards/{}/sharing", h.board[i]),
            |_, _| json!({"enabled": false}),
        ),
        // Its blocks.
        case(
            Method::GET,
            |h, i| format!("/boards/{}/blocks", h.board[i]),
            no_body,
        ),
        case(
            Method::GET,
            |h, i| format!("/boards/{}/blocks?block_id={}", h.board[i], h.card[i]),
            no_body,
        ),
        case(
            Method::POST,
            |h, i| format!("/boards/{}/blocks", h.board[i]),
            |h, i| {
                json!([{
                    "boardId": h.board[i], "type": "card", "createAt": 1, "updateAt": 1,
                }])
            },
        ),
        case(
            Method::PATCH,
            |h, i| format!("/boards/{}/blocks", h.board[i]),
            |h, i| json!({"block_ids": [h.card[i]], "block_patches": [{"title": "x"}]}),
        ),
        case(
            Method::PATCH,
            |h, i| format!("/boards/{}/blocks/{}", h.board[i], h.card[i]),
            title,
        ),
        case(
            Method::DELETE,
            |h, i| format!("/boards/{}/blocks/{}", h.board[i], h.card[i]),
            no_body,
        ),
        case(
            Method::POST,
            |h, i| format!("/boards/{}/blocks/{}/undelete", h.board[i], h.card[i]),
            no_body,
        ),
        case(
            Method::POST,
            |h, i| format!("/boards/{}/blocks/{}/duplicate", h.board[i], h.card[i]),
            no_body,
        ),
        // Its cards, by the board and by themselves.
        case(
            Method::GET,
            |h, i| format!("/boards/{}/cards", h.board[i]),
            no_body,
        ),
        case(
            Method::POST,
            |h, i| format!("/boards/{}/cards", h.board[i]),
            title,
        ),
        case(Method::GET, |h, i| format!("/cards/{}", h.card[i]), no_body),
        case(Method::PATCH, |h, i| format!("/cards/{}", h.card[i]), title),
        // Content: the block to move, and the block to move next to.
        case(
            Method::POST,
            |h, i| format!("/content-blocks/{}/moveto/after/{}", h.text[i], h.text[i]),
            no_body,
        ),
        // Its files.
        case(
            Method::GET,
            |h, i| format!("/files/teams/{TEST_TEAM_ID}/{}/{}", h.board[i], h.file[i]),
            no_body,
        ),
        case(
            Method::GET,
            |h, i| {
                format!(
                    "/files/teams/{TEST_TEAM_ID}/{}/{}/info",
                    h.board[i], h.file[i]
                )
            },
            no_body,
        ),
        // A wrong read token is no token.
        case(
            Method::GET,
            |h, i| format!("/boards/{}?read_token=wrong", h.board[i]),
            no_body,
        ),
        case(
            Method::GET,
            |h, i| format!("/boards/{}/blocks?read_token=wrong", h.board[i]),
            no_body,
        ),
        // Both kinds of combined request.
        case(
            Method::PATCH,
            |_, _| "/boards-and-blocks".to_owned(),
            |h, i| json!({"boardIDs": [h.board[i]], "boardPatches": [{"title": "x"}]}),
        ),
        case(
            Method::DELETE,
            |_, _| "/boards-and-blocks".to_owned(),
            |h, i| json!({"boards": [h.board[i]]}),
        ),
        // A subscription to one of its blocks, and the owner's subscriptions.
        case(
            Method::POST,
            |_, _| "/subscriptions".to_owned(),
            |h, i| {
                json!({
                    "blockType": "card", "blockId": h.card[i],
                    "subscriberType": "user", "subscriberId": h.outsider,
                })
            },
        ),
        case(
            Method::GET,
            |h, i| format!("/subscriptions/{}", h.owner[i]),
            no_body,
        ),
        case(
            Method::DELETE,
            |h, i| format!("/subscriptions/{}/{}", h.card[i], h.owner[i]),
            no_body,
        ),
        // The owner's category.
        case(
            Method::PUT,
            |h, i| format!("/teams/{TEST_TEAM_ID}/categories/{}", h.category[i]),
            |h, i| json!({"id": h.category[i], "name": "x", "teamID": TEST_TEAM_ID}),
        ),
        case(
            Method::DELETE,
            |h, i| format!("/teams/{TEST_TEAM_ID}/categories/{}", h.category[i]),
            no_body,
        ),
        case(
            Method::PUT,
            |h, i| {
                format!(
                    "/teams/{TEST_TEAM_ID}/categories/{}/boards/reorder",
                    h.category[i]
                )
            },
            |_, _| json!([]),
        ),
        case(
            Method::POST,
            |h, i| {
                format!(
                    "/teams/{TEST_TEAM_ID}/categories/{}/boards/{}",
                    h.category[i], h.board[i]
                )
            },
            no_body,
        ),
        case(
            Method::PUT,
            |h, i| {
                format!(
                    "/teams/{TEST_TEAM_ID}/categories/{}/boards/{}/hide",
                    h.category[i], h.board[i]
                )
            },
            no_body,
        ),
        // The owner's settings.
        case(
            Method::PUT,
            |h, i| format!("/users/{}/config", h.owner[i]),
            |_, _| json!({"updatedFields": {"a": "b"}}),
        ),
    ]
}

/// The parts of an answer a client could tell two requests apart by.
fn told(response: &ApiResponse) -> (StatusCode, Value) {
    let mut body = response.value();
    if let Some(body) = body.as_object_mut() {
        // Every request has its own identifier.
        body.remove("request_id");
    }
    (response.status, body)
}

async fn ask(client: &Client, hidden: &Hidden, case: &Case, which: usize) -> ApiResponse {
    let path = (case.path)(hidden, which);
    let request = client.request(case.method.clone(), &path);
    let request = match (case.body)(hidden, which) {
        Value::Null => request,
        body => request.json(&body),
    };
    Client::send(request).await
}

#[tokio::test]
async fn what_a_user_cannot_see_answers_as_what_does_not_exist() {
    let hidden = Hidden::start().await;
    let outsider = &hidden.th.client2;

    for case in cases() {
        let path = (case.path)(&hidden, 0);
        let existing = ask(outsider, &hidden, &case, 0).await;
        let missing = ask(outsider, &hidden, &case, 1).await;

        assert_eq!(
            existing.status,
            StatusCode::NOT_FOUND,
            "{} {path}: {}",
            case.method,
            existing.text()
        );
        assert_eq!(
            told(&existing),
            told(&missing),
            "{} {path}: what is hidden and what is missing answer alike",
            case.method
        );
        assert_eq!(
            existing.value()["error"],
            json!("not_found"),
            "{} {path}",
            case.method
        );
    }

    // None of it changed anything: the owner still has it all.
    let owner = &hidden.th.client;
    owner
        .get(&format!("/boards/{}", hidden.board[0]))
        .await
        .check_ok();
    let blocks: Vec<Block> = owner
        .get(&format!("/boards/{}/blocks", hidden.board[0]))
        .await
        .ok_json();
    assert_eq!(blocks.len(), 2, "the card and its text");
    let members: Vec<Value> = owner
        .get(&format!("/boards/{}/members", hidden.board[0]))
        .await
        .ok_json();
    assert_eq!(members.len(), 1, "the owner alone");
    let subscriptions: Vec<Value> = owner
        .get(&format!("/subscriptions/{}", hidden.owner[0]))
        .await
        .ok_json();
    assert_eq!(subscriptions.len(), 1, "the owner's subscription");
    hidden.th.teardown().await;
}

/// Without a session the answer is the same for everything too: 401, whatever is named.
#[tokio::test]
async fn without_a_session_nothing_is_told_either() {
    let hidden = Hidden::start().await;
    let anonymous = hidden.th.anonymous();

    for case in cases() {
        let path = (case.path)(&hidden, 0);
        let existing = ask(&anonymous, &hidden, &case, 0).await;
        let missing = ask(&anonymous, &hidden, &case, 1).await;

        assert_eq!(
            existing.status,
            StatusCode::UNAUTHORIZED,
            "{} {path}: {}",
            case.method,
            existing.text()
        );
        assert_eq!(
            told(&existing),
            told(&missing),
            "{} {path}: what is there and what is missing answer alike",
            case.method
        );
    }
    hidden.th.teardown().await;
}
