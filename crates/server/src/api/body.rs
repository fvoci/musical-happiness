//! Decoding request bodies.
//!
//! A body that is not the JSON a route expects is a bad request (400), whatever is wrong
//! with it: not JSON at all, the wrong type, `null`, a member of the wrong type.

use fvoci_model::{BlockPatch, Body, BodyChange, BodyError, JsonMap};
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

/// The JSON objects in the list that is the member `name` of `object`. An element of the
/// list that is not an object is `None`.
pub(crate) fn objects_mut<'v>(
    object: &'v mut JsonMap,
    name: &str,
) -> impl Iterator<Item = Option<&'v mut JsonMap>> {
    object
        .get_mut(name)
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
        .map(Value::as_object_mut)
}

/// The refusal of a body that may not be stored, naming `body`.
fn body_refused(error: BodyError) -> AppError {
    AppError::bad_request(error).with_field("body")
}

/// The JSON Pointer to the block at `index` of a request: empty if the request body is
/// the one block (`list` is `None`), else the element of the list at the pointer
/// `list`, which is empty if the body is the list itself.
pub(crate) fn element_pointer(list: Option<&str>, index: usize) -> String {
    list.map(|list| format!("{list}/{index}"))
        .unwrap_or_default()
}

/// Takes the member `body` out of each block of a request before the blocks are decoded
/// and reads it, so that a body that may not be stored is refused as one (`errors`
/// names `/body`). Returns the bodies in the order of the blocks; an element that is no object
/// has none.
///
/// # Errors
///
/// Returns a bad request that names `body` for the first body that may not be stored,
/// where its block is in the request ([`element_pointer`]).
pub(crate) fn take_block_bodies<'a>(
    blocks: impl IntoIterator<Item = Option<&'a mut JsonMap>>,
    list: Option<&str>,
) -> Result<Vec<Option<Body>>, AppError> {
    blocks
        .into_iter()
        .enumerate()
        .map(
            |(index, block)| match block.and_then(|block| block.remove("body")) {
                None => Ok(None),
                Some(value) => Body::parse(value)
                    .map_err(|error| body_refused(error).under(&element_pointer(list, index))),
            },
        )
        .collect()
}

/// Takes the member `body` out of each block patch of a request before the patches are
/// decoded and reads what it asks for, in the order of the patches.
///
/// # Errors
///
/// Returns a bad request that names `body` for the first body that may not be stored,
/// where its patch is in the request ([`element_pointer`]).
pub(crate) fn take_patch_bodies<'a>(
    patches: impl IntoIterator<Item = Option<&'a mut JsonMap>>,
    list: Option<&str>,
) -> Result<Vec<BodyChange>, AppError> {
    patches
        .into_iter()
        .enumerate()
        .map(
            |(index, patch)| match patch.and_then(|patch| patch.remove("body")) {
                None => Ok(BodyChange::Keep),
                Some(value) => BodyChange::parse(value)
                    .map_err(|error| body_refused(error).under(&element_pointer(list, index))),
            },
        )
        .collect()
}

/// Gives each patch the change of body that was taken out of it, and checks that a
/// patch that changes the body says when its block was read.
///
/// # Errors
///
/// Returns a bad request that names `readAt` for a change of body without it, where
/// its patch is in the request ([`element_pointer`]).
pub(crate) fn put_patch_bodies<'a>(
    patches: impl IntoIterator<Item = &'a mut BlockPatch>,
    changes: Vec<BodyChange>,
    list: Option<&str>,
) -> Result<(), AppError> {
    let mut changes = changes.into_iter();
    for (index, patch) in patches.into_iter().enumerate() {
        // One change was taken out of each patch.
        let Some(change) = changes.next() else {
            return Err(AppError::from(anyhow::anyhow!(
                "fewer changes of body than block patches"
            )));
        };
        if !change.is_keep() && patch.read_at.is_none() {
            return Err(AppError::bad_request(
                "a change of the body needs readAt, the updateAt of the block as it was read",
            )
            .with_field("readAt")
            .under(&element_pointer(list, index)));
        }
        patch.body = change;
    }
    Ok(())
}

/// Decodes a request body that must be a JSON object and may carry a document under
/// `body`. The document is checked apart from the rest, so that its refusal names it
/// (`errors` names `/body`). Returns what the member asks for: [`BodyChange::Keep`] if it was
/// left out.
///
/// # Errors
///
/// Returns a bad request if the body is not a JSON object with the expected members, or
/// its document may not be stored.
pub(crate) fn decode_with_document<T: DeserializeOwned>(
    body: &[u8],
) -> Result<(T, BodyChange), AppError> {
    let (decoded, document) = decode_object_with(body, |members| members.remove("body"))?;
    let change = match document {
        None => BodyChange::Keep,
        Some(value) => BodyChange::parse(value)
            .map_err(|error| AppError::bad_request(error).with_field("body"))?,
    };
    Ok((decoded, change))
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
