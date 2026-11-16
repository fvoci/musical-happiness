//! The built-in templates replaced: what goes and what is handed back, once however many
//! replace them at once.

use crate::boards::new_board;
use crate::common;
use fvoci_model::{Container, ContainerType, FileInfo, new_id};

/// Replacing the built-in templates is decided, done and handed back in one go: it keeps
/// the templates when `needed` says so, and otherwise removes them with their file
/// records, which it returns so that the files can go too, and stores the new ones.
#[tokio::test]
async fn replacing_the_built_in_templates_hands_back_the_old_files() {
    let test = common::setup().await;
    let workspace_id = common::workspace(&test).await;
    let template = |title: &str| Container {
        is_template: true,
        title: title.to_owned(),
        ..new_board(&new_id(), &workspace_id, ContainerType::Open)
    };
    let mut old = template("old");
    test.insert_container(&mut old, "")
        .await
        .expect("the old template is stored");
    let file = FileInfo {
        id: new_id(),
        container_id: old.id.clone(),
        stored_name: "picture.png".to_owned(),
        name: "picture.png".to_owned(),
        size: 1,
        path: format!("{}/picture.png", old.id),
        create_at: 1,
        ..FileInfo::default()
    };
    test.save_file_info(&file)
        .await
        .expect("the template's file is recorded");

    let mut new = fvoci_model::ContainersAndBlocks {
        containers: vec![template("new")],
        blocks: Vec::new(),
    };
    let kept = test
        .replace_builtin_templates(&mut new, <[Container]>::is_empty)
        .await
        .expect("the check runs");
    assert_eq!(kept, None, "the stored templates are kept when not needed");

    let files = test
        .replace_builtin_templates(&mut new, |stored| {
            stored.iter().any(|board| board.title == "old")
        })
        .await
        .expect("the templates are replaced")
        .expect("they were replaced");
    assert_eq!(
        files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>(),
        [file.path.as_str()],
        "the old template's file is handed back"
    );
    let titles: Vec<String> = test
        .get_builtin_templates()
        .await
        .expect("the built-in templates are read")
        .into_iter()
        .map(|board| board.title)
        .collect();
    assert_eq!(titles, ["new"], "the new templates are stored, built in");
    let error = test
        .get_file_info(&old.id, &file.stored_name)
        .await
        .expect_err("the record went with its board");
    assert!(error.is_not_found(), "got {error:?}");

    test.teardown().await;
}

/// Two replacements of the built-in templates that start at once, each finding none,
/// store them once between them: the second decides after the first has committed. On
/// PostgreSQL that is the advisory lock, on SQLite the write transaction.
#[tokio::test]
async fn replacements_of_the_built_in_templates_at_once_store_them_once() {
    let test = common::setup().await;
    let workspace_id = common::workspace(&test).await;
    let templates = || fvoci_model::ContainersAndBlocks {
        containers: vec![Container {
            is_template: true,
            ..new_board(&new_id(), &workspace_id, ContainerType::Open)
        }],
        blocks: Vec::new(),
    };
    let (mut first, mut second) = (templates(), templates());
    let (first, second) = tokio::join!(
        test.replace_builtin_templates(&mut first, <[Container]>::is_empty),
        test.replace_builtin_templates(&mut second, <[Container]>::is_empty),
    );
    let replaced = [first, second]
        .into_iter()
        .map(|result| result.expect("a replacement runs").is_some())
        .filter(|&replaced| replaced)
        .count();
    assert_eq!(replaced, 1, "one of them stores the templates");
    assert_eq!(
        test.get_builtin_templates()
            .await
            .expect("the built-in templates are read")
            .len(),
        1,
        "and they are there once"
    );
    test.teardown().await;
}
