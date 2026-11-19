//! Pins the JSON form of the wire types, and what decoding takes and refuses.

use fvoci_model::{
    Block, BlockPatch, BlockPatchBatch, BlockType, Category, CategoryContainerMetadata,
    CategoryContainers, CategoryType, ClientConfig, Container, ContainerKind, ContainerMember,
    ContainerPatch, ContainerType, Credential, InvalidInput, LoginRequest, MinimumRole, NameShown,
    PatchContainersAndBlocks, PersonName, Problem, Sharing, Subscriber, SubscriberType,
    Subscription, Title, User, UserPreferencesPatch, Workspace,
};
use serde_json::{Value, json};

fn to_json<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).expect("wire types serialize to JSON")
}

#[test]
fn block_json_members() {
    let block = Block {
        id: "a1".to_owned(),
        parent_id: "c1".to_owned(),
        created_by: "u1".to_owned(),
        modified_by: "u2".to_owned(),
        schema: 1,
        block_type: BlockType::Comment,
        title: "hello".to_owned(),
        fields: serde_json::from_value(json!({ "z": 1, "a": [true] })).expect("object"),
        body: None,
        create_at: 10,
        update_at: 20,
        delete_at: 0,
        container_id: "b1".to_owned(),
        edited_at: 0,
    };
    let want = json!({
        "id": "a1", "parentId": "c1", "createdBy": "u1", "modifiedBy": "u2", "schema": 1,
        "type": "comment", "title": "hello", "fields": { "a": [true], "z": 1 }, "body": null,
        "createAt": 10, "updateAt": 20, "deleteAt": 0, "containerId": "b1"
    });
    assert_eq!(
        to_json(&block),
        want,
        "a comment never changed has no editedAt"
    );
    assert_eq!(
        serde_json::from_value::<Block>(want).expect("decodes"),
        block
    );
    let edited = Block {
        edited_at: 30,
        ..block
    };
    assert_eq!(to_json(&edited)["editedAt"], json!(30), "one that was has");
}

#[test]
fn block_decodes_missing_members_as_empty_and_refuses_null_ones() {
    let block: Block = serde_json::from_str(r#"{"id":"a1"}"#).expect("decodes");
    assert_eq!(
        block,
        Block {
            id: "a1".to_owned(),
            ..Block::default()
        }
    );
    assert!(
        block.block_type.is_empty(),
        "a missing type is the empty type"
    );
    serde_json::from_str::<Block>(r#"{"id":"a1","fields":null}"#)
        .expect_err("fields is no member that may be null");
    let block: Block =
        serde_json::from_str(r#"{"id":"a1","body":null}"#).expect("body may be null");
    assert_eq!(block.body, None, "a null body is none");
}

#[test]
fn block_type_keeps_unknown_strings_and_parses_strictly() {
    for name in ["card", "view", "comment", "attachment", "page"] {
        let block_type = BlockType::from(name);
        assert!(
            !matches!(block_type, BlockType::Other(_)),
            "{name} is a known type"
        );
        assert_eq!(block_type.as_str(), name);
        assert_eq!(name.parse::<BlockType>(), Ok(block_type));
        name.to_uppercase()
            .parse::<BlockType>()
            .expect_err("a type is parsed as it is written, case and all");
    }

    // Decoding is exact: a different case is some other type, kept as sent.
    assert_eq!(BlockType::from("Card"), BlockType::Other("Card".to_owned()));
    // The content block types of the original, and its board block, are no types of
    // this server.
    assert_eq!(BlockType::from("text"), BlockType::Other("text".to_owned()));
    assert_eq!(
        BlockType::from("board"),
        BlockType::Other("board".to_owned())
    );
    assert_eq!(to_json(&BlockType::from("list-item")), json!("list-item"));
    assert_eq!(
        serde_json::from_value::<BlockType>(json!("h1")).expect("decodes"),
        BlockType::Other("h1".to_owned())
    );

    let error = "h1"
        .parse::<BlockType>()
        .expect_err("h1 is not a server-side type");
    assert_eq!(
        error.to_string(),
        "the text names no block type",
        "the text is not echoed"
    );
}

#[test]
fn block_patch_json_members() {
    let patch: BlockPatch = serde_json::from_value(json!({
        "parentId": "p", "schema": 2, "type": "card", "title": "t",
        "updatedFields": { "a": 1 }, "deletedFields": ["b"]
    }))
    .expect("decodes");
    assert_eq!(patch.parent_id.as_deref(), Some("p"));
    assert_eq!(patch.schema, Some(2));
    assert_eq!(patch.block_type, Some(BlockType::Card));
    assert_eq!(patch.title.as_deref(), Some("t"));
    assert_eq!(patch.updated_fields["a"], json!(1));
    assert_eq!(patch.deleted_fields, ["b"]);

    // A member that may be null is none when it is; the lists may only be left out.
    let nulls: BlockPatch = serde_json::from_value(json!({
        "parentId": null, "schema": null, "type": null, "title": null,
    }))
    .expect("decodes");
    assert_eq!(nulls, BlockPatch::default());
    for list in ["updatedFields", "deletedFields"] {
        serde_json::from_value::<BlockPatch>(json!({ list: null }))
            .expect_err("a null list is refused");
    }
    assert_eq!(
        to_json(&BlockPatch::default()),
        json!({ "updatedFields": {}, "deletedFields": [] })
    );

    let batch: BlockPatchBatch =
        serde_json::from_value(json!({ "blockIds": ["a"], "blockPatches": [{ "title": "t" }] }))
            .expect("decodes");
    assert_eq!(batch.block_ids, ["a"]);
    assert_eq!(batch.block_patches[0].title.as_deref(), Some("t"));
}

#[test]
fn block_patch_apply_updates_then_deletes() {
    let mut block = Block {
        title: "old".to_owned(),
        fields: serde_json::from_value(json!({ "keep": 1, "drop": 2, "both": 3 })).expect("object"),
        ..Block::default()
    };
    let patch: BlockPatch = serde_json::from_value(json!({
        "title": "new",
        "updatedFields": { "added": 4, "both": 5 },
        "deletedFields": ["drop", "both", "absent"]
    }))
    .expect("decodes");

    patch.apply(&mut block);

    assert_eq!(block.title, "new");
    assert_eq!(
        Value::Object(block.fields),
        json!({ "keep": 1, "added": 4 })
    );
}

#[test]
fn container_json_members() {
    let board = Container {
        id: "b1".to_owned(),
        workspace_id: "0".to_owned(),
        created_by: "u1".to_owned(),
        modified_by: "u1".to_owned(),
        container_type: ContainerType::Private,
        kind: ContainerKind::Board,
        minimum_role: MinimumRole::Commenter,
        title: "Roadmap".to_owned(),
        description: "d".to_owned(),
        icon: "🚀".to_owned(),
        show_description: true,
        is_template: false,
        is_builtin: false,
        template_version: 6,
        properties: serde_json::from_value(json!({ "k": "v" })).expect("object"),
        card_properties: serde_json::from_value(json!([{ "id": "p1", "name": "Status" }]))
            .expect("list of objects"),
        create_at: 1,
        update_at: 2,
        delete_at: 0,
    };
    let want = json!({
        "id": "b1", "workspaceId": "0", "createdBy": "u1", "modifiedBy": "u1",
        "type": "P", "kind": "board", "minimumRole": "commenter", "title": "Roadmap", "description": "d",
        "icon": "🚀", "showDescription": true, "isTemplate": false, "isBuiltin": false, "templateVersion": 6,
        "properties": { "k": "v" }, "cardProperties": [{ "id": "p1", "name": "Status" }],
        "createAt": 1, "updateAt": 2, "deleteAt": 0
    });
    assert_eq!(to_json(&board), want);
    assert_eq!(
        serde_json::from_value::<Container>(want).expect("decodes"),
        board
    );
}

#[test]
fn container_requires_a_valid_type_and_role() {
    let minimal: Container = serde_json::from_value(json!({ "type": "O" })).expect("decodes");
    assert_eq!(minimal, Container::default());
    // A board always has a minimum role. One that names none starts with editor.
    assert_eq!(minimal.minimum_role, MinimumRole::Editor);
    assert_eq!(to_json(&minimal)["minimumRole"], json!("editor"));
    serde_json::from_value::<Container>(json!({ "type": "O", "minimumRole": null }))
        .expect_err("a null minimum role is refused: it may only be left out");
    for role in MinimumRole::ALL {
        let board: Container =
            serde_json::from_value(json!({ "type": "O", "minimumRole": role.as_str() }))
                .expect("decodes");
        assert_eq!(board.minimum_role, role);
    }

    // Empty is no minimum role, and neither is admin: nobody becomes an administrator
    // of a board by joining it.
    for body in [
        json!({}),
        json!({ "type": "" }),
        json!({ "type": "X" }),
        json!({ "type": "O", "minimumRole": "owner" }),
        json!({ "type": "O", "minimumRole": "" }),
        json!({ "type": "O", "minimumRole": "admin" }),
        json!({ "type": "O", "minimumRole": "Editor" }),
    ] {
        assert!(
            serde_json::from_value::<Container>(body.clone()).is_err(),
            "{body} must not decode"
        );
    }
}

#[test]
fn container_patch_apply_merges_card_properties_in_order() {
    let mut board = Container {
        properties: serde_json::from_value(json!({ "keep": 1, "drop": 2 })).expect("object"),
        card_properties: serde_json::from_value(json!([
            { "id": "a", "name": "A" },
            { "name": "no id" },
            { "id": "b", "name": "B" },
            { "id": "c", "name": "C" }
        ]))
        .expect("list of objects"),
        ..Container::default()
    };
    let patch: ContainerPatch = serde_json::from_value(json!({
        "type": "P", "minimumRole": "viewer", "title": "t", "description": "d", "icon": "i",
        "showDescription": true,
        "updatedProperties": { "new": 3 }, "deletedProperties": ["drop"],
        "updatedCardProperties": [{ "id": "b", "name": "B2" }, { "id": "d", "name": "D" }, { "name": "skipped" }],
        "deletedCardProperties": ["a"]
    }))
    .expect("decodes");

    patch.apply(&mut board);

    assert_eq!(board.container_type, ContainerType::Private);
    assert_eq!(board.minimum_role, MinimumRole::Viewer);
    assert_eq!(
        (board.title.as_str(), board.description.as_str()),
        ("t", "d")
    );
    assert_eq!(board.icon, "i");
    assert!(board.show_description, "showDescription is set");
    assert_eq!(
        Value::Object(board.properties),
        json!({ "keep": 1, "new": 3 })
    );
    assert_eq!(
        to_json(&board.card_properties),
        json!([{ "id": "b", "name": "B2" }, { "id": "c", "name": "C" }, { "id": "d", "name": "D" }])
    );
}

#[test]
fn container_patch_without_card_property_changes_keeps_them_untouched() {
    let card_properties = json!([{ "name": "no id" }, { "id": "a" }]);
    let mut board = Container {
        card_properties: serde_json::from_value(card_properties.clone()).expect("list"),
        ..Container::default()
    };

    ContainerPatch {
        title: Some("t".to_owned()),
        ..ContainerPatch::default()
    }
    .apply(&mut board);

    assert_eq!(to_json(&board.card_properties), card_properties);
}

/// A property that the same patch deletes and updates comes out once, updated, at its
/// place. (In the original's merge it comes out twice.)
#[test]
fn container_patch_deleting_and_updating_a_card_property_keeps_one() {
    let mut board = Container {
        card_properties: serde_json::from_value(json!([{ "id": "a", "v": 1 }, { "id": "b" }]))
            .expect("list"),
        ..Container::default()
    };
    let patch: ContainerPatch = serde_json::from_value(json!({
        "updatedCardProperties": [{ "id": "a", "v": 2 }],
        "deletedCardProperties": ["a"]
    }))
    .expect("decodes");

    patch.apply(&mut board);

    assert_eq!(
        to_json(&board.card_properties),
        json!([{ "id": "a", "v": 2 }, { "id": "b" }]),
        "one definition per identifier"
    );
}

#[test]
fn container_member_json_members() {
    let member = ContainerMember {
        container_id: "b".to_owned(),
        user_id: "u".to_owned(),
        roles: "r".to_owned(),
        scheme_admin: true,
        scheme_editor: false,
        scheme_commenter: true,
        scheme_viewer: false,
        synthetic: true,
        inherited_role: Some(fvoci_model::MinimumRole::Editor),
    };
    // Direct flags and inherited roles remain distinguishable on the wire.
    assert_eq!(
        to_json(&member),
        json!({
            "containerId": "b", "userId": "u",
            "schemeAdmin": true, "schemeEditor": false, "schemeCommenter": true,
            "schemeViewer": false, "synthetic": true, "inheritedRole": "editor"
        })
    );
}

#[test]
fn patch_containers_and_blocks_json_members() {
    let patch: PatchContainersAndBlocks = serde_json::from_value(json!({
        "containerIds": ["b"], "containerPatches": [{ "title": "t" }],
        "blockIds": ["a"], "blockPatches": [{ "title": "u" }]
    }))
    .expect("decodes");
    assert_eq!((patch.container_ids.len(), patch.block_ids.len()), (1, 1));
    assert_eq!(patch.container_patches[0].title.as_deref(), Some("t"));
    assert_eq!(patch.block_patches[0].title.as_deref(), Some("u"));
}

#[test]
fn category_json_members() {
    let category = Category {
        id: "7c".to_owned(),
        name: "Boards".to_owned(),
        user_id: "u".to_owned(),
        workspace_id: "0".to_owned(),
        create_at: 1,
        update_at: 2,
        delete_at: 0,
        collapsed: true,
        sort_order: 20,
        sorting: "alpha".to_owned(),
        category_type: CategoryType::System,
    };
    // The sorting method has no place in the storage and is not sent.
    let want = json!({
        "id": "7c", "name": "Boards", "userId": "u", "workspaceId": "0", "createAt": 1,
        "updateAt": 2, "deleteAt": 0, "collapsed": true, "sortOrder": 20,
        "type": "system"
    });
    assert_eq!(to_json(&category), want);
    assert_eq!(
        serde_json::from_value::<Category>(want).expect("decodes"),
        Category {
            sorting: String::new(),
            ..category
        }
    );

    let defaulted: Category = serde_json::from_value(json!({ "name": "n" })).expect("decodes");
    assert_eq!(defaulted.category_type, CategoryType::Custom);
}

#[test]
fn category_containers_json_flattens_the_category() {
    let category_boards = CategoryContainers {
        category: Category {
            id: "7c".to_owned(),
            name: "Boards".to_owned(),
            sort_order: 30,
            ..Category::default()
        },
        container_metadata: vec![CategoryContainerMetadata {
            container_id: "b1".to_owned(),
            hidden: true,
        }],
    };
    assert_eq!(
        to_json(&category_boards),
        json!({
            "id": "7c", "name": "Boards", "userId": "", "workspaceId": "", "createAt": 0,
            "updateAt": 0, "deleteAt": 0, "collapsed": false, "sortOrder": 30,
            "type": "custom", "containerMetadata": [{ "containerId": "b1", "hidden": true }]
        })
    );
}

#[test]
fn category_hydrate_and_validate() {
    let mut category = Category {
        name: "n".to_owned(),
        user_id: "u".to_owned(),
        workspace_id: "t".to_owned(),
        sort_order: -5,
        ..Category::default()
    };
    assert_eq!(
        category.validate().expect_err("no id yet").to_string(),
        "the category has no id"
    );

    category.hydrate(77);

    assert!(
        fvoci_model::is_id(&category.id),
        "category id {}",
        category.id
    );
    assert_eq!((category.create_at, category.update_at), (77, 77));
    assert_eq!(category.sort_order, 0);
    assert_eq!(category.validate(), Ok(()));

    let blank_name = Category {
        name: "  ".to_owned(),
        ..category.clone()
    };
    assert_eq!(
        blank_name.validate().expect_err("blank name").to_string(),
        "the category has no name"
    );
    let no_user = Category {
        user_id: String::new(),
        ..category.clone()
    };
    assert_eq!(
        no_user.validate().expect_err("no user").to_string(),
        "the category names no user"
    );
    let no_workspace = Category {
        workspace_id: String::new(),
        ..category
    };
    assert_eq!(
        no_workspace
            .validate()
            .expect_err("no workspace")
            .to_string(),
        "the category names no workspace"
    );
}

#[test]
fn user_json_hides_credentials_and_carries_the_name() {
    let user = User {
        id: "u1".to_owned(),
        username: "alice".to_owned(),
        display_name: "Alice A.".to_owned(),
        name: PersonName {
            first_name: "Alice".to_owned(),
            last_name: "A".to_owned(),
            custom_name: "Alice A.".to_owned(),
            name_shown: NameShown::CustomName,
            title: Some(Title::Dr),
            credentials: vec![Credential::Phd, Credential::KrAttorney],
            ..PersonName::default()
        },
        email: "alice@example.com".to_owned(),
        password_hash: "hash".to_owned(),
        create_at: 0,
        update_at: 5,
        delete_at: 0,
        is_admin: true,
        is_bot: false,
        is_guest: true,
        permissions: vec![],
        roles: "system_user".to_owned(),
    };
    assert_eq!(
        to_json(&user),
        json!({
            "id": "u1", "username": "alice", "displayName": "Alice A.", "firstName": "Alice",
            "lastName": "A", "latinFirstName": "", "latinLastName": "",
            "customName": "Alice A.", "nameShown": "custom_name", "title": "dr",
            "credentials": ["phd", "kr_attorney"], "createAt": 0, "updateAt": 5,
            "deleteAt": 0, "isAdmin": true
        })
    );

    let debug = format!("{user:?}");
    assert!(!debug.contains("hash"), "Debug leaks: {debug}");

    let decoded: User = serde_json::from_value(json!({
        "id": "u1", "email": "x@example.com", "password": "p", "permissions": null
    }))
    .expect("decodes");
    assert_eq!(
        (decoded.email.as_str(), decoded.password_hash.as_str()),
        ("", "")
    );
}

#[test]
fn workspace_and_sharing_json_members() {
    let workspace = Workspace {
        id: "0".to_owned(),
        title: "t".to_owned(),
        settings: serde_json::from_value(json!({ "a": 1 })).expect("object"),
        modified_by: "u".to_owned(),
        update_at: 3,
        guest_invitations: false,
    };
    assert_eq!(
        to_json(&workspace),
        json!({
            "id": "0", "title": "t", "settings": { "a": 1 },
            "modifiedBy": "u", "updateAt": 3, "guestInvitations": false
        })
    );

    // What is answered about sharing: whether, the token while there is one, and when.
    // Whose board it is and who changed it are not part of the answer.
    let sharing = Sharing {
        container_id: "b".to_owned(),
        enabled: true,
        token: "tok".to_owned(),
        modified_by: "u".to_owned(),
        update_at: 9,
    };
    assert_eq!(
        to_json(&sharing),
        json!({ "enabled": true, "token": "tok", "updateAt": 9 })
    );
    assert_eq!(
        to_json(&Sharing {
            container_id: "b".to_owned(),
            ..Sharing::default()
        }),
        json!({ "enabled": false }),
        "a board that was never shared"
    );
}

#[test]
fn subscription_json_members() {
    let subscription = Subscription {
        block_type: BlockType::Card,
        block_id: "c".to_owned(),
        subscriber_type: SubscriberType::User,
        subscriber_id: "u".to_owned(),
        notified_at: 0,
        create_at: 1,
        delete_at: 0,
    };
    let want = json!({
        "blockType": "card", "blockId": "c", "subscriberType": "user", "subscriberId": "u",
        "notifiedAt": 0, "createAt": 1, "deleteAt": 0
    });
    assert_eq!(to_json(&subscription), want);
    assert_eq!(
        serde_json::from_value::<Subscription>(want).expect("decodes"),
        subscription
    );
    assert!(
        serde_json::from_value::<Subscription>(json!({ "subscriberType": "workspace" })).is_err(),
        "an unknown subscriber type must not decode"
    );

    let subscriber = Subscriber {
        subscriber_type: SubscriberType::Channel,
        subscriber_id: "ch".to_owned(),
        notified_at: 4,
    };
    assert_eq!(
        to_json(&subscriber),
        json!({ "subscriberType": "channel", "subscriberId": "ch", "notifiedAt": 4 })
    );
}

#[test]
fn config_error_and_request_json_members() {
    assert_eq!(
        to_json(&ClientConfig {
            max_file_bytes: 7,
            max_import_bytes: 9,
            allow_public_links: true,
            name_display: "username".to_owned(),
            ..ClientConfig::default()
        }),
        json!({
            "allowPublicLinks": true,
            "nameDisplay": "username", "featureFlags": {}, "maxFileBytes": 7,
            "maxImportBytes": 9, "searchEnabled": false
        })
    );

    // A problem says its type, title, status and detail; the other members appear only
    // when set.
    assert_eq!(
        to_json(&Problem {
            problem_type: "/problems/not-found".to_owned(),
            title: "Not found".to_owned(),
            status: 404,
            detail: "board not found".to_owned(),
            ..Problem::default()
        }),
        json!({
            "type": "/problems/not-found", "title": "Not found", "status": 404,
            "detail": "board not found"
        })
    );
    assert_eq!(
        to_json(&Problem {
            problem_type: "/problems/r".to_owned(),
            title: "R".to_owned(),
            status: 413,
            detail: "the request body is too large".to_owned(),
            instance: Some("urn:uuid:id".to_owned()),
            reason: Some("r".to_owned()),
            errors: vec![
                InvalidInput {
                    pointer: Some("/f".to_owned()),
                    detail: "too long".to_owned(),
                    reason: "too_long".to_owned(),
                    ..InvalidInput::default()
                },
                InvalidInput {
                    parameter: Some("page".to_owned()),
                    detail: "no number".to_owned(),
                    reason: "invalid".to_owned(),
                    ..InvalidInput::default()
                },
            ],
            limit: Some(4_194_304),
            retry_after: Some(30),
            request_id: Some("id".to_owned()),
            org_id: Some("org".to_owned()),
        }),
        json!({
            "type": "/problems/r",
            "title": "R",
            "status": 413,
            "detail": "the request body is too large",
            "instance": "urn:uuid:id",
            "reason": "r",
            "errors": [
                { "pointer": "/f", "detail": "too long", "reason": "too_long" },
                { "parameter": "page", "detail": "no number", "reason": "invalid" },
            ],
            "limit": 4_194_304,
            "retryAfter": 30,
            "requestId": "id",
            "orgId": "org",
        })
    );

    // Members a client of the original sends along are not read and not refused.
    let login: LoginRequest = serde_json::from_value(json!({
        "type": "normal", "username": "u", "email": "e", "password": "p", "mfa_token": "m"
    }))
    .expect("decodes");
    assert_eq!((login.username.as_str(), login.email.as_str()), ("u", "e"));
    assert!(
        !format!("{login:?}").contains("\"p\""),
        "Debug leaks the password"
    );

    let preferences: UserPreferencesPatch =
        serde_json::from_value(json!({ "updatedFields": { "a": "b" } })).expect("decodes");
    assert_eq!(preferences.updated_fields["a"], "b");
    assert!(
        preferences.deleted_fields.is_empty(),
        "a list left out is empty"
    );
    serde_json::from_value::<UserPreferencesPatch>(
        json!({ "updatedFields": {}, "deletedFields": null }),
    )
    .expect_err("a null list is refused");
}

#[test]
fn subscription_validate() {
    let valid = Subscription {
        block_type: BlockType::Card,
        block_id: "c".to_owned(),
        subscriber_type: SubscriberType::User,
        subscriber_id: "u".to_owned(),
        notified_at: 0,
        create_at: 0,
        delete_at: 0,
    };
    assert_eq!(valid.validate(), Ok(()));

    let cases = [
        (
            Subscription {
                block_id: String::new(),
                ..valid.clone()
            },
            "missing block id",
        ),
        (
            Subscription {
                block_type: BlockType::default(),
                ..valid.clone()
            },
            "missing block type",
        ),
        (
            Subscription {
                subscriber_id: String::new(),
                ..valid
            },
            "missing subscriber id",
        ),
    ];
    for (subscription, message) in cases {
        let error = subscription.validate().expect_err(message);
        assert_eq!(error.to_string(), message);
    }
}

/// `null` is a value only for a member that may be `null` (an `Option`, `null` in the
/// contract's schema), and is then none. For every other member it is refused, as any
/// value of the wrong type is: a member that may be left out is left out instead.
#[test]
fn null_is_refused_for_members_that_are_not_nullable() {
    /// `base` decodes; each of its members is refused when it is null, and may be left
    /// out unless it is one of `required`.
    fn refuses<T: serde::de::DeserializeOwned>(base: &Value, required: &[&str], what: &str) {
        assert!(
            serde_json::from_value::<T>(base.clone()).is_ok(),
            "{what} decodes"
        );
        let members = base.as_object().expect("an object");
        for member in members.keys() {
            let mut nulled = base.clone();
            nulled[member.as_str()] = Value::Null;
            assert!(
                serde_json::from_value::<T>(nulled).is_err(),
                "{what}: a null {member} is refused"
            );
            if required.contains(&member.as_str()) {
                continue;
            }
            let mut left_out = base.clone();
            left_out
                .as_object_mut()
                .expect("an object")
                .remove(member.as_str());
            assert!(
                serde_json::from_value::<T>(left_out).is_ok(),
                "{what}: {member} may be left out"
            );
        }
    }

    refuses::<Block>(
        &json!({
            "id": "a1", "parentId": "", "createdBy": "u1", "modifiedBy": "u1", "schema": 1,
            "type": "card", "title": "t", "fields": {}, "createAt": 1, "updateAt": 1,
            "deleteAt": 0, "containerId": "b1",
        }),
        &[],
        "a block",
    );
    refuses::<Container>(
        &json!({
            "id": "b1", "workspaceId": "w1", "createdBy": "u1", "modifiedBy": "u1",
            "type": "P", "minimumRole": "editor", "title": "t", "description": "", "icon": "",
            "showDescription": false, "isTemplate": false, "templateVersion": 0,
            "properties": {}, "cardProperties": [], "createAt": 1, "updateAt": 1,
            "deleteAt": 0,
        }),
        &["type"],
        "a board",
    );
    refuses::<ContainerMember>(
        &json!({
            "containerId": "b1", "userId": "u1", "schemeAdmin": false, "schemeEditor": false, "schemeCommenter": false,
            "schemeViewer": false,
        }),
        &[],
        "a member",
    );
    refuses::<Category>(
        &json!({
            "id": "c1", "name": "n", "userId": "u1", "workspaceId": "w1", "createAt": 1,
            "updateAt": 1, "deleteAt": 0, "collapsed": false, "sortOrder": 0,
        }),
        &[],
        "a category",
    );
    refuses::<LoginRequest>(
        &json!({ "username": "u", "password": "p" }),
        &[],
        "a login request",
    );

    // A member that may be null is none when it is.
    let block: Block =
        serde_json::from_value(json!({ "id": "a1", "body": null })).expect("decodes");
    assert_eq!(block.body, None, "a null body is no body");
}
