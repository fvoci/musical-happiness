//! Port of `server/services/store/storetests/teams.go`, followed by tests of the workspace
//! operations that the original suite does not cover.
//!
//! The original creates a workspace by upserting its signup token or its settings. Here a
//! workspace is created whole and only its signup token is replaced afterwards, so the
//! original's `testUpsertTeamSettings` has no port.

use fvoci_model::{JsonMap, Workspace, new_id};
use fvoci_store::StoreError;
use serde_json::json;

use crate::common;

fn settings(value: serde_json::Value) -> JsonMap {
    serde_json::from_value(value).expect("the settings are a JSON object")
}

/// `testGetTeam`.
#[tokio::test]
async fn get_workspace() {
    let test = common::setup().await;
    let store = &test.store;

    // "Nonexistent team"
    let error = store.get_workspace(&new_id()).await.unwrap_err();
    assert!(error.is_not_found(), "got {error:?}");
    let error = store.get_workspace("nonexistent-id").await.unwrap_err();
    assert!(error.is_not_found(), "got {error:?}");

    // "Valid team"
    let workspace = Workspace {
        id: new_id(),
        signup_token: "signup-token".to_owned(),
        ..Workspace::default()
    };

    store.create_workspace(&workspace).await.unwrap();

    let got = store.get_workspace(&workspace.id).await.unwrap();
    assert_eq!(got.id, workspace.id);

    test.teardown().await;
}

/// `testUpsertTeamSignupToken`: the insert is `create_workspace` here, and the update is
/// `set_workspace_signup_token`.
#[tokio::test]
async fn upsert_workspace_signup_token() {
    let test = common::setup().await;
    let store = &test.store;

    // "Insert and update team with signup token"
    let mut workspace = Workspace {
        id: new_id(),
        signup_token: "first-signup-token".to_owned(),
        ..Workspace::default()
    };

    // insert
    store.create_workspace(&workspace).await.unwrap();

    let got = store.get_workspace(&workspace.id).await.unwrap();
    assert_eq!(got.id, workspace.id);
    assert_eq!(got.signup_token, workspace.signup_token);

    // update signup token
    workspace.signup_token = "second-signup-token".to_owned();
    store
        .set_workspace_signup_token(&workspace.id, &workspace.signup_token, "")
        .await
        .unwrap();

    let got = store.get_workspace(&workspace.id).await.unwrap();
    assert_eq!(got.id, workspace.id);
    assert_eq!(got.signup_token, workspace.signup_token);

    test.teardown().await;
}

/// `testGetAllTeams`, of `get_workspaces`, which also promises an order: oldest first.
#[tokio::test]
async fn get_all_workspaces() {
    let test = common::setup().await;
    let store = &test.store;

    // "No teams response": an organization has its first workspace from the start.
    let got = store.get_workspaces().await.unwrap();
    let ids: Vec<&str> = got.iter().map(|workspace| workspace.id.as_str()).collect();
    assert_eq!(ids, [test.workspace_id.as_str()], "got {got:?}");

    // "Insert multiple team and get all teams"
    // insert
    let workspace_count = 10;
    let mut created = vec![test.workspace_id.clone()];
    for index in 0..workspace_count {
        let workspace = Workspace {
            id: new_id(),
            signup_token: format!("signup-token-{index}"),
            ..Workspace::default()
        };

        store.create_workspace(&workspace).await.unwrap();
        created.push(workspace.id);
    }

    let got = store.get_workspaces().await.unwrap();
    assert_eq!(got.len(), workspace_count + 1);
    let ids: Vec<String> = got.into_iter().map(|workspace| workspace.id).collect();
    assert_eq!(
        ids, created,
        "oldest first: the first is the root workspace"
    );

    test.teardown().await;
}

// The tests below are not in the original suite.

/// A workspace is stored whole, and a new signup token leaves the rest of it alone.
#[tokio::test]
async fn workspace_upserts_keep_each_others_members() {
    let test = common::setup().await;
    let store = &test.store;
    let creator = common::user(store, "workspace.creator").await;
    let renewer = common::user(store, "token.renewer").await;

    let workspace = Workspace {
        id: new_id(),
        title: "the name".to_owned(),
        signup_token: "first".to_owned(),
        settings: settings(json!({ "list": [1, 2], "nested": { "on": true } })),
        modified_by: creator.clone(),
        update_at: 5,
    };
    store.create_workspace(&workspace).await.unwrap();
    let created = store.get_workspace(&workspace.id).await.unwrap();
    assert_eq!(created.signup_token, "first", "the given token is stored");
    assert_eq!(created.settings, workspace.settings);
    assert_eq!(created.modified_by, creator);
    assert!(created.update_at > 5, "the store stamps the time");
    assert_eq!(created.title, "the name", "the title is the stored name");

    // A new signup token keeps the settings and records who set it.
    store
        .set_workspace_signup_token(&workspace.id, "renewed", &renewer)
        .await
        .unwrap();
    let got = store.get_workspace(&workspace.id).await.unwrap();
    assert_eq!(got.signup_token, "renewed");
    assert_eq!(got.settings, workspace.settings);
    assert_eq!(got.modified_by, renewer);
    assert!(
        got.update_at >= created.update_at,
        "the store stamps the time"
    );

    // A workspace stored without settings and on behalf of nobody reads back as such.
    let bare = Workspace {
        id: new_id(),
        signup_token: "bare".to_owned(),
        ..Workspace::default()
    };
    store.create_workspace(&bare).await.unwrap();
    let got = store.get_workspace(&bare.id).await.unwrap();
    assert!(got.settings.is_empty(), "got {got:?}");
    assert_eq!(got.modified_by, "", "no user is stored as none");

    let all = store.get_workspaces().await.unwrap();
    let ids: Vec<&str> = all.iter().map(|workspace| workspace.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            test.workspace_id.as_str(),
            workspace.id.as_str(),
            bare.id.as_str()
        ],
        "oldest first, after the organization's first workspace"
    );

    // A workspace is created once.
    let error = store.create_workspace(&workspace).await.unwrap_err();
    assert!(matches!(error, StoreError::Database(_)), "got {error:?}");
    let got = store.get_workspace(&workspace.id).await.unwrap();
    assert_eq!(
        got.signup_token, "renewed",
        "the refused workspace changed nothing"
    );

    // A workspace that does not exist: not found, and no token is set for it.
    let nowhere = new_id();
    let missing = store.get_workspace(&nowhere).await.unwrap_err();
    assert_eq!(missing.to_string(), "{workspace} not found");
    let missing = store
        .set_workspace_signup_token(&nowhere, "token", &renewer)
        .await
        .unwrap_err();
    assert!(missing.is_not_found(), "got {missing:?}");
    assert_eq!(missing.to_string(), "{workspace} not found");
    assert_eq!(
        store.get_workspaces().await.unwrap().len(),
        3,
        "no workspace was made"
    );

    // Text that is not an identifier names no workspace. The workspace "0" of the original is
    // such text.
    let error = store
        .set_workspace_signup_token("0", "token", &renewer)
        .await
        .unwrap_err();
    assert!(
        matches!(&error, StoreError::InvalidId(refused) if refused == "0"),
        "got {error:?}"
    );
    let zero = Workspace {
        id: "0".to_owned(),
        signup_token: "token".to_owned(),
        ..Workspace::default()
    };
    let error = store.create_workspace(&zero).await.unwrap_err();
    assert!(
        matches!(&error, StoreError::InvalidId(refused) if refused == "0"),
        "got {error:?}"
    );

    test.teardown().await;
}
