//! Decoding request bodies.
//!
//! A body that is not the JSON a route expects is a bad request (400), whatever is wrong
//! with it: not JSON at all, the wrong type, `null`, a member of the wrong type.

use fvoci_model::JsonMap;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::error::AppError;

/// Decodes a JSON request body.
///
/// # Errors
///
/// Returns a bad request if the body is not the expected JSON.
pub(crate) fn decode_json<T: DeserializeOwned>(body: &[u8]) -> Result<T, AppError> {
    serde_json::from_slice(body).map_err(AppError::bad_request)
}

/// Returns the JSON object that a request body must be.
///
/// # Errors
///
/// Returns a bad request for anything else, `null` included.
pub(crate) fn json_object(body: Value) -> Result<JsonMap, AppError> {
    match body {
        Value::Object(object) => Ok(object),
        _ => Err(AppError::bad_request(
            "the request body is not a JSON object",
        )),
    }
}

/// Decodes a request body that must be a JSON object.
///
/// Decoding the body straight into a record type would also accept a JSON array, read
/// as the record's members in order.
///
/// # Errors
///
/// Returns a bad request if the body is not a JSON object with the expected members.
pub(crate) fn decode_object<T: DeserializeOwned>(body: &[u8]) -> Result<T, AppError> {
    let (decoded, ()) = decode_object_with(body, |_| ())?;
    Ok(decoded)
}

/// Decodes a request body that must be a JSON object, after `prepare` had a look at its
/// members and a chance to change them.
///
/// # Errors
///
/// Returns a bad request if the body is not a JSON object with the expected members.
pub(crate) fn decode_object_with<T: DeserializeOwned, R>(
    body: &[u8],
    prepare: impl FnOnce(&mut JsonMap) -> R,
) -> Result<(T, R), AppError> {
    let mut object = json_object(decode_json(body)?)?;
    let prepared = prepare(&mut object);
    let decoded = serde_json::from_value(Value::Object(object)).map_err(AppError::bad_request)?;
    Ok((decoded, prepared))
}

/// Reads the members of a request body that must be a JSON object, leaving out those
/// that are `null`: a member that is `null` counts as not sent.
///
/// # Errors
///
/// Returns a bad request if the body is not a JSON object.
pub(crate) fn decode_object_members(body: &[u8]) -> Result<Map<String, Value>, AppError> {
    let mut members = json_object(decode_json(body)?)?;
    members.retain(|_, member| !member.is_null());
    Ok(members)
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;
    use crate::error::ErrorKind;

    #[derive(Debug, Deserialize, PartialEq)]
    struct Record {
        #[serde(default)]
        title: String,
    }

    #[test]
    fn whatever_is_wrong_with_a_body_is_a_bad_request() {
        for body in [
            "",
            "{",
            "nope",
            "null",
            "[]",
            r#"["x"]"#,
            "7",
            r#""text""#,
            r#"{"title": 7}"#,
        ] {
            let error = decode_object::<Record>(body.as_bytes()).expect_err("no record");
            assert_eq!(error.kind(), ErrorKind::BadRequest, "body {body:?}");
        }
        assert_eq!(
            decode_object::<Record>(br#"{"title": "x", "more": 1}"#).expect("a record"),
            Record {
                title: "x".to_owned()
            },
            "an object with the members is decoded"
        );
    }

    #[test]
    fn null_members_count_as_not_sent() {
        let members = decode_object_members(br#"{"a": null, "b": 1}"#).expect("an object");
        assert_eq!(members.len(), 1, "{members:?}");
        assert_eq!(members["b"], 1);
        let error = decode_object_members(b"null").expect_err("null is no object");
        assert_eq!(error.kind(), ErrorKind::BadRequest);
    }

    #[test]
    fn the_members_can_be_looked_at_before_decoding() {
        let (record, had_title): (Record, bool) =
            decode_object_with(br#"{"title": "x"}"#, |members| {
                members.contains_key("title")
            })
            .expect("a record");
        assert!(had_title, "what the preparation returns comes back");
        assert_eq!(record.title, "x");
    }
}
