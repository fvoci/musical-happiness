//! Board trash and default template access do not freeze a team's membership.

use crate::boards::{board_with_admin, new_board};
use crate::common::{ROOT_WORKSPACE_ID, Setup, TestServer};
use fvoci_model::{Block, BlockType, BoardTeam, BoardType, Subscription, Team, new_id};
use reqwest::StatusCode;
use serde_json::json;

async fn assigned_team(th: &TestServer, board_path: &str, user: &str, name: &str) -> Team {
    let team: Team = th
        .client
        .post(
            &format!("/w/{ROOT_WORKSPACE_ID}/teams"),
            &json!({"name":name}),
        )
        .await
        .ok_json();
    th.client
        .put(&member_path(&team, user), &json!({}))
        .await
        .check_no_content();
    th.client
        .put(
            &format!("{board_path}/teams/{}", team.id),
            &json!({"role":"editor"}),
        )
        .await
        .check_no_content();
    team
}

fn member_path(team: &Team, user: &str) -> String {
    format!("/w/{ROOT_WORKSPACE_ID}/teams/{}/members/{user}", team.id)
}

#[tokio::test]
async fn board_restore_keeps_surviving_teams_and_uses_their_current_people() {
    let th = Setup::native().start().await.init_basic().await;
    let owner = th.client.me().await.id;
    let user = th.client2.me().await.id;
    let board = board_with_admin(
        &th,
        new_board(ROOT_WORKSPACE_ID, BoardType::Private, "Restore teams"),
        &owner,
    )
    .await;
    let path = format!("/w/{ROOT_WORKSPACE_ID}/boards/{}", board.id);
    let surviving = assigned_team(&th, &path, &user, "Surviving").await;
    let deleted = assigned_team(&th, &path, &user, "Deleted").await;
    th.client2.get(&path).await.check_ok();
    th.client.delete(&path).await.check_no_content();
    th.client2
        .get(&path)
        .await
        .check_status(StatusCode::NOT_FOUND);
    th.client2
        .post(&format!("{path}/undelete"), &json!({}))
        .await
        .check_status(StatusCode::NOT_FOUND);
    th.client
        .delete(&member_path(&surviving, &user))
        .await
        .check_no_content();
    let newcomer = th.stranger().await;
    th.client
        .put(&member_path(&surviving, &newcomer), &json!({}))
        .await
        .check_no_content();
    th.client
        .delete(&format!("/w/{ROOT_WORKSPACE_ID}/teams/{}", deleted.id))
        .await
        .check_no_content();
    th.client
        .post(&format!("{path}/undelete"), &json!({}))
        .await
        .check_ok();
    let assigned: Vec<BoardTeam> = th.client.get(&format!("{path}/teams")).await.ok_json();
    assert_eq!(assigned.len(), 1, "deleted teams stay deleted");
    assert_eq!(assigned[0].team.id, surviving.id);
    assert_eq!(assigned[0].role.as_str(), "editor");
    th.client2
        .get(&path)
        .await
        .check_status(StatusCode::NOT_FOUND);
    assert!(
        th.app
            .board_grant(&newcomer, &board.id)
            .await
            .unwrap()
            .reads(),
        "membership is read at restoration, not snapshotted at deletion"
    );
    th.teardown().await;
}

#[tokio::test]
async fn team_removal_preserves_template_subscriptions_until_workspace_membership_ends() {
    for departure in [false, true] {
        let th = Setup::native().start().await.init_basic().await;
        let owner = th.client.me().await.id;
        let user = th.client2.me().await.id;
        let mut template = new_board(ROOT_WORKSPACE_ID, BoardType::Open, "Open template");
        template.is_template = true;
        let board = board_with_admin(&th, template, &owner).await;
        let path = format!("/w/{ROOT_WORKSPACE_ID}/boards/{}", board.id);
        let team = assigned_team(&th, &path, &user, "Template editors").await;
        let mut block = Block {
            id: new_id(),
            board_id: board.id.clone(),
            block_type: BlockType::Card,
            ..Block::default()
        };
        th.app
            .store()
            .insert_block(&mut block, &owner)
            .await
            .unwrap();
        th.client2.post(&format!("/w/{ROOT_WORKSPACE_ID}/subscriptions"), &json!({"blockType":"card", "blockId":block.id, "subscriberType":"user", "subscriberId":user})).await.check_created();
        let subscriptions_path = format!("/w/{ROOT_WORKSPACE_ID}/subscriptions/{user}");
        th.client
            .delete(&member_path(&team, &user))
            .await
            .check_no_content();
        th.client2.get(&path).await.check_ok();
        let subscriptions: Vec<Subscription> = th.client2.get(&subscriptions_path).await.ok_json();
        assert_eq!(
            subscriptions.len(),
            1,
            "default template viewing still permits the subscription"
        );
        let membership = format!("/w/{ROOT_WORKSPACE_ID}/members/{user}");
        if departure {
            th.client.delete(&membership).await.check_no_content();
        } else {
            th.client
                .put(&membership, &json!({"role":"guest"}))
                .await
                .check_ok();
        }
        th.client2
            .get(&path)
            .await
            .check_status(StatusCode::NOT_FOUND);
        if departure {
            th.client2
                .get(&subscriptions_path)
                .await
                .check_status(StatusCode::NOT_FOUND);
        } else {
            th.client2.get(&subscriptions_path).await.check_ok();
        }
        let subscriptions = th
            .app
            .store()
            .get_subscriptions_in_workspace(&user, ROOT_WORKSPACE_ID)
            .await
            .unwrap();
        assert!(
            subscriptions.is_empty(),
            "the former member has no default template access"
        );
        th.teardown().await;
    }
}
