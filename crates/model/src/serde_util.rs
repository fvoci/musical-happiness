//! Serde helpers shared by the wire types.

use serde::{Deserialize, Deserializer};

/// Deserializes a JSON `null` as the type's default value.
///
/// The original's decoder leaves a member at its zero value when the JSON says `null`,
/// and its own client sends `null` for collections that were never set. Every member of
/// a type that is decoded from a request accepts it.
pub(crate) fn null_as_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    let value: Option<T> = Option::deserialize(deserializer)?;
    Ok(value.unwrap_or_default())
}

/// `skip_serializing_if` predicate for integer fields the original marks `omitempty`.
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if passes the field by reference"
)]
pub(crate) fn is_zero(value: &i64) -> bool {
    *value == 0
}
