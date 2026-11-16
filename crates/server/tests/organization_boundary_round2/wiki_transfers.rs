//! A wiki of another organization: copied whole only by its own organization's service,
//! never into a workspace of another.

use fvoci_model::{Board, BoardKind, BoardType, NewPage, PageLimits, new_id};
use fvoci_store::{BlockFilter, DbType, DuplicateAs};

use super::common::ROOT_WORKSPACE_ID;
use super::db_type_of_the_tests;
use super::media::Scene;

/// A wiki of organization B with a page: the wiki and the page.
pub(super) async fn b_wiki(scene: &Scene) -> (Board, String) {
    let mut wiki = Board {
        id: new_id(),
        workspace_id: scene.b.workspace_id.clone(),
        board_type: BoardType::Private,
        kind: BoardKind::Wiki,
        title: "B's handbook".to_owned(),
        ..Board::default()
    };
    let store = scene.b_app.store();
    store
        .insert_board_with_admin(&mut wiki, &scene.user)
        .await
        .expect("B's wiki");
    let new = NewPage {
        title: "B's page".to_owned(),
        ..NewPage::default()
    };
    let page = store
        .create_page(&wiki.id, &new, PageLimits::default(), &scene.user)
        .await
        .expect("B's page")
        .page
        .id;
    (wiki, page)
}

#[tokio::test]
async fn a_wiki_is_copied_by_its_own_organization_and_into_none_other() {
    if db_type_of_the_tests() != DbType::Postgres {
        return;
    }
    let scene = Scene::new(None).await;
    let (wiki, page) = b_wiki(&scene).await;
    assert!(
        scene
            .ts
            .app
            .duplicate_board(&wiki.id, &scene.user, ROOT_WORKSPACE_ID, DuplicateAs::Copy)
            .await
            .is_err(),
        "A's service does not find B's wiki"
    );
    assert!(
        scene
            .b_app
            .duplicate_board(&wiki.id, &scene.user, ROOT_WORKSPACE_ID, DuplicateAs::Copy)
            .await
            .is_err(),
        "B's service does not copy into A's workspace"
    );
    let (copies, _) = scene
        .b_app
        .duplicate_board(
            &wiki.id,
            &scene.user,
            &scene.b.workspace_id,
            DuplicateAs::Copy,
        )
        .await
        .expect("B copies its own wiki");
    let copy = &copies.boards[0];
    assert_eq!(copy.workspace_id, scene.b.workspace_id);
    assert!(
        scene.ts.app.store().get_board(&copy.id).await.is_err(),
        "A cannot read B's copy"
    );
    let pages = scene
        .b_app
        .store()
        .get_blocks(&BlockFilter::board(&copy.id))
        .await
        .expect("the copy's pages");
    assert_eq!(pages.len(), 1);
    assert_ne!(pages[0].id, page, "the copy's own page");
    scene.ts.teardown().await;
}
