//! Tests of the search routes. The original's only test of them at this level is
//! `TestSearchBoards` in `server/integrationtests/board_test.go`, ported here with boards
//! that are put into the store directly; the rest checks the routes' answers against the
//! original server.

use fvoci_model::{Board, BoardType, JsonMap, new_id};
use reqwest::StatusCode;
use serde_json::{Value, json};

use crate::common::{
    ApiResponse, Client, ROOT_WORKSPACE_ID, Setup, TEST_WORKSPACE_ID, TestServer, UNKNOWN_ID,
};

/// Whether the creator of a board becomes a member of it; the last argument of the
/// original's `App.CreateBoard`.
#[derive(Clone, Copy)]
enum Membership {
    Admin,
    None,
}

/// The counterpart of the original's `App.CreateBoard`: puts a board into the store.
async fn create_board(
    th: &TestServer,
    board: Board,
    user_id: &str,
    membership: Membership,
) -> Board {
    let mut board = Board {
        id: new_id(),
        ..board
    };
    let store = th.app.store();
    match membership {
        Membership::Admin => {
            store
                .insert_board_with_admin(&mut board, user_id)
                .await
                .expect("the board is inserted with its administrator");
        }
        Membership::None => {
            store
                .insert_board(&mut board, user_id)
                .await
                .expect("the board is inserted");
        }
    }
    board
}

fn titled(title: &str, board_type: BoardType, workspace_id: &str) -> Board {
    Board {
        title: title.to_owned(),
        board_type,
        workspace_id: workspace_id.to_owned(),
        ..Board::default()
    }
}

/// `SearchBoardsForTeam`
async fn search_workspace(client: &Client, workspace_id: &str, term: &str) -> ApiResponse {
    client
        .get(&format!("/w/{workspace_id}/boards/search?q={term}"))
        .await
}

/// The identifiers of the boards in an answer, sorted.
#[track_caller]
fn found_ids(response: &ApiResponse) -> Vec<String> {
    let mut found: Vec<String> = response
        .ok_json::<Vec<Board>>()
        .into_iter()
        .map(|board| board.id)
        .collect();
    found.sort_unstable();
    found
}

fn sorted_ids(boards: &[&Board]) -> Vec<String> {
    let mut expected: Vec<String> = boards.iter().map(|board| board.id.clone()).collect();
    expected.sort_unstable();
    expected
}

#[track_caller]
fn check_error(response: &ApiResponse, status: StatusCode, message: &str) {
    response.check_error(status, message);
}

/// The answer when nothing is found: an empty list, not `null`.
#[track_caller]
fn check_nothing_found(response: &ApiResponse) {
    response.check_ok();
    assert_eq!(response.text(), "[]", "an empty list");
}

// ---------------------------------------------------------------------------------------
// server/integrationtests/board_test.go

/// `TestSearchBoards/"a non authenticated user should be rejected"`
#[tokio::test]
async fn search_boards_a_non_authenticated_user_should_be_rejected() {
    let th = Setup::native().start().await.init_basic().await;

    let response = search_workspace(&th.anonymous(), TEST_WORKSPACE_ID, "term").await;

    check_error(&response, StatusCode::UNAUTHORIZED, "no session token");
    th.teardown().await;
}

/// `TestSearchBoards/"all the matching private boards that the user is a member of and
/// all matching public boards should be returned"`, with its five cases.
#[tokio::test]
async fn search_boards_all_the_matching_private_boards_that_the_user_is_a_member_of_and_all_matching_public_boards_should_be_returned()
 {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let workspace_id = TEST_WORKSPACE_ID;
    let board =
        async |title: &str, board_type: BoardType, workspace_id: &str, membership: Membership| {
            create_board(
                &th,
                titled(title, board_type, workspace_id),
                &user1.id,
                membership,
            )
            .await
        };

    let board1 = board(
        "public board where user1 is admin",
        BoardType::Open,
        workspace_id,
        Membership::Admin,
    )
    .await;
    let board2 = board(
        "public board where user1 is not member",
        BoardType::Open,
        workspace_id,
        Membership::None,
    )
    .await;
    let board3 = board(
        "private board where user1 is admin",
        BoardType::Private,
        workspace_id,
        Membership::Admin,
    )
    .await;
    board(
        "private board where user1 is not member",
        BoardType::Private,
        workspace_id,
        Membership::None,
    )
    .await;
    let board5 = board(
        "private board where user1 is admin, but in other workspace",
        BoardType::Private,
        ROOT_WORKSPACE_ID,
        Membership::Admin,
    )
    .await;

    let cases = [
        (
            "should return all boards where user1 is member or that are public",
            &th.client,
            "board",
            vec![&board1, &board2, &board3, &board5],
        ),
        (
            "matching a full word",
            &th.client,
            "admin",
            vec![&board1, &board3, &board5],
        ),
        (
            "matching part of the word",
            &th.client,
            "ubli",
            vec![&board1, &board2],
        ),
        (
            "case insensitive",
            &th.client,
            "UBLI",
            vec![&board1, &board2],
        ),
        (
            "user2 can only see the public boards, as he's not a member of any",
            &th.client2,
            "board",
            vec![&board1, &board2],
        ),
    ];
    for (name, client, term, expected) in cases {
        let response = search_workspace(client, workspace_id, term).await;
        assert_eq!(found_ids(&response), sorted_ids(&expected), "{name}");
    }

    th.teardown().await;
}

// ---------------------------------------------------------------------------------------
// Not in the original: the routes' answers, checked against the original server.

/// A server with two users and two boards of the first: an open one titled "b one" and a
/// private one titled "b two", both in workspace `0`.
async fn two_boards() -> (TestServer, Board, Board) {
    let th = Setup::native().start().await.init_basic().await;
    let user = th.client.me().await;
    let open = create_board(
        &th,
        titled("b one", BoardType::Open, ROOT_WORKSPACE_ID),
        &user.id,
        Membership::Admin,
    )
    .await;
    let private = create_board(
        &th,
        titled("b two", BoardType::Private, ROOT_WORKSPACE_ID),
        &user.id,
        Membership::Admin,
    )
    .await;
    (th, open, private)
}

/// Without a search term nothing is found, whatever boards there are.
#[tokio::test]
async fn search_without_a_term_finds_nothing() {
    let (th, _open, _private) = two_boards().await;

    for path in [
        &format!("/w/{ROOT_WORKSPACE_ID}/boards/search"),
        &format!("/w/{ROOT_WORKSPACE_ID}/boards/search?q="),
        &format!("/w/{ROOT_WORKSPACE_ID}/boards/search?field=title"),
        "/boards/search",
        "/boards/search?q=",
    ] {
        check_nothing_found(&th.client.get(path).await);
    }
    // And a term that matches no board answers the same.
    check_nothing_found(&search_workspace(&th.client, ROOT_WORKSPACE_ID, "zzzzqq").await);
    check_nothing_found(&th.client.get("/boards/search?q=zzzzqq").await);

    th.teardown().await;
}

/// The search within a workspace: what each user finds, and how the term is read.
#[tokio::test]
async fn search_boards_of_a_workspace() {
    let (th, open, private) = two_boards().await;

    let response = search_workspace(&th.client, ROOT_WORKSPACE_ID, "b").await;
    assert_eq!(
        found_ids(&response),
        sorted_ids(&[&open, &private]),
        "the member finds both"
    );
    // The boards are answered in full.
    let found: Vec<Board> = response.ok_json();
    let found_open = found
        .iter()
        .find(|board| board.id == open.id)
        .expect("the open board is found");
    assert_eq!(*found_open, open, "the board as stored");

    let response = search_workspace(&th.client2, ROOT_WORKSPACE_ID, "b").await;
    assert_eq!(
        found_ids(&response),
        sorted_ids(&[&open]),
        "another user finds the open board only"
    );
    check_nothing_found(&search_workspace(&th.client2, ROOT_WORKSPACE_ID, "two").await);

    // The term is decoded from the query: `+` and `%20` are spaces, and every word of
    // the term must be in the title.
    for term in ["b+one", "b%20one", "ONE+b"] {
        let response = search_workspace(&th.client, ROOT_WORKSPACE_ID, term).await;
        assert_eq!(found_ids(&response), sorted_ids(&[&open]), "term {term}");
    }
    check_nothing_found(&search_workspace(&th.client, ROOT_WORKSPACE_ID, "one+two").await);

    // Preserved quirk: the workspace of the path is not part of the search.
    let response = search_workspace(&th.client, TEST_WORKSPACE_ID, "b").await;
    assert_eq!(
        found_ids(&response),
        sorted_ids(&[&open, &private]),
        "the boards of the root workspace are found under another workspace"
    );
    // The workspace has to be one, though.
    for no_workspace in [UNKNOWN_ID, "another-workspace"] {
        let response = search_workspace(&th.client, no_workspace, "b").await;
        response.check_error(StatusCode::NOT_FOUND, "workspace not found");
    }

    th.teardown().await;
}

/// The `field` parameter of the search within a workspace.
#[tokio::test]
async fn search_boards_by_field() {
    let (th, open, _private) = two_boards().await;
    let user = th.client.me().await;
    let mut properties = JsonMap::new();
    properties.insert("status".to_owned(), json!("green"));
    let with_property = create_board(
        &th,
        Board {
            properties,
            ..titled("with a property", BoardType::Open, ROOT_WORKSPACE_ID)
        },
        &user.id,
        Membership::Admin,
    )
    .await;
    let search = async |query: &str| {
        th.client
            .get(&format!("/w/{ROOT_WORKSPACE_ID}/boards/search?{query}"))
            .await
    };

    // The title is the default field.
    let response = search("q=one&field=title").await;
    assert_eq!(found_ids(&response), sorted_ids(&[&open]), "by title");
    let response = search("q=one&field=").await;
    assert_eq!(found_ids(&response), sorted_ids(&[&open]), "an empty field");

    // By property name: the term is the name of a property of the board.
    let response = search("q=status&field=property_name").await;
    assert_eq!(
        found_ids(&response),
        sorted_ids(&[&with_property]),
        "by property name"
    );
    check_nothing_found(&search("q=one&field=property_name").await);
    check_nothing_found(&search("q=status").await);

    // An unknown field is refused, even without a term; the names are case-sensitive.
    for query in ["q=one&field=bogus", "field=bogus", "q=one&field=TITLE"] {
        let response = search(query).await;
        check_error(
            &response,
            StatusCode::BAD_REQUEST,
            "invalid board search field",
        );
    }

    // The search across workspaces has no such parameter.
    let response = th.client.get("/boards/search?q=one&field=bogus").await;
    assert_eq!(found_ids(&response), sorted_ids(&[&open]), "field ignored");

    th.teardown().await;
}

/// The searches answer the same on both databases: a property name is a name, not a
/// path, and is found with a `null` value too; a title and a user name are matched
/// ignoring the case of every letter; `%` and `_` are characters.
#[tokio::test]
async fn searches_answer_the_same_on_both_databases() {
    let th = Setup::native().start().await.init_basic().await;
    let user = th.client.me().await;
    let mut properties = JsonMap::new();
    properties.insert("a.b".to_owned(), json!("x"));
    properties.insert("empty".to_owned(), Value::Null);
    properties.insert("nested".to_owned(), json!({"inner": 1}));
    let board = create_board(
        &th,
        Board {
            properties,
            ..titled("Ärger 100% über_all", BoardType::Open, ROOT_WORKSPACE_ID)
        },
        &user.id,
        Membership::Admin,
    )
    .await;
    let search = async |query: &str| {
        th.client
            .get(&format!("/w/{ROOT_WORKSPACE_ID}/boards/search?{query}"))
            .await
    };
    for query in [
        "q=a.b&field=property_name",
        "q=empty&field=property_name",
        "q=%C3%A4rger",
        "q=%C3%84RGER%20%C3%BCBER",
        "q=100%25",
        "q=r_a",
    ] {
        let response = search(query).await;
        assert_eq!(found_ids(&response), sorted_ids(&[&board]), "{query}");
    }
    for query in [
        "q=nested.inner&field=property_name",
        "q=a&field=property_name",
        "q=%5B&field=property_name",
        "q=a%00&field=property_name",
        "q=10%25%25",
        "q=r%25a",
    ] {
        let response = search(query).await;
        response.check_ok();
        check_nothing_found(&response);
    }
    th.teardown().await;
}

/// The search across all workspaces.
#[tokio::test]
async fn search_all_boards() {
    let (th, open, private) = two_boards().await;
    let user = th.client.me().await;
    let elsewhere = create_board(
        &th,
        titled("b three", BoardType::Private, TEST_WORKSPACE_ID),
        &user.id,
        Membership::Admin,
    )
    .await;
    let template = create_board(
        &th,
        Board {
            is_template: true,
            ..titled("b template", BoardType::Open, ROOT_WORKSPACE_ID)
        },
        &user.id,
        Membership::Admin,
    )
    .await;

    let response = th.client.get("/boards/search?q=b").await;
    assert_eq!(
        found_ids(&response),
        sorted_ids(&[&open, &private, &elsewhere]),
        "the member finds the boards of every workspace, and no template"
    );
    assert!(
        !found_ids(&response).contains(&template.id),
        "templates are left out"
    );

    let response = th.client2.get("/boards/search?q=b").await;
    assert_eq!(
        found_ids(&response),
        sorted_ids(&[&open]),
        "another user finds the open board only"
    );

    let response = th.anonymous().get("/boards/search?q=b").await;
    check_error(&response, StatusCode::UNAUTHORIZED, "no session token");

    th.teardown().await;
}

/// The original has routes that search the channels of its host application and the
/// boards that can be linked to one; a standalone server answered them with 501. They
/// are gone: for every caller they are paths that do not exist.
#[tokio::test]
async fn search_routes_of_the_host_application_do_not_exist() {
    let th = Setup::native().start().await.init_basic().await;
    let paths = [
        &format!("/teams/{ROOT_WORKSPACE_ID}/channels"),
        &format!("/teams/{ROOT_WORKSPACE_ID}/channels?search=x"),
        &format!("/w/{ROOT_WORKSPACE_ID}/boards/search/linkable?q=b"),
    ];

    for path in paths {
        for client in [&th.client, &th.anonymous()] {
            let response = client.get(path).await;
            check_error(&response, StatusCode::NOT_FOUND, "route not found");
        }
    }
    th.teardown().await;
}

/// The search within one workspace, which no route of a standalone server reaches (its caller
/// in the original is the linkable-boards route).
#[tokio::test]
async fn search_boards_for_user_in_workspace() {
    let (th, open, private) = two_boards().await;
    let user = th.client.me().await;
    let other = th.client2.me().await;
    create_board(
        &th,
        titled("b three", BoardType::Open, TEST_WORKSPACE_ID),
        &user.id,
        Membership::Admin,
    )
    .await;
    let search = async |workspace_id: &str, term: &str, user_id: &str| {
        let mut found: Vec<String> = th
            .app
            .search_boards_for_user_in_workspace(workspace_id, term, user_id)
            .await
            .expect("the search runs")
            .into_iter()
            .map(|board| board.id)
            .collect();
        found.sort_unstable();
        found
    };

    assert_eq!(
        search(ROOT_WORKSPACE_ID, "b", &user.id).await,
        sorted_ids(&[&open, &private]),
        "the member finds the boards of the workspace"
    );
    assert_eq!(
        search(ROOT_WORKSPACE_ID, "b", &other.id).await,
        sorted_ids(&[&open]),
        "another user finds the open one"
    );
    assert_eq!(
        search(ROOT_WORKSPACE_ID, "two", &user.id).await,
        sorted_ids(&[&private]),
        "by title"
    );
    assert!(
        search(UNKNOWN_ID, "b", &user.id).await.is_empty(),
        "nothing in a workspace that is none"
    );

    th.teardown().await;
}
