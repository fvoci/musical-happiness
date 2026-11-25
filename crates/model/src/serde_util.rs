//! Serde helpers shared by the wire types.

use serde::{Deserialize, Deserializer};

/// `skip_serializing_if` predicate for integer members left out of the JSON while zero.
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if passes the field by reference"
)]
pub(crate) fn is_zero(value: &i64) -> bool {
    *value == 0
}

/// Deserializes a present field as `Some(T)`, refusing explicit JSON `null`.
pub(crate) fn non_null<'de, T, D>(deserializer: D) -> Result<Option<T>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    T::deserialize(deserializer).map(Some)
}
