//! The authors an archive names: whoever is no member of the organization imported into
//! is written as the importer, so an archive cannot make a stranger an author whose name
//! the workspace may then read; a member stays the author.

use std::io::{Cursor, Write as _};

use fvoci_model::{Block, BlockType, Board, User};
use serde_json::{Value, json};

use crate::common::{ROOT_WORKSPACE_ID, ROOT_WS, Setup, TestServer};
use crate::export::import;

/// An archive of one board, whose lines are `lines`.
fn archive(lines: &[Value]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    writer.start_file("version.json", options).expect("start");
    writer
        .write_all(br#"{"version":3,"date":1}"#)
        .expect("write");
    writer
        .start_file("authored/board.jsonl", options)
        .expect("start");
    for line in lines {
        writer
            .write_all(format!("{line}\n").as_bytes())
            .expect("write");
    }
    writer.finish().expect("finish").into_inner()
}

fn block(id: &str, parent: &str, kind: &str, created_by: &str, modified_by: &str) -> Value {
    json!({"type": "block", "data": {
        "id": id, "boardId": "board", "parentId": parent, "type": kind, "title": id,
        "createdBy": created_by, "modifiedBy": modified_by, "createAt": 1, "updateAt": 1,
    }})
}

/// A user who is no member of the organization, nor of any of its workspaces.
async fn outsider(th: &TestServer) -> String {
    let outsider = th.stranger().await;
    for table in ["workspace_members", "organization_members"] {
        th.app
            .store()
            .execute_unchecked(&format!("DELETE FROM {table} WHERE user_id = '{outsider}'"))
            .await
            .expect("the stranger leaves");
    }
    outsider
}

async fn named(th: &TestServer, user: &str) -> Vec<User> {
    th.client
        .post(&format!("{ROOT_WS}/users"), &[user])
        .await
        .ok_json()
}

#[tokio::test]
async fn an_imported_author_who_is_no_member_of_the_organization_is_the_importer() {
    let th = Setup::native().start().await.init_basic().await;
    let importer = th.client.me().await.id;
    let colleague = th.client2.me().await.id;
    let stranger = outsider(&th).await;
    assert!(
        named(&th, &stranger).await.is_empty(),
        "nobody names the stranger before the import"
    );
    let comment = json!({"type": "block", "data": {
        "id": "comment", "boardId": "board", "parentId": "theirs", "type": "comment",
        "title": "", "createdBy": stranger, "modifiedBy": stranger, "createAt": 1,
        "updateAt": 1,
        "body": {"type": "doc", "content": [
            {"type": "paragraph", "content": [{"type": "text", "text": "hello"}]}]},
    }});
    let bytes = archive(&[
        json!({"type": "board", "data": {
            "id": "board", "workspaceId": ROOT_WORKSPACE_ID, "type": "P", "title": "Authored",
            "createdBy": stranger, "modifiedBy": stranger, "createAt": 1, "updateAt": 1,
        }}),
        block("theirs", "board", "card", &stranger, &stranger),
        block("ours", "board", "card", &colleague, &colleague),
        block("mixed", "board", "card", &colleague, &stranger),
        comment,
    ]);
    import(&th.client, ROOT_WORKSPACE_ID, &bytes)
        .await
        .check_ok();

    let boards: Vec<Board> = th.client.get(&format!("{ROOT_WS}/boards")).await.ok_json();
    let board = boards
        .into_iter()
        .find(|board| board.title == "Authored")
        .expect("the imported board");
    assert_eq!(board.created_by, importer, "the board's stranger creator");
    assert_eq!(board.modified_by, importer, "the board's stranger modifier");
    let blocks: Vec<Block> = th
        .app
        .store()
        .get_blocks(&fvoci_store::BlockFilter::board(&board.id))
        .await
        .expect("the imported blocks");
    let by_title = |title: &str| {
        blocks
            .iter()
            .find(|block| block.title == title)
            .unwrap_or_else(|| panic!("the block {title}"))
    };
    for (title, created_by, modified_by) in [
        ("theirs", &importer, &importer),
        ("ours", &colleague, &colleague),
        ("mixed", &colleague, &importer),
    ] {
        let block = by_title(title);
        assert_eq!(&block.created_by, created_by, "{title}'s creator");
        assert_eq!(&block.modified_by, modified_by, "{title}'s modifier");
    }
    let comment = blocks
        .iter()
        .find(|block| block.block_type == BlockType::Comment)
        .expect("the imported comment");
    assert_eq!(
        comment.created_by, importer,
        "the comment's stranger writer"
    );
    assert_eq!(comment.modified_by, importer);

    assert!(
        named(&th, &stranger).await.is_empty(),
        "the stranger is still named to nobody"
    );
    assert_eq!(
        named(&th, &colleague).await.len(),
        1,
        "the colleague is named as before"
    );
    th.teardown().await;
}
