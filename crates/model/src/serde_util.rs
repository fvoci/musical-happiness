//! Serde helpers shared by the wire types.

/// `skip_serializing_if` predicate for integer members left out of the JSON while zero.
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if passes the field by reference"
)]
pub(crate) fn is_zero(value: &i64) -> bool {
    *value == 0
}
