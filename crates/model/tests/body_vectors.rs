//! The shared test vectors of `contracts/body-vectors.json`: every case is read as a
//! request's body would be, and must be accepted, taken for no body, or refused for the
//! reason the case names.

use std::path::Path;

use fvoci_model::{BODY_VERSION, Body, BodyError};
use serde_json::Value;

/// The kind of a refusal, as the vectors name it.
fn reason(error: &BodyError) -> &'static str {
    match error {
        BodyError::NotADocument => "notDocument",
        BodyError::TooLarge => "tooLarge",
        BodyError::TooDeep => "tooDeep",
        BodyError::TooManyNodes => "tooManyNodes",
        BodyError::InvalidNode { .. } => "invalidNode",
    }
}

fn vectors() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../contracts/body-vectors.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

#[test]
fn the_shared_vectors_hold() {
    let vectors = vectors();
    assert_eq!(
        vectors["bodyVersion"],
        Value::from(BODY_VERSION),
        "the vectors are written for the allowlist this server checks"
    );
    let cases = vectors["cases"].as_array().expect("the cases are a list");
    assert!(!cases.is_empty(), "there are cases");
    for case in cases {
        let name = case["name"].as_str().expect("a case has a name");
        let got = Body::parse(case["body"].clone());
        match case["expect"].as_str() {
            Some("accept") => {
                let body = got
                    .unwrap_or_else(|error| panic!("{name}: refused ({error})"))
                    .unwrap_or_else(|| panic!("{name}: taken for no body"));
                assert_eq!(body.document(), &case["body"], "{name}: stored as sent");
            }
            Some("empty") => {
                assert!(matches!(got, Ok(None)), "{name}: no body, not {got:?}");
            }
            Some("reject") => {
                let error = got.expect_err(name);
                assert_eq!(
                    Some(reason(&error)),
                    case["reason"].as_str(),
                    "{name}: {error}"
                );
            }
            other => panic!("{name}: unknown expectation {other:?}"),
        }
    }
}
