//! Port of `server/services/store/storetests/teams.go`, followed by tests of the team
//! operations that the original suite does not cover.

use fvoci_model::{IdKind, JsonMap, Team, new_id};
use serde_json::json;

use crate::common;

fn settings(value: serde_json::Value) -> JsonMap {
    serde_json::from_value(value).expect("the settings are a JSON object")
}

/// `testGetTeam`.
#[tokio::test]
async fn get_team() {
    let test = common::setup().await;
    let store = &test.store;

    // "Nonexistent team"
    let error = store.get_team("nonexistent-id").await.unwrap_err();
    assert!(error.is_not_found(), "got {error:?}");

    // "Valid team"
    let team_id = "0";
    let team = Team {
        id: team_id.to_owned(),
        signup_token: new_id(IdKind::Token),
        ..Team::default()
    };

    store.upsert_team_signup_token(&team).await.unwrap();

    let got = store.get_team(team_id).await.unwrap();
    assert_eq!(got.id, team_id);

    test.teardown().await;
}

/// `testUpsertTeamSignupToken`.
#[tokio::test]
async fn upsert_team_signup_token() {
    let test = common::setup().await;
    let store = &test.store;

    // "Insert and update team with signup token"
    let team_id = "0";
    let mut team = Team {
        id: team_id.to_owned(),
        signup_token: new_id(IdKind::Token),
        ..Team::default()
    };

    // insert
    store.upsert_team_signup_token(&team).await.unwrap();

    let got = store.get_team(team_id).await.unwrap();
    assert_eq!(got.id, team.id);
    assert_eq!(got.signup_token, team.signup_token);

    // update signup token
    team.signup_token = new_id(IdKind::Token);
    store.upsert_team_signup_token(&team).await.unwrap();

    let got = store.get_team(team_id).await.unwrap();
    assert_eq!(got.id, team.id);
    assert_eq!(got.signup_token, team.signup_token);

    test.teardown().await;
}

/// `testUpsertTeamSettings`.
#[tokio::test]
async fn upsert_team_settings() {
    let test = common::setup().await;
    let store = &test.store;

    // "Insert and update team with settings"
    let team_id = "0";
    let mut team = Team {
        id: team_id.to_owned(),
        settings: settings(json!({ "field1": "A" })),
        ..Team::default()
    };

    // insert
    store.upsert_team_settings(&team).await.unwrap();

    let got = store.get_team(team_id).await.unwrap();
    assert_eq!(got.id, team.id);
    assert_eq!(got.settings, team.settings);

    // update settings
    team.settings = settings(json!({ "field1": "B" }));
    store.upsert_team_settings(&team).await.unwrap();

    let got2 = store.get_team(team_id).await.unwrap();
    assert_eq!(got2.id, team.id);
    assert_eq!(got2.settings, team.settings);
    assert_eq!(got.signup_token, got2.signup_token);

    test.teardown().await;
}

/// `testGetAllTeams`.
#[tokio::test]
async fn get_all_teams() {
    let test = common::setup().await;
    let store = &test.store;

    // "No teams response"
    let got = store.get_all_teams().await.unwrap();
    assert!(got.is_empty(), "got {got:?}");

    // "Insert multiple team and get all teams"
    // insert
    let team_count = 10;
    for index in 0..team_count {
        let team = Team {
            id: index.to_string(),
            signup_token: new_id(IdKind::Token),
            ..Team::default()
        };

        store.upsert_team_signup_token(&team).await.unwrap();
    }

    let got = store.get_all_teams().await.unwrap();
    assert_eq!(got.len(), team_count);

    test.teardown().await;
}

// The tests below are not in the original suite. They pin behavior of the original's
// standalone store that its suite leaves untested.

/// Each upsert writes its own members and leaves the other's alone.
#[tokio::test]
async fn team_upserts_keep_each_others_members() {
    let test = common::setup().await;
    let store = &test.store;
    assert_eq!(store.get_team_count().await.unwrap(), 0);

    // Settings first: the new team gets a signup token of its own, whatever the given
    // team carries.
    let team = Team {
        id: "0".to_owned(),
        title: "not stored".to_owned(),
        signup_token: "ignored".to_owned(),
        settings: settings(json!({ "list": [1, 2], "nested": { "on": true } })),
        modified_by: "user-1".to_owned(),
        update_at: 5,
    };
    store.upsert_team_settings(&team).await.unwrap();
    let created = store.get_team("0").await.unwrap();
    assert_eq!(created.signup_token.len(), 27, "a generated token");
    assert!(created.signup_token.starts_with('k'), "a generated token");
    assert_eq!(created.settings, team.settings);
    assert_eq!(created.modified_by, "user-1");
    assert!(created.update_at > 5, "the store stamps the time");
    assert_eq!(created.title, "", "a team has no stored title");

    // A new signup token keeps the settings.
    let renewed = Team {
        id: "0".to_owned(),
        signup_token: "renewed".to_owned(),
        modified_by: "user-2".to_owned(),
        ..Team::default()
    };
    store.upsert_team_signup_token(&renewed).await.unwrap();
    let got = store.get_team("0").await.unwrap();
    assert_eq!(got.signup_token, "renewed");
    assert_eq!(got.settings, team.settings);
    assert_eq!(got.modified_by, "user-2");

    // A team inserted through its signup token has no settings, which read as empty.
    let bare = Team {
        id: "1".to_owned(),
        signup_token: "bare".to_owned(),
        ..Team::default()
    };
    store.upsert_team_signup_token(&bare).await.unwrap();
    let got = store.get_team("1").await.unwrap();
    assert!(got.settings.is_empty(), "got {got:?}");

    assert_eq!(store.get_team_count().await.unwrap(), 2);
    let all = store.get_all_teams().await.unwrap();
    let mut ids: Vec<&str> = all.iter().map(|team| team.id.as_str()).collect();
    ids.sort_unstable();
    assert_eq!(ids, ["0", "1"]);

    let missing = store.get_team("2").await.unwrap_err();
    assert_eq!(missing.to_string(), "sql: no rows in result set");

    test.teardown().await;
}
