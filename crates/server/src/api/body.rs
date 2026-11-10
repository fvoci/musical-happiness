//! Decoding request bodies.
//!
//! The original decodes each body with its JSON library and differs from handler to
//! handler in what a body that does not decode is answered with. The helpers here name
//! those policies. They also reproduce two things the original's decoder does and a
//! plain decode into a record type would not: a JSON array is never read as a record,
//! and `null` stands for a record with no members set.

use fvoci_model::JsonMap;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::error::AppError;

/// Decodes a JSON request body. The caller decides which status a failure gets, because
/// the original differs from handler to handler.
pub(crate) fn decode_json<T: DeserializeOwned>(body: &[u8]) -> Result<T, serde_json::Error> {
    serde_json::from_slice(body)
}

/// Decodes a JSON request body; a failure is reported as a server error.
///
/// Preserved quirk: many handlers of the original pass the decoding error on unclassified,
/// which makes a malformed body a 500 rather than a 400.
pub(crate) fn decode_json_or_internal<T: DeserializeOwned>(body: &[u8]) -> Result<T, AppError> {
    decode_json(body).map_err(AppError::internal)
}

/// Decodes a JSON request body; a failure is reported as a bad request.
///
/// # Errors
///
/// Returns [`AppError::BadRequest`] if the body is not the expected JSON.
pub(crate) fn decode_json_or_bad_request<T: DeserializeOwned>(body: &[u8]) -> Result<T, AppError> {
    decode_json(body).map_err(AppError::bad_request)
}

/// Returns the JSON object that a request body must be.
///
/// Anything else is a bad request, as when the original fails to decode it, except
/// `null`, which is a server error: the original decodes into a pointer, leaves it nil
/// and crashes on its first use, which its recovery turns into a 500.
pub(crate) fn json_object(body: Value) -> Result<JsonMap, AppError> {
    match body {
        Value::Object(object) => Ok(object),
        Value::Null => Err(AppError::internal("the request body is null")),
        _ => Err(AppError::bad_request(
            "the request body is not a JSON object",
        )),
    }
}

/// Decodes a JSON object from a request body. A body that is not the expected JSON is a
/// bad request, and `null` a server error (see [`json_object`]).
pub(crate) fn decode_object_or_bad_request<T: DeserializeOwned>(
    body: &[u8],
) -> Result<T, AppError> {
    let object = json_object(decode_json_or_bad_request(body)?)?;
    serde_json::from_value(Value::Object(object)).map_err(AppError::bad_request)
}

/// Decodes a JSON object from a request body, after `prepare` had a look at it.
///
/// Preserved quirk: a body that is not the expected JSON is a server error on the routes
/// that use this (`server/api/boards_and_blocks.go:86-89`, `:265-268`).
pub(crate) fn decode_object_or_internal<T: DeserializeOwned, R>(
    body: &[u8],
    prepare: impl FnOnce(&mut JsonMap) -> R,
) -> Result<(T, R), AppError> {
    let mut object = json_object(decode_json_or_internal(body)?).map_err(AppError::internal)?;
    let prepared = prepare(&mut object);
    let decoded = serde_json::from_value(Value::Object(object)).map_err(AppError::internal)?;
    Ok((decoded, prepared))
}

/// Reads a request body that describes one record, the way the original's decoder fills
/// a record: the body must be a JSON object or `null`, and a member that is `null`
/// counts as left out. The members are returned for decoding into the record.
///
/// Decoding the body straight into a record type would differ: a JSON array would be read
/// as the record's members in order, and a `null` member is only accepted where the
/// record type says so.
///
/// # Errors
///
/// Returns a server error if the body is anything else, because the handlers that use
/// this do not classify the original's decoding error.
pub(crate) fn decode_object_members(body: &[u8]) -> Result<Map<String, Value>, AppError> {
    let members: Option<Map<String, Value>> = decode_json_or_internal(body)?;
    let mut members = members.unwrap_or_default();
    members.retain(|_, member| !member.is_null());
    Ok(members)
}
