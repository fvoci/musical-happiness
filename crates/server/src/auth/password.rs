//! Password hashing and password rules.

use thiserror::Error;

/// The cost of the bcrypt hashes this server creates.
pub const PASSWORD_HASH_COST: u32 = 10;

/// The longest accepted password, in bytes.
pub const PASSWORD_MAXIMUM_LENGTH: usize = 64;

/// The characters that count as symbols.
const SYMBOLS: &str = "!\"\\#$%&'()*+,-./:;<=>?@[]^_`|~";

/// Hashes a password with bcrypt at [`PASSWORD_HASH_COST`].
///
/// # Errors
///
/// Returns the bcrypt error if the password cannot be hashed.
pub fn hash_password(password: &str) -> Result<String, bcrypt::BcryptError> {
    hash_password_with_cost(password, PASSWORD_HASH_COST)
}

/// Hashes a password with bcrypt at the given cost. Tests use a low cost to stay fast.
///
/// # Errors
///
/// Returns the bcrypt error if the cost is out of range or the password cannot be hashed.
pub fn hash_password_with_cost(password: &str, cost: u32) -> Result<String, bcrypt::BcryptError> {
    bcrypt::hash(password, cost)
}

/// Returns `true` if `password` matches the bcrypt `hash`.
///
/// An empty password or hash never matches, and neither does a malformed hash.
pub fn compare_password(hash: &str, password: &str) -> bool {
    if password.is_empty() || hash.is_empty() {
        return false;
    }
    bcrypt::verify(password, hash).unwrap_or(false)
}

/// A kind of character a password can be required to contain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharacterClass {
    /// An ASCII lower-case letter.
    Lowercase,
    /// An ASCII upper-case letter.
    Uppercase,
    /// An ASCII digit.
    Number,
    /// One of ``!"\#$%&'()*+,-./:;<=>?@[]^_`|~``.
    Symbol,
}

/// The rules a new password must meet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PasswordSettings<'a> {
    /// The shortest accepted password, in bytes.
    pub minimum_length: usize,
    /// The kinds of character the password must contain.
    pub required: &'a [CharacterClass],
}

/// A password breaks one or more of the [`PasswordSettings`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("invalid password, failing criteria: {}", .failing_criteria.join(", "))]
pub struct InvalidPasswordError {
    /// The names of the rules that failed: `min-length`, `max-length`, `lowercase`,
    /// `uppercase`, `number`, `symbol`.
    pub failing_criteria: Vec<&'static str>,
}

/// Checks `password` against `settings` and [`PASSWORD_MAXIMUM_LENGTH`].
///
/// # Errors
///
/// Returns an [`InvalidPasswordError`] listing every rule the password breaks, lengths
/// first, then character classes in the order lowercase, uppercase, number, symbol.
pub fn check_password(
    password: &str,
    settings: PasswordSettings<'_>,
) -> Result<(), InvalidPasswordError> {
    let mut failing_criteria = Vec::new();
    if password.len() < settings.minimum_length {
        failing_criteria.push("min-length");
    }
    if password.len() > PASSWORD_MAXIMUM_LENGTH {
        failing_criteria.push("max-length");
    }
    let classes = [
        (CharacterClass::Lowercase, "lowercase"),
        (CharacterClass::Uppercase, "uppercase"),
        (CharacterClass::Number, "number"),
        (CharacterClass::Symbol, "symbol"),
    ];
    for (class, name) in classes {
        if settings.required.contains(&class) && !password.chars().any(|c| is_in_class(c, class)) {
            failing_criteria.push(name);
        }
    }

    if failing_criteria.is_empty() {
        Ok(())
    } else {
        Err(InvalidPasswordError { failing_criteria })
    }
}

fn is_in_class(character: char, class: CharacterClass) -> bool {
    match class {
        CharacterClass::Lowercase => character.is_ascii_lowercase(),
        CharacterClass::Uppercase => character.is_ascii_uppercase(),
        CharacterClass::Number => character.is_ascii_digit(),
        CharacterClass::Symbol => SYMBOLS.contains(character),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use CharacterClass::{Lowercase, Number, Symbol, Uppercase};

    /// `TestPasswordHash` in `server/services/auth/password_test.go`.
    #[test]
    fn password_hash() {
        let hash = hash_password_with_cost("Test", 4).expect("hashing succeeds");
        assert!(compare_password(&hash, "Test"), "passwords don't match");
        assert!(
            !compare_password(&hash, "Test2"),
            "passwords should not have matched"
        );
    }

    #[test]
    fn hashes_use_the_original_cost_and_verify_original_hashes() {
        let hash = hash_password("Test").expect("hashing succeeds");
        assert!(
            hash.starts_with("$2b$10$"),
            "unexpected hash prefix: {hash}"
        );

        // The hash the original server stored for a user registered with this password
        // (taken from its database during porting; note the `$2a$` prefix).
        let original = "$2a$10$DZVQxQn1EROpkOn480U6r.1kNvclgpYT9dEdvM7Xx3MdlV83Fn90G";
        assert!(
            compare_password(original, "test-password"),
            "an original hash verifies"
        );
        assert!(
            !compare_password(original, "test-passwore"),
            "a wrong password does not"
        );
    }

    #[test]
    fn empty_or_malformed_input_never_matches() {
        let hash = hash_password_with_cost("Test", 4).expect("hashing succeeds");
        assert!(
            !compare_password(&hash, ""),
            "an empty password never matches"
        );
        assert!(!compare_password("", "Test"), "an empty hash never matches");
        assert!(
            !compare_password("not-a-hash", "Test"),
            "a malformed hash never matches"
        );
    }

    /// `TestIsPasswordValidWithSettings` in `server/services/auth/password_test.go`, one
    /// row per case of its table.
    #[test]
    fn is_password_valid_with_settings() {
        let long = "x".repeat(PASSWORD_MAXIMUM_LENGTH);
        let too_long = "x".repeat(PASSWORD_MAXIMUM_LENGTH + 1);
        let none: &[CharacterClass] = &[];
        let cases: [(&str, &str, &[CharacterClass], &[&str]); 10] = [
            ("Short", "xxx", none, &[]),
            ("Long", &long, none, &[]),
            ("TooShort", "xx", none, &["min-length"]),
            ("TooLong", &too_long, none, &["max-length"]),
            (
                "MissingLower",
                "AAAAAAAAAAASD123!@#",
                &[Lowercase],
                &["lowercase"],
            ),
            (
                "MissingUpper",
                "aaaaaaaaaaaaasd123!@#",
                &[Uppercase],
                &["uppercase"],
            ),
            ("MissingNumber", "asasdasdsadASD!@#", &[Number], &["number"]),
            (
                "MissingSymbol",
                "asdasdasdasdasdASD123",
                &[Symbol],
                &["symbol"],
            ),
            (
                "MissingMultiple",
                "asdasdasdasdasdasd",
                &[Lowercase, Uppercase, Number, Symbol],
                &["uppercase", "number", "symbol"],
            ),
            (
                "Everything",
                "asdASD!@#123",
                &[Lowercase, Uppercase, Number, Symbol],
                &[],
            ),
        ];
        for (name, password, required, want) in cases {
            let settings = PasswordSettings {
                minimum_length: 3,
                required,
            };
            let got = match check_password(password, settings) {
                Ok(()) => Vec::new(),
                Err(error) => error.failing_criteria,
            };
            assert_eq!(got, want, "case {name}");
        }
    }

    #[test]
    fn error_message_lists_the_failing_criteria() {
        let settings = PasswordSettings {
            minimum_length: 6,
            required: &[],
        };
        let error = check_password("abc", settings).expect_err("too short");
        assert_eq!(
            error.to_string(),
            "invalid password, failing criteria: min-length"
        );
    }
}
