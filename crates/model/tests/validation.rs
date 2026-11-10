//! Validation rules of blocks, boards and authentication requests. The original has no
//! unit tests for these; the cases follow `server/model/block.go`, `board.go`, `auth.go`
//! and `server/services/auth/email.go`.

use fvoci_model::{
    BLOCK_FIELDS_MAX_CHARS, BLOCK_TITLE_MAX_CHARS, Block, BlockError, Board, BoardError,
    BoardSearchField, ChangePasswordRequest, RegisterRequest, is_email_valid,
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
fn block_validate_limits_the_serialized_fields() {
    // {"k":"<value>"} is the value plus eight characters of JSON syntax.
    let with_value_len = |len: usize| -> Block {
        let mut fields = fvoci_model::JsonMap::new();
        fields.insert("k".to_owned(), json!("a".repeat(len)));
        Block { fields, ..block() }
    };
    assert_eq!(
        with_value_len(BLOCK_FIELDS_MAX_CHARS - 8).validate(),
        Ok(())
    );
    assert_eq!(
        with_value_len(BLOCK_FIELDS_MAX_CHARS - 7).validate(),
        Err(BlockError::FieldsSizeLimitExceeded)
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
fn password_length_is_counted_in_bytes() {
    // Four two-byte characters are eight bytes, which meets the minimum.
    assert_eq!(register("alice", "alice@example.com", "éééé"), Ok(()));
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
