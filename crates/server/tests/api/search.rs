//! Tests of the search routes. The original's only test of them at this level is
//! `TestSearchBoards` in `server/integrationtests/board_test.go`, ported here with boards
//! that are put into the store directly; the rest checks the routes' answers against the
//! original server.

use fvoci_model::{Board, BoardType, IdKind, JsonMap, SINGLE_USER_ID, new_id};
use reqwest::StatusCode;
use serde_json::json;

use crate::common::{ApiResponse, Client, Setup, TEST_TEAM_ID, TestServer};

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
        id: new_id(IdKind::Board),
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

fn titled(title: &str, board_type: BoardType, team_id: &str) -> Board {
    Board {
        title: title.to_owned(),
        board_type,
        team_id: team_id.to_owned(),
        ..Board::default()
    }
}

/// `SearchBoardsForTeam`
async fn search_team(client: &Client, team_id: &str, term: &str) -> ApiResponse {
    client
        .get(&format!("/teams/{team_id}/boards/search?q={term}"))
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
    response.check_status(status);
    assert_eq!(
        response.value(),
        json!({"error": message, "errorCode": status.as_u16()}),
        "the error answer"
    );
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

    let response = search_team(&th.anonymous(), TEST_TEAM_ID, "term").await;

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
    let team_id = TEST_TEAM_ID;
    let board =
        async |title: &str, board_type: BoardType, team_id: &str, membership: Membership| {
            create_board(
                &th,
                titled(title, board_type, team_id),
                &user1.id,
                membership,
            )
            .await
        };

    let board1 = board(
        "public board where user1 is admin",
        BoardType::Open,
        team_id,
        Membership::Admin,
    )
    .await;
    let board2 = board(
        "public board where user1 is not member",
        BoardType::Open,
        team_id,
        Membership::None,
    )
    .await;
    let board3 = board(
        "private board where user1 is admin",
        BoardType::Private,
        team_id,
        Membership::Admin,
    )
    .await;
    board(
        "private board where user1 is not member",
        BoardType::Private,
        team_id,
        Membership::None,
    )
    .await;
    let board5 = board(
        "private board where user1 is admin, but in other team",
        BoardType::Private,
        "other-team-id",
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
        let response = search_team(client, team_id, term).await;
        assert_eq!(found_ids(&response), sorted_ids(&expected), "{name}");
    }

    th.teardown().await;
}

// ---------------------------------------------------------------------------------------
// Not in the original: the routes' answers, checked against the original server.

/// A server with two users and two boards of the first: an open one titled "b one" and a
/// private one titled "b two", both in team `0`.
async fn two_boards() -> (TestServer, Board, Board) {
    let th = Setup::native().start().await.init_basic().await;
    let user = th.client.me().await;
    let open = create_board(
        &th,
        titled("b one", BoardType::Open, "0"),
        &user.id,
        Membership::Admin,
    )
    .await;
    let private = create_board(
        &th,
        titled("b two", BoardType::Private, "0"),
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
        "/teams/0/boards/search",
        "/teams/0/boards/search?q=",
        "/teams/0/boards/search?field=title",
        "/boards/search",
        "/boards/search?q=",
    ] {
        check_nothing_found(&th.client.get(path).await);
    }
    // And a term that matches no board answers the same.
    check_nothing_found(&search_team(&th.client, "0", "zzzzqq").await);
    check_nothing_found(&th.client.get("/boards/search?q=zzzzqq").await);

    th.teardown().await;
}

/// The search within a team: what each user finds, and how the term is read.
#[tokio::test]
async fn search_boards_of_a_team() {
    let (th, open, private) = two_boards().await;

    let response = search_team(&th.client, "0", "b").await;
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

    let response = search_team(&th.client2, "0", "b").await;
    assert_eq!(
        found_ids(&response),
        sorted_ids(&[&open]),
        "another user finds the open board only"
    );
    check_nothing_found(&search_team(&th.client2, "0", "two").await);

    // The term is decoded from the query: `+` and `%20` are spaces, and every word of
    // the term must be in the title.
    for term in ["b+one", "b%20one", "ONE+b"] {
        let response = search_team(&th.client, "0", term).await;
        assert_eq!(found_ids(&response), sorted_ids(&[&open]), "term {term}");
    }
    check_nothing_found(&search_team(&th.client, "0", "one+two").await);

    // Preserved quirk: the team of the path is not part of the search.
    let response = search_team(&th.client, "another-team", "b").await;
    assert_eq!(
        found_ids(&response),
        sorted_ids(&[&open, &private]),
        "the boards of team 0 are found under another team"
    );

    th.teardown().await;
}

/// The `field` parameter of the search within a team.
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
            ..titled("with a property", BoardType::Open, "0")
        },
        &user.id,
        Membership::Admin,
    )
    .await;
    let search = async |query: &str| {
        th.client
            .get(&format!("/teams/0/boards/search?{query}"))
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

    // The search across teams has no such parameter.
    let response = th.client.get("/boards/search?q=one&field=bogus").await;
    assert_eq!(found_ids(&response), sorted_ids(&[&open]), "field ignored");

    th.teardown().await;
}

/// The search across all teams.
#[tokio::test]
async fn search_all_boards() {
    let (th, open, private) = two_boards().await;
    let user = th.client.me().await;
    let elsewhere = create_board(
        &th,
        titled("b three", BoardType::Private, "other-team"),
        &user.id,
        Membership::Admin,
    )
    .await;
    let template = create_board(
        &th,
        Board {
            is_template: true,
            ..titled("b template", BoardType::Open, "0")
        },
        &user.id,
        Membership::Admin,
    )
    .await;

    let response = th.client.get("/boards/search?q=b").await;
    assert_eq!(
        found_ids(&response),
        sorted_ids(&[&open, &private, &elsewhere]),
        "the member finds the boards of every team, and no template"
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

/// In single-user mode there are no accounts to look up, and the search works.
#[tokio::test]
async fn search_in_single_user_mode() {
    let th = Setup::single_user().start().await;
    let board = create_board(
        &th,
        titled("the only board", BoardType::Private, TEST_TEAM_ID),
        SINGLE_USER_ID,
        Membership::Admin,
    )
    .await;

    let response = search_team(&th.client, TEST_TEAM_ID, "only").await;
    assert_eq!(found_ids(&response), sorted_ids(&[&board]), "in the team");
    let response = th.client.get("/boards/search?q=only").await;
    assert_eq!(found_ids(&response), sorted_ids(&[&board]), "across teams");

    th.teardown().await;
}

/// The two routes that need the host application the original can run in: a standalone
/// server refuses them, whatever is asked.
#[tokio::test]
async fn search_routes_of_the_host_application_are_not_implemented() {
    let (th, _open, _private) = two_boards().await;
    let paths = [
        "/teams/0/channels",
        "/teams/0/channels?search=x",
        "/teams/0/boards/search/linkable",
        "/teams/0/boards/search/linkable?q=b",
    ];

    for path in paths {
        let response = th.client.get(path).await;
        check_error(
            &response,
            StatusCode::NOT_IMPLEMENTED,
            "not permitted in standalone mode",
        );
        // The session is checked first.
        let response = th.anonymous().get(path).await;
        check_error(&response, StatusCode::UNAUTHORIZED, "no session token");
    }

    let single_user = Setup::single_user().start().await;
    for path in paths {
        single_user.client.get(path).await.check_not_implemented();
    }
    single_user.teardown().await;

    th.teardown().await;
}

/// The search within one team, which no route of a standalone server reaches (its caller
/// in the original is the linkable-boards route).
#[tokio::test]
async fn search_boards_for_user_in_team() {
    let (th, open, private) = two_boards().await;
    let user = th.client.me().await;
    let other = th.client2.me().await;
    create_board(
        &th,
        titled("b three", BoardType::Open, "other-team"),
        &user.id,
        Membership::Admin,
    )
    .await;
    let search = async |team_id: &str, term: &str, user_id: &str| {
        let mut found: Vec<String> = th
            .app
            .search_boards_for_user_in_team(team_id, term, user_id)
            .await
            .expect("the search runs")
            .into_iter()
            .map(|board| board.id)
            .collect();
        found.sort_unstable();
        found
    };

    assert_eq!(
        search("0", "b", &user.id).await,
        sorted_ids(&[&open, &private]),
        "the member finds the boards of the team"
    );
    assert_eq!(
        search("0", "b", &other.id).await,
        sorted_ids(&[&open]),
        "another user finds the open one"
    );
    assert_eq!(
        search("0", "two", &user.id).await,
        sorted_ids(&[&private]),
        "by title"
    );
    assert!(
        search("no-such-team", "b", &user.id).await.is_empty(),
        "nothing in a team without boards"
    );

    th.teardown().await;
}
