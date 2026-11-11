//! Validation rules of blocks, boards and authentication requests. The original has no
//! unit tests for these; the cases follow `server/model/block.go`, `board.go`, `auth.go`
//! and `server/services/auth/email.go`.

use fvoci_model::{
    BLOCK_FIELDS_MAX_CHARS, BLOCK_TITLE_MAX_CHARS, Block, BlockError, BlockPatch, BlockType, Board,
    BoardError, BoardSearchField, CARD_FIELDS_MAX_CHARS, ChangePasswordRequest, RegisterRequest,
    is_email_valid,
};
use serde_json::json;

fn block() -> Block {
    Block {
        board_id: "board-id".to_owned(),
        ..Block::default()
    }
}

#[test]
fn block_validate_requires_a_board() {
    assert_eq!(Block::default().validate(), Err(BlockError::EmptyBoardId));
    assert_eq!(block().validate(), Ok(()));
    assert_eq!(BlockError::EmptyBoardId.to_string(), "boardID is empty");
}

#[test]
fn block_validate_limits_the_title_in_characters() {
    // The limit counts characters, not bytes: a three-byte character counts once.
    let at_limit = Block {
        title: "가".repeat(BLOCK_TITLE_MAX_CHARS),
        ..block()
    };
    assert_eq!(at_limit.validate(), Ok(()));

    let over_limit = Block {
        title: "a".repeat(BLOCK_TITLE_MAX_CHARS + 1),
        ..block()
    };
    assert_eq!(
        over_limit.validate(),
        Err(BlockError::TitleSizeLimitExceeded)
    );
    assert_eq!(BLOCK_TITLE_MAX_CHARS, 16383);
}

#[test]
fn block_validate_limits_the_serialized_fields_by_type() {
    // {"k":"<value>"} is the value plus eight characters of JSON syntax.
    let with_value_len = |block_type: &BlockType, len: usize| -> Block {
        let mut fields = fvoci_model::JsonMap::new();
        fields.insert("k".to_owned(), json!("a".repeat(len)));
        Block {
            block_type: block_type.clone(),
            fields,
            ..block()
        }
    };
    assert_eq!(CARD_FIELDS_MAX_CHARS, 64_000);
    assert_eq!(BLOCK_FIELDS_MAX_CHARS, 800_000);
    let cases = [
        (BlockType::Card, CARD_FIELDS_MAX_CHARS),
        (BlockType::Comment, CARD_FIELDS_MAX_CHARS),
        (BlockType::Attachment, CARD_FIELDS_MAX_CHARS),
        (BlockType::View, BLOCK_FIELDS_MAX_CHARS),
    ];
    for (block_type, limit) in cases {
        assert_eq!(block_type.fields_max_chars(), limit, "{block_type}");
        assert_eq!(
            with_value_len(&block_type, limit - 8).validate(),
            Ok(()),
            "{block_type} at the limit"
        );
        assert_eq!(
            with_value_len(&block_type, limit - 7).validate(),
            Err(BlockError::FieldsSizeLimitExceeded),
            "{block_type} over the limit"
        );
    }
}

/// A new block is a card, a view, a comment or an attachment (`docs/s6-design.md` 5.7).
#[test]
fn only_four_types_are_current() {
    for name in ["card", "view", "comment", "attachment"] {
        assert!(BlockType::from(name).is_current(), "{name}");
    }
    for name in [
        "board", "text", "image", "checkbox", "divider", "h1", "", "Card",
    ] {
        assert!(!BlockType::from(name).is_current(), "{name:?}");
    }
}

/// The refusal shows at most 64 characters of the type a client wrote.
#[test]
fn a_refused_type_is_shown_shortened() {
    assert_eq!(fvoci_model::TYPE_SHOWN_CHARS, 64);
    let long = "é".repeat(100);
    let BlockError::TypeNotCurrent(shown) = BlockError::type_not_current(&BlockType::from(long))
    else {
        panic!("a refusal of the type");
    };
    assert_eq!(shown, format!("{}…", "é".repeat(64)));
    for (length, shown) in [(64, "a".repeat(64)), (65, format!("{}…", "a".repeat(64)))] {
        assert_eq!(
            BlockError::type_not_current(&BlockType::from("a".repeat(length))),
            BlockError::TypeNotCurrent(shown),
            "{length} characters"
        );
    }
    assert_eq!(
        BlockError::type_not_current(&BlockType::from("text")),
        BlockError::TypeNotCurrent("text".to_owned())
    );
}

/// A patch may give a block a type that a new block may have, and no other.
#[test]
fn a_patch_does_not_give_a_block_a_type_that_is_not_current() {
    let to = |name: &str| BlockPatch {
        block_type: Some(BlockType::from(name)),
        ..BlockPatch::default()
    };
    assert_eq!(BlockPatch::default().check_type(), Ok(()), "no type");
    assert_eq!(to("card").check_type(), Ok(()), "to a card");
    assert_eq!(
        to("text").check_type(),
        Err(BlockError::TypeNotCurrent("text".to_owned()))
    );
    assert_eq!(
        to("h1").check_type(),
        Err(BlockError::TypeNotCurrent("h1".to_owned()))
    );
}

#[test]
fn board_validate_requires_a_team() {
    assert_eq!(Board::default().validate(), Err(BoardError::EmptyTeamId));
    assert_eq!(BoardError::EmptyTeamId.to_string(), "empty-team-id");
    let board = Board {
        team_id: "0".to_owned(),
        ..Board::default()
    };
    assert_eq!(board.validate(), Ok(()));
}

#[test]
fn board_search_field_parses_the_two_wire_names() {
    assert_eq!("title".parse(), Ok(BoardSearchField::Title));
    assert_eq!("property_name".parse(), Ok(BoardSearchField::PropertyName));
    for other in ["", "Title", "name"] {
        let error = other.parse::<BoardSearchField>().expect_err(other);
        assert_eq!(error.to_string(), "invalid board search field");
    }
}

fn register(username: &str, email: &str, password: &str) -> Result<(), String> {
    RegisterRequest {
        username: username.to_owned(),
        email: email.to_owned(),
        password: password.to_owned(),
        token: String::new(),
        display_name: String::new(),
    }
    .validate()
    .map_err(|error| error.to_string())
}

#[test]
fn register_request_validate() {
    assert_eq!(register("alice", "alice@example.com", "12345678"), Ok(()));

    let cases = [
        (
            ("", "alice@example.com", "12345678"),
            "username is required",
        ),
        (
            ("  ", "alice@example.com", "12345678"),
            "username is required",
        ),
        (("alice", "", "12345678"), "email is required"),
        (
            ("alice", "not-an-email", "12345678"),
            "invalid email format",
        ),
        (("alice", "alice@example.com", ""), "password is required"),
        (
            ("alice", "alice@example.com", "1234567"),
            "password must be at least 8 characters",
        ),
    ];
    for ((username, email, password), message) in cases {
        assert_eq!(
            register(username, email, password),
            Err(message.to_owned()),
            "{username:?} {email:?} {password:?}"
        );
    }
}

#[test]
fn the_minimum_password_length_is_counted_in_characters() {
    // Four two-byte characters are eight bytes, and four characters: too short.
    assert_eq!(
        register("alice", "alice@example.com", "éééé"),
        Err("password must be at least 8 characters".to_owned())
    );
    // Eight characters, of whatever width, meet the minimum.
    assert_eq!(
        register("alice", "alice@example.com", "비밀번호여덟글자"),
        Ok(())
    );
    assert_eq!(register("alice", "alice@example.com", "éééééééé"), Ok(()));
}

#[test]
fn the_maximum_password_length_is_counted_in_bytes() {
    let longest = "a".repeat(fvoci_model::MAXIMUM_PASSWORD_BYTES);
    assert_eq!(register("alice", "alice@example.com", &longest), Ok(()));
    let too_long = "a".repeat(fvoci_model::MAXIMUM_PASSWORD_BYTES + 1);
    assert_eq!(
        register("alice", "alice@example.com", &too_long),
        Err("password must be at most 1024 bytes".to_owned())
    );
    // 342 three-byte characters are 1026 bytes.
    let wide = "가".repeat(342);
    assert_eq!(
        register("alice", "alice@example.com", &wide),
        Err("password must be at most 1024 bytes".to_owned())
    );
    assert_eq!(
        fvoci_model::check_new_password(&too_long).map_err(|error| error.to_string()),
        Err("password must be at most 1024 bytes".to_owned()),
        "the same rule for every way a password is set"
    );
}

#[test]
fn change_password_request_validate() {
    let check = |old: &str, new: &str| -> Result<(), String> {
        ChangePasswordRequest {
            old_password: old.to_owned(),
            new_password: new.to_owned(),
        }
        .validate()
        .map_err(|error| error.to_string())
    };
    assert_eq!(check("old", "12345678"), Ok(()));
    assert_eq!(
        check("", "12345678"),
        Err("old password is required".to_owned())
    );
    assert_eq!(check("old", ""), Err("new password is required".to_owned()));
    assert_eq!(
        check("old", "short"),
        Err("password must be at least 8 characters".to_owned())
    );
}

#[test]
fn email_validation_follows_the_original_pattern() {
    let label63 = "a".repeat(63);
    let valid = [
        "a@b".to_owned(),
        "test@mail.com".to_owned(),
        "first.last+tag@sub.example.co".to_owned(),
        "!#$%&'*+/=?^_`{|}~-@example.com".to_owned(),
        "user@a-b.example".to_owned(),
        "user@1.2".to_owned(),
        format!("user@{label63}.com"),
    ];
    for email in &valid {
        assert!(is_email_valid(email), "{email} should be valid");
    }

    let label64 = "a".repeat(64);
    let invalid = [
        String::new(),
        "a@".to_owned(),
        "@b".to_owned(),
        "ab".to_owned(),
        "a b@example.com".to_owned(),
        "a@b@c".to_owned(),
        "user@-example.com".to_owned(),
        "user@example-.com".to_owned(),
        "user@example..com".to_owned(),
        "user@example.com.".to_owned(),
        "user@.example.com".to_owned(),
        "user@exa_mple.com".to_owned(),
        "üser@example.com".to_owned(),
        "user@example.com\n".to_owned(),
        format!("user@{label64}.com"),
        format!("{}@example.com", "a".repeat(243)),
    ];
    for email in &invalid {
        assert!(!is_email_valid(email), "{email:?} should be invalid");
    }

    // 254 bytes is the longest address accepted.
    let longest = format!("{}@{}.{}.{}.com", "a".repeat(58), label63, label63, label63);
    assert_eq!(longest.len(), 254);
    assert!(is_email_valid(&longest), "a 254-byte address is valid");
}
