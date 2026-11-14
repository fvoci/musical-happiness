//! The pages of a wiki: what keeps a page on a wiki and below a page, where a new page
//! goes among the pages of its parent, and the tree.

use fvoci_model::{
    Block, BlockError, BlockPatch, BlockType, Board, BoardKind, BoardKinds, BoardType, NewPage,
    PageNode, new_id,
};
use fvoci_store::{DbType, Store, StoreError, Tenant};

use crate::boards::new_board;
use crate::common;

/// A board of `kind` with `user_id` as its administrator.
async fn stored(store: &Store, workspace_id: &str, user_id: &str, kind: BoardKind) -> Board {
    let mut board = Board {
        kind,
        ..new_board(&new_id(), workspace_id, BoardType::Private)
    };
    store
        .insert_board_with_admin(&mut board, user_id)
        .await
        .expect("the board is stored");
    board
}

fn block(board_id: &str, parent_id: &str, block_type: BlockType) -> Block {
    Block {
        id: new_id(),
        board_id: board_id.to_owned(),
        parent_id: parent_id.to_owned(),
        block_type,
        ..Block::default()
    }
}

fn refusal(result: Result<impl std::fmt::Debug, StoreError>) -> BlockError {
    match result {
        Err(StoreError::InvalidBlock { source, .. }) => source,
        other => panic!("the block is refused: {other:?}"),
    }
}

/// A page made below `parent_id` at the place `after` asks for; its identifier.
async fn page(store: &Store, wiki: &Board, parent_id: &str, after: Option<&str>) -> String {
    let new = NewPage {
        parent_id: parent_id.to_owned(),
        after: after.map(str::to_owned),
        ..NewPage::default()
    };
    store
        .create_page(&wiki.id, &new, &wiki.created_by)
        .await
        .expect("the page is made")
        .id
}

/// The pages of the wiki below `parent_id`, as the tree has them.
async fn below(store: &Store, wiki: &Board, parent_id: &str) -> Vec<String> {
    let tree = store.get_page_tree(&wiki.id).await.expect("the tree");
    tree.into_iter()
        .filter(|page| page.parent_id == parent_id)
        .map(|page| page.id)
        .collect()
}

#[tokio::test]
async fn a_page_is_a_block_of_a_wiki_and_a_wiki_has_no_other() {
    let test = common::setup().await;
    let user_id = common::user(&test, "user").await;
    let workspace_id = common::workspace_of(&test, &[&user_id]).await;
    let board = stored(&test, &workspace_id, &user_id, BoardKind::Board).await;
    let wiki = stored(&test, &workspace_id, &user_id, BoardKind::Wiki).await;
    let mut cards = [block(&board.id, "", BlockType::Card)];
    common::insert_blocks(&test, &mut cards, &user_id).await;
    let [card] = cards;
    let on_wiki = page(&test, &wiki, "", None).await;

    // A page on a board: alone, among others, below a card, and by the making of pages.
    for parent in ["", card.id.as_str()] {
        let mut stray = block(&board.id, parent, BlockType::Page);
        assert_eq!(
            refusal(test.insert_block(&mut stray, &user_id).await),
            BlockError::PageOffWiki,
            "below {parent:?}"
        );
        let mut among = [block(&board.id, "", BlockType::View), stray];
        assert_eq!(
            refusal(test.insert_blocks(&mut among, &user_id).await),
            BlockError::PageOffWiki
        );
    }
    assert_eq!(
        refusal(
            test.create_page(&board.id, &NewPage::default(), &user_id)
                .await
        ),
        BlockError::PageOffWiki
    );
    // What a board has, on a wiki: at its top and below a page.
    for block_type in [
        BlockType::Card,
        BlockType::View,
        BlockType::Attachment,
        BlockType::Comment,
        BlockType::from("text"),
    ] {
        for parent in ["", on_wiki.as_str()] {
            let mut stray = block(&wiki.id, parent, block_type.clone());
            assert_eq!(
                refusal(test.insert_block(&mut stray, &user_id).await),
                BlockError::NotPageOnWiki,
                "{block_type} below {parent:?}"
            );
        }
    }
    // Nor does a block become what its board has not: no patch makes a page.
    let to = |block_type: BlockType| BlockPatch {
        block_type: Some(block_type),
        ..BlockPatch::default()
    };
    assert_eq!(
        refusal(
            test.patch_block(&card.id, &to(BlockType::Page), &user_id)
                .await
        ),
        BlockError::PageByBlocks
    );
    assert_eq!(
        refusal(
            test.patch_block(&on_wiki, &to(BlockType::Card), &user_id)
                .await
        ),
        BlockError::NotPageOnWiki
    );

    // Nothing of what was refused is there.
    let blocks_of = async |board: &Board| {
        let filter = fvoci_store::BlockFilter::board(&board.id);
        let blocks = test.get_blocks(&filter).await.expect("the blocks");
        let mut found: Vec<(String, BlockType)> = blocks
            .into_iter()
            .map(|block| (block.id, block.block_type))
            .collect();
        found.sort_by(|one, other| one.0.cmp(&other.0));
        found
    };
    assert_eq!(
        blocks_of(&board).await,
        [(card.id.clone(), BlockType::Card)]
    );
    assert_eq!(blocks_of(&wiki).await, [(on_wiki.clone(), BlockType::Page)]);
    // A handle of boards finds no page, and one of wikis no card.
    for (id, kinds, found) in [
        (&on_wiki, BoardKinds::Wikis, true),
        (&on_wiki, BoardKinds::Boards, false),
        (&card.id, BoardKinds::Wikis, false),
    ] {
        let read = test.get_block_within(id, Some(&workspace_id), kinds).await;
        assert_eq!(read.is_ok(), found, "{kinds:?}: {read:?}");
    }
    test.teardown().await;
}

#[tokio::test]
async fn the_parent_of_a_page_is_a_page_of_its_wiki() {
    let test = common::setup().await;
    let user_id = common::user(&test, "user").await;
    let workspace_id = common::workspace_of(&test, &[&user_id]).await;
    let wiki = stored(&test, &workspace_id, &user_id, BoardKind::Wiki).await;
    let other = stored(&test, &workspace_id, &user_id, BoardKind::Wiki).await;
    let top = page(&test, &wiki, "", None).await;
    let elsewhere = page(&test, &other, "", None).await;

    // Below a page of the wiki, as deep as it goes; the wiki named as the parent is the top.
    let child = page(&test, &wiki, &top, None).await;
    let grandchild = page(&test, &wiki, &child, None).await;
    let named_top = page(&test, &wiki, &wiki.id, None).await;
    assert_eq!(below(&test, &wiki, "").await, [top.clone(), named_top]);
    assert_eq!(
        below(&test, &wiki, &top).await,
        std::slice::from_ref(&child)
    );
    assert_eq!(below(&test, &wiki, &child).await, [grandchild]);

    // A page of another wiki, the other wiki itself and nothing are no parent.
    for parent in [elsewhere.as_str(), other.id.as_str(), &new_id()] {
        let new = NewPage {
            parent_id: parent.to_owned(),
            ..NewPage::default()
        };
        assert_eq!(
            refusal(test.create_page(&wiki.id, &new, &user_id).await),
            BlockError::ParentNotOnBoard,
            "below {parent}"
        );
        let mut moved = test.get_block(&child).await.expect("the page");
        parent.clone_into(&mut moved.parent_id);
        assert_eq!(
            refusal(test.insert_block(&mut moved, &user_id).await),
            BlockError::ParentNotOnBoard,
            "moved below {parent}"
        );
    }
    // Of a page that is wrong in what it is below and in what it goes after, and of
    // one whose title is wrong besides, the first of these is what is refused.
    let mut new = NewPage {
        parent_id: elsewhere.clone(),
        after: Some(new_id()),
        ..NewPage::default()
    };
    assert_eq!(
        refusal(test.create_page(&wiki.id, &new, &user_id).await),
        BlockError::ParentNotOnBoard
    );
    new.title = "a\0b".to_owned();
    assert_eq!(
        refusal(test.create_page(&wiki.id, &new, &user_id).await),
        BlockError::NulInTitle
    );
    assert_eq!(below(&test, &wiki, &top).await, [child]);
    assert_eq!(below(&test, &other, "").await, [elsewhere]);
    // A wiki that is not there has no pages to make.
    let nowhere = test
        .create_page(&new_id(), &NewPage::default(), &user_id)
        .await;
    assert!(
        matches!(&nowhere, Err(error) if error.is_not_found()),
        "{nowhere:?}"
    );
    test.teardown().await;
}

#[tokio::test]
async fn a_new_page_goes_where_it_is_asked_among_the_pages_of_its_parent() {
    let test = common::setup().await;
    let user_id = common::user(&test, "user").await;
    let workspace_id = common::workspace_of(&test, &[&user_id]).await;
    let wiki = stored(&test, &workspace_id, &user_id, BoardKind::Wiki).await;

    // Last when no place is asked for, first after nothing, and right after a page.
    let first = page(&test, &wiki, "", None).await;
    let last = page(&test, &wiki, "", None).await;
    let before_all = page(&test, &wiki, "", Some("")).await;
    let after_first = page(&test, &wiki, "", Some(&first)).await;
    let after_last = page(&test, &wiki, "", Some(&last)).await;
    let top = [&before_all, &first, &after_first, &last, &after_last];
    assert_eq!(below(&test, &wiki, "").await, top.map(String::clone));
    // The pages of another parent have their own order.
    let child = page(&test, &wiki, &first, Some("")).await;
    let second_child = page(&test, &wiki, &first, Some(&child)).await;
    assert_eq!(
        below(&test, &wiki, &first).await,
        [child.clone(), second_child]
    );

    // After a page of another parent, or after nothing that is there.
    for (parent, after) in [("", child.as_str()), (first.as_str(), last.as_str())] {
        let new = NewPage {
            parent_id: parent.to_owned(),
            after: Some(after.to_owned()),
            ..NewPage::default()
        };
        assert_eq!(
            refusal(test.create_page(&wiki.id, &new, &user_id).await),
            BlockError::AfterNotSibling,
            "below {parent:?} after {after}"
        );
    }
    let unknown = NewPage {
        after: Some(new_id()),
        ..NewPage::default()
    };
    assert_eq!(
        refusal(test.create_page(&wiki.id, &unknown, &user_id).await),
        BlockError::AfterNotSibling
    );
    assert_eq!(below(&test, &wiki, "").await.len(), 5);
    test.teardown().await;
}

#[tokio::test]
async fn pages_made_at_once_get_places_of_their_own() {
    let test = common::setup().await;
    let user_id = common::user(&test, "user").await;
    let workspace_id = common::workspace_of(&test, &[&user_id]).await;
    let wiki = stored(&test, &workspace_id, &user_id, BoardKind::Wiki).await;
    let first = page(&test, &wiki, "", None).await;

    // Half of them at the end, half right after the first page.
    let (end, next) = (None, Some(first.as_str()));
    let made = tokio::join!(
        page(&test, &wiki, "", end),
        page(&test, &wiki, "", next),
        page(&test, &wiki, "", end),
        page(&test, &wiki, "", next),
        page(&test, &wiki, "", end),
        page(&test, &wiki, "", next),
    );
    let made = [made.0, made.1, made.2, made.3, made.4, made.5];
    let tree: Vec<PageNode> = test.get_page_tree(&wiki.id).await.expect("the tree");
    let mut orders: Vec<&str> = tree.iter().map(|page| page.order.as_str()).collect();
    assert_eq!(orders.len(), made.len() + 1);
    orders.dedup();
    assert_eq!(orders.len(), made.len() + 1, "no two pages have one order");
    assert_eq!(tree[0].id, first);
    test.teardown().await;
}

#[tokio::test]
async fn the_tree_has_every_page_that_is_not_in_the_trash_and_no_body() {
    let test = common::setup().await;
    let user_id = common::user(&test, "user").await;
    let workspace_id = common::workspace_of(&test, &[&user_id]).await;
    let wiki = stored(&test, &workspace_id, &user_id, BoardKind::Wiki).await;
    let board = stored(&test, &workspace_id, &user_id, BoardKind::Board).await;
    let new = NewPage {
        title: "Welcome".to_owned(),
        icon: "👋".to_owned(),
        ..NewPage::default()
    };
    let made = test
        .create_page(&wiki.id, &new, &user_id)
        .await
        .expect("the page is made");
    let child = page(&test, &wiki, &made.id, None).await;
    let grandchild = page(&test, &wiki, &child, None).await;
    let kept = page(&test, &wiki, "", None).await;

    let tree = test.get_page_tree(&wiki.id).await.expect("the tree");
    assert_eq!(
        tree[0],
        PageNode {
            id: made.id.clone(),
            parent_id: String::new(),
            title: "Welcome".to_owned(),
            icon: "👋".to_owned(),
            order: tree[0].order.clone(),
            create_at: made.create_at,
            update_at: made.update_at,
        }
    );
    assert!(!tree[0].order.is_empty() && made.created_by == user_id);
    assert_eq!(tree.len(), 4);
    let of_board = test.get_page_tree(&board.id).await.expect("the tree");
    assert!(of_board.is_empty(), "a board has no pages: {of_board:?}");

    // A page goes to the trash with the pages below it.
    test.delete_block(&made.id, &user_id)
        .await
        .expect("the page goes to the trash");
    let ids: Vec<String> = test
        .get_page_tree(&wiki.id)
        .await
        .expect("the tree")
        .into_iter()
        .map(|page| page.id)
        .collect();
    assert_eq!(ids, [kept]);
    for gone in [&made.id, &child, &grandchild] {
        let read = test.get_block(gone).await;
        assert!(
            matches!(&read, Err(error) if error.is_not_found()),
            "{read:?}"
        );
    }
    test.teardown().await;
}

#[tokio::test]
async fn the_body_of_a_page_has_a_room_in_its_wiki_as_that_of_a_card_in_its_board() {
    let test = common::setup().await;
    let user_id = common::user(&test, "user").await;
    let workspace_id = common::workspace_of(&test, &[&user_id]).await;
    let wiki = stored(&test, &workspace_id, &user_id, BoardKind::Wiki).await;
    let board = stored(&test, &workspace_id, &user_id, BoardKind::Board).await;
    let page_id = page(&test, &wiki, "", None).await;
    let mut on_board = [
        block(&board.id, "", BlockType::Card),
        block(&board.id, "", BlockType::View),
    ];
    common::insert_blocks(&test, &mut on_board, &user_id).await;
    let [card, view] = &on_board;
    let mut comment = [block(&board.id, &card.id, BlockType::Comment)];
    common::insert_blocks(&test, &mut comment, &user_id).await;

    for (block_id, expected) in [
        (&page_id, Some(&wiki.id)),
        (&card.id, Some(&board.id)),
        // A view has no body, a comment's is written whole by its author, and a wiki
        // is no block of itself.
        (&view.id, None),
        (&comment[0].id, None),
        (&wiki.id, None),
    ] {
        let found = test
            .board_of_card(block_id)
            .await
            .expect("the board of the block is read");
        assert_eq!(found.as_ref(), expected, "{block_id}");
    }
    test.teardown().await;
}

/// The wikis and pages of two organizations stay apart (PostgreSQL; SQLite holds one
/// organization): a handle of one finds no tree and no page of the other's, either
/// way, and makes no page in the other's wiki; a handle that names no organization
/// finds none of either. Nor does a page of the other's pass for a parent or a
/// sibling of one's own, which the wiki a page is asked for in refuses already.
#[tokio::test]
async fn the_pages_of_two_organizations_are_out_of_each_others_reach() {
    if common::db_type() != DbType::Postgres {
        return;
    }
    let test = common::setup().await;
    let unset = test.scoped(Tenant::Unset);
    let (other_org, their_workspace) = common::organization(&unset, "the other").await;
    let ours = &test.store;
    let theirs = test.for_org(&other_org).expect("an identifier");
    let user_id = common::user(ours, "anybody").await;
    let workspace_id = common::workspace(ours).await;
    let wiki_in = async |store: &Store, workspace_id: &str| {
        let mut wiki = Board {
            kind: BoardKind::Wiki,
            ..new_board(&new_id(), workspace_id, BoardType::Open)
        };
        store
            .insert_board(&mut wiki, &user_id)
            .await
            .expect("the wiki is stored");
        wiki
    };
    let our_wiki = wiki_in(ours, &workspace_id).await;
    let their_wiki = wiki_in(&theirs, &their_workspace).await;
    let our_page = page(ours, &our_wiki, "", None).await;
    let their_page = page(&theirs, &their_wiki, "", None).await;

    // Their wiki has no tree for us and takes no page of ours, and their page is no
    // block; nor ours for them. These read the blocks by nothing but their wiki or
    // their identifier: it is the wall that keeps them apart.
    for (store, wiki, page_id) in [
        (ours, &their_wiki, &their_page),
        (&theirs, &our_wiki, &our_page),
        (&unset, &their_wiki, &their_page),
        (&unset, &our_wiki, &our_page),
    ] {
        let tree = store.get_page_tree(&wiki.id).await.expect("a tree");
        assert!(tree.is_empty(), "{tree:?}");
        let found = store.board_of_card(page_id).await.expect("read");
        assert_eq!(found, None, "the wiki of the other's page");
    }
    let read = theirs.get_block(&our_page).await;
    assert!(
        matches!(&read, Err(error) if error.is_not_found()),
        "{read:?}"
    );
    let made = ours
        .create_page(&their_wiki.id, &NewPage::default(), &user_id)
        .await;
    assert!(
        matches!(&made, Err(error) if error.is_not_found()),
        "{made:?}"
    );
    let read = ours.get_block(&their_page).await;
    assert!(
        matches!(&read, Err(error) if error.is_not_found()),
        "{read:?}"
    );
    let wikis = ours
        .get_boards_of_workspace(&their_workspace, BoardKinds::Wikis)
        .await
        .expect("the wikis of the workspace");
    assert!(wikis.is_empty(), "{wikis:?}");

    // Nor is their page a parent or a sibling for a page of ours.
    let below_theirs = NewPage {
        parent_id: their_page.clone(),
        ..NewPage::default()
    };
    assert_eq!(
        refusal(
            ours.create_page(&our_wiki.id, &below_theirs, &user_id)
                .await
        ),
        BlockError::ParentNotOnBoard
    );
    let after_theirs = NewPage {
        after: Some(their_page.clone()),
        ..NewPage::default()
    };
    assert_eq!(
        refusal(
            ours.create_page(&our_wiki.id, &after_theirs, &user_id)
                .await
        ),
        BlockError::AfterNotSibling
    );

    // Each has its own, as it was.
    assert_eq!(below(ours, &our_wiki, "").await, [our_page]);
    let found = theirs.board_of_card(&their_page).await.expect("read");
    assert_eq!(found.as_ref(), Some(&their_wiki.id));
    assert_eq!(below(&theirs, &their_wiki, "").await, [their_page]);
    test.teardown().await;
}
