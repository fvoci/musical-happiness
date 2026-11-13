use super::*;

fn add_listener(listeners: &mut Listeners, user_id: &str) -> ListenerId {
    let (outbox, _inbox) = mpsc::channel(1);
    listeners.next_id += 1;
    let id = listeners.next_id;
    listeners.by_id.insert(
        id,
        Listener {
            user_id: user_id.to_owned(),
            session_id: String::new(),
            login_provider: None,
            outbox,
            closed: CancellationToken::new(),
            closed_because: Arc::default(),
            workspaces: Vec::new(),
            blocks: Vec::new(),
            shared_boards: Vec::new(),
        },
    );
    id
}

fn unsubscribe_from_workspace(listeners: &mut Listeners, id: ListenerId, workspace_id: &str) {
    if let Some(listener) = listeners.by_id.get_mut(&id) {
        listener
            .workspaces
            .retain(|workspace| workspace != workspace_id);
    }
    remove_from(&mut listeners.by_workspace, workspace_id, id);
}

fn workspaces_of(listeners: &Listeners, id: ListenerId) -> &[String] {
    &listeners.by_id[&id].workspaces
}

fn blocks_of(listeners: &Listeners, id: ListenerId) -> &[String] {
    &listeners.by_id[&id].blocks
}

fn workspace_listeners<'a>(listeners: &'a Listeners, workspace_id: &str) -> &'a [ListenerId] {
    listeners
        .by_workspace
        .get(workspace_id)
        .map_or(&[], Vec::as_slice)
}

fn block_listeners<'a>(listeners: &'a Listeners, block_id: &str) -> &'a [ListenerId] {
    listeners.by_block.get(block_id).map_or(&[], Vec::as_slice)
}

/// `TestTeamSubscription`
#[test]
fn workspace_subscription() {
    let mut listeners = Listeners::default();
    let workspace_id = "fake-workspace-id";

    // "Should correctly add a session"
    let session = add_listener(&mut listeners, "");
    assert_eq!(listeners.by_id.len(), 1, "one listener");
    assert!(
        listeners.by_workspace.is_empty(),
        "no workspace subscriptions yet"
    );
    assert!(
        workspaces_of(&listeners, session).is_empty(),
        "the session has no workspaces"
    );

    // "Should correctly subscribe to a team" and "Subscribing again to a subscribed
    // workspace would have no effect"
    for round in ["first", "again"] {
        subscribe_to_workspace(&mut listeners, session, workspace_id);
        assert_eq!(
            workspace_listeners(&listeners, workspace_id),
            [session],
            "{round}"
        );
        assert_eq!(
            workspaces_of(&listeners, session),
            [workspace_id],
            "{round}"
        );
    }

    // "Should correctly unsubscribe to a team" and "Unsubscribing again to an
    // unsubscribed workspace would have no effect"
    for round in ["first", "again"] {
        unsubscribe_from_workspace(&mut listeners, session, workspace_id);
        assert!(
            workspace_listeners(&listeners, workspace_id).is_empty(),
            "{round}"
        );
        assert!(workspaces_of(&listeners, session).is_empty(), "{round}");
    }

    // "Should correctly be removed from the server"
    listeners.remove(session);
    assert!(listeners.by_id.is_empty(), "no listener is left");

    // "If subscribed to workspaces and removed, should be removed from the workspaces
    // subscription list"
    let workspace_id2 = "other-fake-workspace-id";
    let session = add_listener(&mut listeners, "");
    subscribe_to_workspace(&mut listeners, session, workspace_id);
    subscribe_to_workspace(&mut listeners, session, workspace_id2);
    assert_eq!(listeners.by_id.len(), 1, "one listener");
    assert_eq!(
        workspace_listeners(&listeners, workspace_id),
        [session],
        "subscribed to the first"
    );
    assert_eq!(
        workspace_listeners(&listeners, workspace_id2),
        [session],
        "and to the second"
    );
    listeners.remove(session);
    assert!(listeners.by_id.is_empty(), "no listener is left");
    assert!(
        workspace_listeners(&listeners, workspace_id).is_empty(),
        "nor in the first workspace"
    );
    assert!(
        workspace_listeners(&listeners, workspace_id2).is_empty(),
        "nor in the second"
    );
    assert!(
        listeners.by_workspace.is_empty(),
        "empty workspaces are dropped from the index"
    );
}

/// A workspace that goes to the trash is followed by nobody: each of its listeners stops
/// following it, and what else they follow stays.
#[test]
fn a_workspace_in_the_trash_is_followed_by_nobody() {
    let mut listeners = Listeners::default();
    let one = add_listener(&mut listeners, "a");
    let other = add_listener(&mut listeners, "b");
    for id in [one, other] {
        subscribe_to_workspace(&mut listeners, id, "trashed");
    }
    subscribe_to_workspace(&mut listeners, one, "kept");

    listeners.stop_following_workspace("trashed");
    assert!(
        !listeners.by_workspace.contains_key("trashed"),
        "nobody follows it"
    );
    assert_eq!(
        workspaces_of(&listeners, one),
        ["kept"],
        "the other one stays"
    );
    assert!(
        workspaces_of(&listeners, other).is_empty(),
        "nothing else was followed"
    );
    assert_eq!(workspace_listeners(&listeners, "kept"), [one]);
}

/// `TestTeamSubscription/"Subscribe users to team retrieve by user"`: the listeners of
/// a user in the workspace are the ones that are told about their categories, every tab of
/// the user and no other user's.
#[test]
fn workspace_subscription_subscribe_users_to_workspace_retrieve_by_user() {
    let mut listeners = Listeners::default();
    let workspace_id = "fake-workspace-id";
    let anonymous = add_listener(&mut listeners, "");
    let user1 = add_listener(&mut listeners, "fake-user-id");
    let user2 = add_listener(&mut listeners, "fake-user-id2");
    for id in [anonymous, user1, user2] {
        subscribe_to_workspace(&mut listeners, id, workspace_id);
    }
    assert_eq!(listeners.by_id.len(), 3, "three listeners");
    assert_eq!(
        workspace_listeners(&listeners, workspace_id).len(),
        3,
        "all in the workspace"
    );

    let found = listeners.of_user(workspace_id, "fake-user-id");
    assert_eq!(found, [user1], "the user's listener is found");

    let other_tab = add_listener(&mut listeners, "fake-user-id");
    subscribe_to_workspace(&mut listeners, other_tab, workspace_id);
    let mut found = listeners.of_user(workspace_id, "fake-user-id");
    found.sort_unstable();
    let mut both = vec![user1, other_tab];
    both.sort_unstable();
    assert_eq!(found, both, "both tabs of the user, not the other user's");
    listeners.remove(other_tab);

    for id in [anonymous, user1, user2] {
        listeners.remove(id);
    }
    assert!(listeners.by_id.is_empty(), "no listener is left");
    assert!(
        workspace_listeners(&listeners, workspace_id).is_empty(),
        "nor in the workspace"
    );
    assert_eq!(
        listeners.of_user(workspace_id, "fake-user-id"),
        Vec::<ListenerId>::new(),
        "nobody to find"
    );
}

/// `TestBlocksSubscription`
#[test]
fn blocks_subscription() {
    let mut listeners = Listeners::default();
    let block_ids = [
        "block1".to_owned(),
        "block2".to_owned(),
        "block3".to_owned(),
    ];

    // "Should correctly add a session"
    let session = add_listener(&mut listeners, "");
    assert_eq!(listeners.by_id.len(), 1, "one listener");
    assert!(listeners.by_block.is_empty(), "no block subscriptions yet");

    // "Should correctly subscribe to a set of blocks" and "Subscribing again to a
    // subscribed block would have no effect"
    for round in ["first", "again"] {
        subscribe_to_blocks(&mut listeners, session, &block_ids);
        for block_id in &block_ids {
            assert_eq!(
                block_listeners(&listeners, block_id),
                [session],
                "{round} {block_id}"
            );
        }
        assert_eq!(blocks_of(&listeners, session), block_ids, "{round}");
    }

    // "Should correctly unsubscribe to a set of blocks"
    unsubscribe_from_blocks(&mut listeners, session, &block_ids);
    for block_id in &block_ids {
        assert!(
            block_listeners(&listeners, block_id).is_empty(),
            "{block_id}"
        );
    }
    assert!(
        blocks_of(&listeners, session).is_empty(),
        "the session has no blocks"
    );

    // "Unsubscribing again to an unsubscribed block would have no effect"
    unsubscribe_from_blocks(&mut listeners, session, &block_ids[..1]);
    assert!(
        block_listeners(&listeners, "block1").is_empty(),
        "still not subscribed"
    );
    assert!(blocks_of(&listeners, session).is_empty(), "still no blocks");

    // "Should correctly be removed from the server"
    listeners.remove(session);
    assert!(listeners.by_id.is_empty(), "no listener is left");

    // "If subscribed to blocks and removed, should be removed from the blocks
    // subscription list"
    let session = add_listener(&mut listeners, "");
    subscribe_to_blocks(&mut listeners, session, &block_ids);
    assert_eq!(
        blocks_of(&listeners, session),
        block_ids,
        "subscribed to all three"
    );
    listeners.remove(session);
    assert!(listeners.by_id.is_empty(), "no listener is left");
    for block_id in &block_ids {
        assert!(
            block_listeners(&listeners, block_id).is_empty(),
            "{block_id}"
        );
    }
}

/// The wall of an organization shuts out the connections that follow one of its
/// workspaces with a session made otherwise, and no other.
#[test]
fn a_wall_shuts_out_who_follows_the_organization_with_another_session() {
    let mut listeners = Listeners::default();
    let ours = vec!["ours-1".to_owned(), "ours-2".to_owned()];
    let follows = |listeners: &mut Listeners, provider: Option<&str>, workspace: Option<&str>| {
        let id = add_listener(listeners, "user");
        if let Some(listener) = listeners.by_id.get_mut(&id) {
            listener.login_provider = provider.map(str::to_owned);
        }
        if let Some(workspace) = workspace {
            subscribe_to_workspace(listeners, id, workspace);
        }
        id
    };
    let with_password = follows(&mut listeners, None, Some("ours-2"));
    let through_another = follows(&mut listeners, Some("theirs"), Some("ours-1"));
    let through_ours = follows(&mut listeners, Some("ours"), Some("ours-1"));
    let elsewhere = follows(&mut listeners, None, Some("theirs-1"));
    let following_nothing = follows(&mut listeners, None, None);

    let shut_out = |id: ListenerId| subscribe::shuts_out(&listeners.by_id[&id], &ours, "ours");
    assert!(shut_out(with_password), "a password");
    assert!(shut_out(through_another), "another provider");
    assert!(!shut_out(through_ours), "the organization's provider");
    assert!(!shut_out(elsewhere), "follows another organization");
    assert!(!shut_out(following_nothing), "follows nothing of it");
}

/// A subscription that was admitted before a wall was raised is not made after it: the
/// answer is asked again.
#[test]
fn a_subscription_admitted_before_a_wall_is_not_made_after_it() {
    let mut listeners = Listeners::default();
    let id = add_listener(&mut listeners, "user");
    let asked_at = listeners.walls_raised;

    listeners.walls_raised += 1;
    assert!(
        !subscribe::subscribe_unless_walled(&mut listeners, id, "workspace", asked_at),
        "a wall was raised since"
    );
    assert_eq!(
        workspace_listeners(&listeners, "workspace"),
        Vec::<ListenerId>::new()
    );

    let asked_at = listeners.walls_raised;
    assert!(subscribe::subscribe_unless_walled(
        &mut listeners,
        id,
        "workspace",
        asked_at
    ));
    assert_eq!(workspace_listeners(&listeners, "workspace"), [id]);
}
