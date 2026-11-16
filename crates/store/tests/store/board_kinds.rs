//! The kind of a board: what a board is made as it stays, what a wiki is not, and the
//! lists of boards, each of the kinds its caller asks for.

use fvoci_model::{
    Block, BlockType, Container, ContainerError, ContainerKind, ContainerKinds, ContainerPatch,
    ContainerSearchField, ContainerType, JsonMap, MinimumRole, WorkspaceRole, new_id,
};
use fvoci_store::{DbType, PublicContainers, Store, StoreError};
use serde_json::json;

use crate::boards::{new_board, object};
use crate::common;

/// Stores a private board of `kind` with `user_id` as its administrator.
async fn stored(
    store: &Store,
    workspace_id: &str,
    user_id: &str,
    kind: ContainerKind,
) -> Container {
    let mut board = Container {
        kind,
        title: format!("a {} to find", kind.as_str()),
        ..new_board(&new_id(), workspace_id, ContainerType::Private)
    };
    store
        .insert_container_with_admin(&mut board, user_id)
        .await
        .expect("the board is stored");
    board
}

fn ids(boards: &[Container]) -> Vec<&str> {
    boards.iter().map(|board| board.id.as_str()).collect()
}

fn refusal(result: Result<impl std::fmt::Debug, StoreError>) -> ContainerError {
    match result {
        Err(StoreError::InvalidContainer(refused)) => refused,
        other => panic!("the board is refused: {other:?}"),
    }
}

#[tokio::test]
async fn a_board_is_of_the_kind_it_was_made_as_whatever_stores_it_again() {
    let test = common::setup().await;
    let user_id = common::user(&test, "user").await;
    let workspace_id = common::workspace_of(&test, &[&user_id]).await;
    let board = stored(&test, &workspace_id, &user_id, ContainerKind::Board).await;
    let wiki = stored(&test, &workspace_id, &user_id, ContainerKind::Wiki).await;
    assert_eq!(
        Container::default().kind,
        ContainerKind::Board,
        "a board that says no kind is a board"
    );

    for (made, other) in [(&board, ContainerKind::Wiki), (&wiki, ContainerKind::Board)] {
        let mut as_other = Container {
            kind: other,
            title: "of another kind".to_owned(),
            ..made.clone()
        };
        assert_eq!(
            refusal(test.insert_container(&mut as_other, &user_id).await),
            ContainerError::KindChanges {
                stored: made.kind,
                asked: other,
            }
        );
        // A change that leaves the kind alone is stored, and the kind with it.
        let patch = ContainerPatch {
            description: Some("described".to_owned()),
            ..ContainerPatch::default()
        };
        let patched = test
            .patch_container(&made.id, &patch, &user_id)
            .await
            .expect("the board is patched");
        assert_eq!(patched.kind, made.kind);
        let read = test
            .get_container(&made.id)
            .await
            .expect("the board is read");
        assert_eq!(
            (read.kind, read.title.as_str(), read.description.as_str()),
            (made.kind, made.title.as_str(), "described")
        );
    }
    test.teardown().await;
}

#[tokio::test]
async fn a_wiki_is_no_template_and_has_no_card_properties() {
    let test = common::setup().await;
    let user_id = common::user(&test, "user").await;
    let workspace_id = common::workspace_of(&test, &[&user_id]).await;
    let property: JsonMap = object(json!({"id": "status", "name": "Status", "type": "text"}));
    let wiki = || Container {
        kind: ContainerKind::Wiki,
        ..new_board(&new_id(), &workspace_id, ContainerType::Open)
    };

    let mut template = Container {
        is_template: true,
        ..wiki()
    };
    assert_eq!(
        refusal(test.insert_container(&mut template, &user_id).await),
        ContainerError::WikiIsTemplate
    );
    let mut with_properties = Container {
        card_properties: vec![property.clone()],
        ..wiki()
    };
    assert_eq!(
        refusal(test.insert_container(&mut with_properties, &user_id).await),
        ContainerError::WikiHasCardProperties
    );
    for refused in [&template, &with_properties] {
        let found = test.get_container(&refused.id).await;
        assert!(
            matches!(&found, Err(error) if error.is_not_found()),
            "nothing is stored of what is refused: {found:?}"
        );
    }

    // Nor does a wiki that is stored get them afterwards.
    let wiki = stored(&test, &workspace_id, &user_id, ContainerKind::Wiki).await;
    let patch = ContainerPatch {
        updated_card_properties: vec![property],
        ..ContainerPatch::default()
    };
    assert_eq!(
        refusal(test.patch_container(&wiki.id, &patch, &user_id).await),
        ContainerError::WikiHasCardProperties
    );
    let read = test
        .get_container(&wiki.id)
        .await
        .expect("the wiki is read");
    assert!(
        read.card_properties.is_empty(),
        "the wiki has none: {:?}",
        read.card_properties
    );
    test.teardown().await;
}

#[tokio::test]
async fn every_list_of_boards_is_of_the_kinds_it_is_asked_for() {
    let test = common::setup().await;
    let user_id = common::user(&test, "user").await;
    let workspace_id = common::workspace_of(&test, &[&user_id]).await;
    let board = stored(&test, &workspace_id, &user_id, ContainerKind::Board).await;
    let wiki = stored(&test, &workspace_id, &user_id, ContainerKind::Wiki).await;
    // Somebody who is on both through a team only.
    let (admin, through_team) = (
        common::user(&test, "admin").await,
        common::user(&test, "teammate").await,
    );
    for (who, role) in [
        (&admin, WorkspaceRole::Admin),
        (&through_team, WorkspaceRole::Member),
    ] {
        test.add_workspace_member(&workspace_id, who, role)
            .await
            .expect("the user is let into the workspace");
    }
    let team = test
        .create_team(&workspace_id, "a team", &admin)
        .await
        .expect("a team is stored");
    test.add_team_member(&workspace_id, &team, &through_team, &admin)
        .await
        .expect("a team member is stored");
    for on in [&board, &wiki] {
        let role = Some(MinimumRole::Editor);
        test.set_container_team(&workspace_id, &on.id, &team, role, &admin)
            .await
            .expect("the team is on the board");
    }
    let cases = [
        (ContainerKinds::Boards, vec![board.id.as_str()]),
        (ContainerKinds::Wikis, vec![wiki.id.as_str()]),
        (
            ContainerKinds::Every,
            vec![board.id.as_str(), wiki.id.as_str()],
        ),
    ];

    for (kinds, expected) in &cases {
        let kinds = *kinds;
        let of_user = test
            .get_containers_for_user_and_workspace(
                &user_id,
                &workspace_id,
                PublicContainers::Include,
                kinds,
            )
            .await
            .expect("the boards of the user");
        let of_workspace = test
            .get_containers_of_workspace(&workspace_id, kinds)
            .await
            .expect("the boards of the workspace");
        let found = test
            .search_containers_for_user(
                "find",
                ContainerSearchField::Title,
                &user_id,
                PublicContainers::Include,
                kinds,
            )
            .await
            .expect("the boards found");
        let found_there = test
            .search_containers_for_user_in_workspace(&workspace_id, "find", &user_id, kinds)
            .await
            .expect("the boards found in the workspace");
        let memberships = test
            .container_memberships_of_user(&user_id, None, kinds)
            .await
            .expect("the memberships");
        let member_of: Vec<&str> = memberships
            .iter()
            .map(|member| member.container_id.as_str())
            .collect();
        let memberships = test
            .container_memberships_of_user(&through_team, None, kinds)
            .await
            .expect("the memberships through the team");
        let through: Vec<&str> = memberships
            .iter()
            .map(|member| member.container_id.as_str())
            .collect();
        for (list, listed) in [
            ("of the user", ids(&of_user)),
            ("of the workspace", ids(&of_workspace)),
            ("found", ids(&found)),
            ("found in the workspace", ids(&found_there)),
            ("the user is on", member_of),
            ("a team is on", through),
        ] {
            assert_eq!(&listed, expected, "the boards {list}, of {kinds:?}");
        }
    }
    test.teardown().await;
}

#[tokio::test]
async fn a_block_is_found_within_the_kinds_of_its_board_only() {
    let test = common::setup().await;
    let user_id = common::user(&test, "user").await;
    let workspace_id = common::workspace_of(&test, &[&user_id]).await;
    let elsewhere = common::workspace(&test).await;
    let board = stored(&test, &workspace_id, &user_id, ContainerKind::Board).await;
    let mut cards = [Block {
        id: new_id(),
        container_id: board.id.clone(),
        block_type: BlockType::Card,
        ..Block::default()
    }];
    common::insert_blocks(&test, &mut cards, &user_id).await;
    let card = &cards[0].id;

    for (workspace, kinds, found) in [
        (None, ContainerKinds::Every, true),
        (None, ContainerKinds::Boards, true),
        (None, ContainerKinds::Wikis, false),
        (Some(workspace_id.as_str()), ContainerKinds::Boards, true),
        (Some(workspace_id.as_str()), ContainerKinds::Wikis, false),
        (Some(elsewhere.as_str()), ContainerKinds::Every, false),
    ] {
        let read = test.get_block_within(card, workspace, kinds).await;
        match read {
            Ok(block) => assert!(found && block.id == *card, "{workspace:?}, {kinds:?}"),
            Err(error) => assert!(
                !found && error.is_not_found(),
                "{workspace:?}, {kinds:?}: {error:?}"
            ),
        }
    }
    test.teardown().await;
}

#[tokio::test]
async fn the_trash_is_listed_by_kind_and_what_went_before_kinds_is_a_board() {
    let test = common::setup().await;
    let user_id = common::user(&test, "user").await;
    let workspace_id = common::workspace_of(&test, &[&user_id]).await;
    let board = stored(&test, &workspace_id, &user_id, ContainerKind::Board).await;
    let wiki = stored(&test, &workspace_id, &user_id, ContainerKind::Wiki).await;
    for deleted in [&board, &wiki] {
        test.delete_container(&deleted.id, &user_id)
            .await
            .expect("the board goes to the trash");
    }
    // The board went before boards had a kind: what the trash keeps of it says none.
    let without_kind = match common::db_type() {
        DbType::Sqlite => "json_remove(container, '$.kind')",
        DbType::Postgres => "(container::jsonb - 'kind')::json",
    };
    common::execute_raw(
        &test.dbconfig(),
        &format!(
            "UPDATE deleted_containers SET container = {without_kind} WHERE id = '{}'",
            board.id
        ),
    )
    .await;

    for (kinds, expected) in [
        (ContainerKinds::Boards, vec![board.id.as_str()]),
        (ContainerKinds::Wikis, vec![wiki.id.as_str()]),
    ] {
        for admin in [None, Some(user_id.as_str())] {
            let listed = test
                .get_deleted_containers(&workspace_id, admin, None, kinds)
                .await
                .expect("the trash is listed");
            let listed: Vec<&str> = listed
                .iter()
                .map(|deleted| deleted.container.id.as_str())
                .collect();
            assert_eq!(listed, expected, "{kinds:?}, for {admin:?}");
        }
    }
    let every = test
        .get_deleted_containers(&workspace_id, None, None, ContainerKinds::Every)
        .await
        .expect("the trash is listed");
    assert_eq!(every.len(), 2);

    // Each comes back as what it was.
    for (deleted, kind) in [(&board, ContainerKind::Board), (&wiki, ContainerKind::Wiki)] {
        let restored = test
            .undelete_container(&deleted.id, &user_id)
            .await
            .expect("the board comes back");
        assert_eq!(restored.kind, kind);
        let read = test.get_container(&deleted.id).await.expect("it is read");
        assert_eq!(read.kind, kind);
    }
    test.teardown().await;
}
