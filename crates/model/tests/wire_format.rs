//! Pins the JSON form of the wire types against the member names of the original structs
//! (`server/model/*.go`), and the leniency of decoding.

use fvoci_model::{
    Block, BlockPatch, BlockPatchBatch, BlockType, Board, BoardMember, BoardPatch, BoardRole,
    BoardType, Category, CategoryBoardMetadata, CategoryBoards, CategoryType, ClientConfig,
    ErrorBody, LoginRequest, PatchBoardsAndBlocks, Sharing, Subscriber, SubscriberType,
    Subscription, Team, User, UserPreferencesPatch,
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
        block_type: BlockType::Text,
        title: "hello".to_owned(),
        fields: serde_json::from_value(json!({ "z": 1, "a": [true] })).expect("object"),
        create_at: 10,
        update_at: 20,
        delete_at: 0,
        board_id: "b1".to_owned(),
    };
    let want = json!({
        "id": "a1", "parentId": "c1", "createdBy": "u1", "modifiedBy": "u2", "schema": 1,
        "type": "text", "title": "hello", "fields": { "a": [true], "z": 1 },
        "createAt": 10, "updateAt": 20, "deleteAt": 0, "boardId": "b1"
    });
    assert_eq!(to_json(&block), want);
    assert_eq!(
        serde_json::from_value::<Block>(want).expect("decodes"),
        block
    );
}

#[test]
fn block_decodes_missing_and_null_members_as_empty() {
    let block: Block = serde_json::from_str(r#"{"id":"a1","fields":null}"#).expect("decodes");
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
}

#[test]
fn block_type_keeps_unknown_strings_and_parses_strictly() {
    for name in [
        "board",
        "card",
        "view",
        "text",
        "checkbox",
        "comment",
        "image",
        "attachment",
        "divider",
    ] {
        let block_type = BlockType::from(name);
        assert!(
            !matches!(block_type, BlockType::Other(_)),
            "{name} is a known type"
        );
        assert_eq!(block_type.as_str(), name);
        assert_eq!(name.to_uppercase().parse::<BlockType>(), Ok(block_type));
    }

    // Decoding is exact: a different case is some other type, kept as sent.
    assert_eq!(BlockType::from("Card"), BlockType::Other("Card".to_owned()));
    assert_eq!(to_json(&BlockType::from("list-item")), json!("list-item"));
    assert_eq!(
        serde_json::from_value::<BlockType>(json!("h1")).expect("decodes"),
        BlockType::Other("h1".to_owned())
    );

    let error = "h1"
        .parse::<BlockType>()
        .expect_err("h1 is not a server-side type");
    assert_eq!(error.to_string(), "h1 is an invalid block type.");
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

    // The original's own client sends null for everything it did not set.
    let nulls: BlockPatch = serde_json::from_value(json!({
        "parentId": null, "schema": null, "type": null, "title": null,
        "updatedFields": null, "deletedFields": null
    }))
    .expect("decodes");
    assert_eq!(nulls, BlockPatch::default());
    assert_eq!(
        to_json(&BlockPatch::default()),
        json!({ "updatedFields": {}, "deletedFields": [] })
    );

    let batch: BlockPatchBatch =
        serde_json::from_value(json!({ "block_ids": ["a"], "block_patches": [{ "title": "t" }] }))
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
fn board_json_members() {
    let board = Board {
        id: "b1".to_owned(),
        team_id: "0".to_owned(),
        created_by: "u1".to_owned(),
        modified_by: "u1".to_owned(),
        board_type: BoardType::Private,
        minimum_role: BoardRole::Editor,
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
        "id": "b1", "teamId": "0", "createdBy": "u1", "modifiedBy": "u1",
        "type": "P", "minimumRole": "editor", "title": "Roadmap", "description": "d",
        "icon": "🚀", "showDescription": true, "isTemplate": false, "isBuiltin": false, "templateVersion": 6,
        "properties": { "k": "v" }, "cardProperties": [{ "id": "p1", "name": "Status" }],
        "createAt": 1, "updateAt": 2, "deleteAt": 0
    });
    assert_eq!(to_json(&board), want);
    assert_eq!(
        serde_json::from_value::<Board>(want).expect("decodes"),
        board
    );
}

#[test]
fn board_requires_a_valid_type_and_role() {
    let minimal: Board = serde_json::from_value(json!({ "type": "O" })).expect("decodes");
    assert_eq!(minimal, Board::default());
    assert_eq!(minimal.minimum_role, BoardRole::None);
    assert_eq!(to_json(&minimal)["minimumRole"], json!(""));

    for body in [
        json!({}),
        json!({ "type": "" }),
        json!({ "type": "X" }),
        json!({ "type": "O", "minimumRole": "owner" }),
    ] {
        assert!(
            serde_json::from_value::<Board>(body.clone()).is_err(),
            "{body} must not decode"
        );
    }
}

#[test]
fn board_patch_apply_merges_card_properties_in_order() {
    let mut board = Board {
        properties: serde_json::from_value(json!({ "keep": 1, "drop": 2 })).expect("object"),
        card_properties: serde_json::from_value(json!([
            { "id": "a", "name": "A" },
            { "name": "no id" },
            { "id": "b", "name": "B" },
            { "id": "c", "name": "C" }
        ]))
        .expect("list of objects"),
        ..Board::default()
    };
    let patch: BoardPatch = serde_json::from_value(json!({
        "type": "P", "minimumRole": "viewer", "title": "t", "description": "d", "icon": "i",
        "showDescription": true,
        "updatedProperties": { "new": 3 }, "deletedProperties": ["drop"],
        "updatedCardProperties": [{ "id": "b", "name": "B2" }, { "id": "d", "name": "D" }, { "name": "skipped" }],
        "deletedCardProperties": ["a"]
    }))
    .expect("decodes");

    patch.apply(&mut board);

    assert_eq!(board.board_type, BoardType::Private);
    assert_eq!(board.minimum_role, BoardRole::Viewer);
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
fn board_patch_without_card_property_changes_keeps_them_untouched() {
    let card_properties = json!([{ "name": "no id" }, { "id": "a" }]);
    let mut board = Board {
        card_properties: serde_json::from_value(card_properties.clone()).expect("list"),
        ..Board::default()
    };

    BoardPatch {
        title: Some("t".to_owned()),
        ..BoardPatch::default()
    }
    .apply(&mut board);

    assert_eq!(to_json(&board.card_properties), card_properties);
}

/// A property that the same patch deletes and updates comes out once, updated, at its
/// place. (In the original's merge it comes out twice.)
#[test]
fn board_patch_deleting_and_updating_a_card_property_keeps_one() {
    let mut board = Board {
        card_properties: serde_json::from_value(json!([{ "id": "a", "v": 1 }, { "id": "b" }]))
            .expect("list"),
        ..Board::default()
    };
    let patch: BoardPatch = serde_json::from_value(json!({
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
fn board_member_json_members() {
    let member = BoardMember {
        board_id: "b".to_owned(),
        user_id: "u".to_owned(),
        roles: "r".to_owned(),
        minimum_role: "viewer".to_owned(),
        scheme_admin: true,
        scheme_editor: false,
        scheme_commenter: true,
        scheme_viewer: false,
        synthetic: true,
    };
    assert_eq!(
        to_json(&member),
        json!({
            "boardId": "b", "userId": "u", "roles": "r", "minimumRole": "viewer",
            "schemeAdmin": true, "schemeEditor": false, "schemeCommenter": true,
            "schemeViewer": false, "synthetic": true
        })
    );
}

#[test]
fn patch_boards_and_blocks_json_members() {
    let patch: PatchBoardsAndBlocks = serde_json::from_value(json!({
        "boardIDs": ["b"], "boardPatches": [{ "title": "t" }],
        "blockIDs": ["a"], "blockPatches": [{ "title": "u" }]
    }))
    .expect("decodes");
    assert_eq!((patch.board_ids.len(), patch.block_ids.len()), (1, 1));
    assert_eq!(patch.board_patches[0].title.as_deref(), Some("t"));
    assert_eq!(patch.block_patches[0].title.as_deref(), Some("u"));
}

#[test]
fn category_json_members() {
    let category = Category {
        id: "7c".to_owned(),
        name: "Boards".to_owned(),
        user_id: "u".to_owned(),
        team_id: "0".to_owned(),
        create_at: 1,
        update_at: 2,
        delete_at: 0,
        collapsed: true,
        sort_order: 20,
        sorting: "alpha".to_owned(),
        category_type: CategoryType::System,
    };
    let want = json!({
        "id": "7c", "name": "Boards", "userID": "u", "teamID": "0", "createAt": 1,
        "updateAt": 2, "deleteAt": 0, "collapsed": true, "sortOrder": 20,
        "sorting": "alpha", "type": "system"
    });
    assert_eq!(to_json(&category), want);
    assert_eq!(
        serde_json::from_value::<Category>(want).expect("decodes"),
        category
    );

    let defaulted: Category = serde_json::from_value(json!({ "name": "n" })).expect("decodes");
    assert_eq!(defaulted.category_type, CategoryType::Custom);
}

/// Preserved quirk: the category's sort order never reaches the wire through
/// `CategoryBoards`.
#[test]
fn category_boards_json_flattens_the_category_and_zeroes_sort_order() {
    let category_boards = CategoryBoards {
        category: Category {
            id: "7c".to_owned(),
            name: "Boards".to_owned(),
            sort_order: 30,
            ..Category::default()
        },
        board_metadata: vec![CategoryBoardMetadata {
            board_id: "b1".to_owned(),
            hidden: true,
        }],
    };
    assert_eq!(
        to_json(&category_boards),
        json!({
            "id": "7c", "name": "Boards", "userID": "", "teamID": "", "createAt": 0,
            "updateAt": 0, "deleteAt": 0, "collapsed": false, "sortOrder": 0, "sorting": "",
            "type": "custom", "boardMetadata": [{ "boardID": "b1", "hidden": true }]
        })
    );
}

#[test]
fn category_hydrate_and_validate() {
    let mut category = Category {
        name: "n".to_owned(),
        user_id: "u".to_owned(),
        team_id: "t".to_owned(),
        sort_order: -5,
        ..Category::default()
    };
    assert_eq!(
        category.validate().expect_err("no id yet").to_string(),
        "category ID cannot be empty"
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
        "category name cannot be empty"
    );
    let no_user = Category {
        user_id: String::new(),
        ..category.clone()
    };
    assert_eq!(
        no_user.validate().expect_err("no user").to_string(),
        "category user ID cannot be empty"
    );
    let no_team = Category {
        team_id: String::new(),
        ..category
    };
    assert_eq!(
        no_team.validate().expect_err("no team").to_string(),
        "category team id ID cannot be empty"
    );
}

#[test]
fn user_json_hides_credentials_and_empty_members() {
    let user = User {
        id: "u1".to_owned(),
        username: "alice".to_owned(),
        email: "alice@example.com".to_owned(),
        nickname: "al".to_owned(),
        first_name: "Alice".to_owned(),
        last_name: "A".to_owned(),
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
            "id": "u1", "username": "alice", "nickname": "al", "firstname": "Alice",
            "lastname": "A", "update_at": 5, "delete_at": 0, "isAdmin": true,
            "is_bot": false, "is_guest": true, "roles": "system_user"
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
fn team_and_sharing_json_members() {
    let team = Team {
        id: "0".to_owned(),
        title: "t".to_owned(),
        signup_token: "tok".to_owned(),
        settings: serde_json::from_value(json!({ "a": 1 })).expect("object"),
        modified_by: "u".to_owned(),
        update_at: 3,
    };
    assert_eq!(
        to_json(&team),
        json!({
            "id": "0", "title": "t", "signupToken": "tok", "settings": { "a": 1 },
            "modifiedBy": "u", "updateAt": 3
        })
    );

    // What is answered about sharing: whether, the token while there is one, and when.
    // Whose board it is and who changed it are not part of the answer.
    let sharing = Sharing {
        board_id: "b".to_owned(),
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
            board_id: "b".to_owned(),
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
        "createAt": 1, "deleteAt": 0
    });
    assert_eq!(to_json(&subscription), want);
    assert_eq!(
        serde_json::from_value::<Subscription>(want).expect("decodes"),
        subscription
    );
    assert!(
        serde_json::from_value::<Subscription>(json!({ "subscriberType": "team" })).is_err(),
        "an unknown subscriber type must not decode"
    );

    let subscriber = Subscriber {
        subscriber_type: SubscriberType::Channel,
        subscriber_id: "ch".to_owned(),
        notified_at: 4,
    };
    assert_eq!(
        to_json(&subscriber),
        json!({ "subscriber_type": "channel", "subscriber_id": "ch", "notified_at": 4 })
    );
}

#[test]
fn config_error_and_request_json_members() {
    assert_eq!(
        to_json(&ClientConfig {
            max_file_size: 7,
            enable_public_shared_boards: true,
            teammate_name_display: "username".to_owned(),
            ..ClientConfig::default()
        }),
        json!({
            "telemetry": false, "telemetryid": "", "enablePublicSharedBoards": true,
            "teammateNameDisplay": "username", "featureFlags": {}, "maxFileSize": 7
        })
    );

    // An error says its kind and a sentence; the optional members appear only when set.
    assert_eq!(
        to_json(&ErrorBody {
            error: "not_found".to_owned(),
            message: "board not found".to_owned(),
            ..ErrorBody::default()
        }),
        json!({ "error": "not_found", "message": "board not found" })
    );
    assert_eq!(
        to_json(&ErrorBody {
            error: "payload_too_large".to_owned(),
            message: "the request body is too large".to_owned(),
            reason: Some("r".to_owned()),
            field: Some("f".to_owned()),
            limit: Some(4_194_304),
            request_id: Some("id".to_owned()),
        }),
        json!({
            "error": "payload_too_large",
            "message": "the request body is too large",
            "reason": "r",
            "field": "f",
            "limit": 4_194_304,
            "request_id": "id",
        })
    );

    let login: LoginRequest = serde_json::from_value(json!({
        "type": "normal", "username": "u", "email": "e", "password": "p", "mfa_token": "m"
    }))
    .expect("decodes");
    assert_eq!(
        (login.login_type.as_str(), login.mfa_token.as_str()),
        ("normal", "m")
    );
    assert!(
        !format!("{login:?}").contains("\"p\""),
        "Debug leaks the password"
    );

    let preferences: UserPreferencesPatch =
        serde_json::from_value(json!({ "updatedFields": { "a": "b" }, "deletedFields": null }))
            .expect("decodes");
    assert_eq!(preferences.updated_fields["a"], "b");
    assert!(
        preferences.deleted_fields.is_empty(),
        "null decodes as empty"
    );
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

/// The original's decoder leaves a member at its zero value when the JSON says `null`,
/// and clients rely on it (a block with `"title": null` is accepted). Verified on the
/// original: `POST /boards/{id}/blocks` with `{"title": null, "parentId": null, ...}`
/// answers 200.
#[test]
fn null_members_decode_as_empty_values() {
    let block: Block = serde_json::from_value(json!({
        "id": null, "parentId": null, "createdBy": null, "modifiedBy": null, "schema": null,
        "type": "card", "title": null, "fields": null, "createAt": null, "updateAt": null,
        "deleteAt": null, "boardId": null,
    }))
    .expect("a block with null members decodes");
    assert_eq!(
        block,
        Block {
            block_type: BlockType::Card,
            ..Block::default()
        },
        "every null member is the empty value"
    );

    let board: Board = serde_json::from_value(json!({
        "id": null, "teamId": null, "channelId": null, "createdBy": null, "modifiedBy": null,
        "type": "P", "minimumRole": null, "title": null, "description": null, "icon": null,
        "showDescription": null, "isTemplate": null, "templateVersion": null, "properties": null,
        "cardProperties": null, "createAt": null, "updateAt": null, "deleteAt": null,
    }))
    .expect("a board with null members decodes");
    assert_eq!(
        board,
        Board {
            board_type: BoardType::Private,
            ..Board::default()
        },
        "every null member is the empty value"
    );

    let member: BoardMember = serde_json::from_value(json!({
        "boardId": null, "userId": null, "roles": null, "minimumRole": null,
        "schemeAdmin": null, "schemeEditor": null, "schemeCommenter": null, "schemeViewer": null,
    }))
    .expect("a member with null members decodes");
    assert_eq!(
        member,
        BoardMember::default(),
        "every null member is the empty value"
    );

    let login: LoginRequest = serde_json::from_value(json!({
        "type": null, "username": null, "email": null, "password": null, "mfa_token": null,
    }))
    .expect("a login request with null members decodes");
    assert_eq!(login.username, "", "null is the empty string");

    let category: Category = serde_json::from_value(json!({
        "id": null, "name": null, "userID": null, "teamID": null, "createAt": null,
        "updateAt": null, "deleteAt": null, "collapsed": null, "sortOrder": null, "sorting": null,
    }))
    .expect("a category with null members decodes");
    assert_eq!(
        category,
        Category::default(),
        "every null member is the empty value"
    );
}
