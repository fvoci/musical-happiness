//! The numbers of a query: `page` and `perPage` of every list answered in pages, and
//! any other whole number a route reads from its query.

use std::num::IntErrorKind;

use super::query_param;
use crate::error::AppError;

/// The page of a list a request without `page` gets.
const DEFAULT_PAGE: u32 = 0;

/// The largest `page` or `perPage`: the contract's `int32`. Two of them multiplied, the
/// first item of a page, stay within the `i64` of an SQL offset.
const MAX_COUNT: u32 = i32::MAX.unsigned_abs();

/// Reads the `page` and `perPage` of any list answered in pages: each a whole number
/// from 0 to [`MAX_COUNT`], `page` 0 and `perPage` `default_per_page` when left out.
/// Both are read, so that a request with both wrong is told of both.
pub(super) fn paging_params(
    query: Option<&str>,
    default_per_page: u32,
) -> Result<(u32, u32), AppError> {
    let page = count_query_param(query, "page", DEFAULT_PAGE);
    let per_page = count_query_param(query, "perPage", default_per_page);
    match (page, per_page) {
        (Ok(page), Ok(per_page)) => Ok((page, per_page)),
        (Err(refusal), Ok(_)) | (Ok(_), Err(refusal)) => Err(refusal),
        (Err(page), Err(per_page)) => Err(page.and(per_page)),
    }
}

/// Reads a count from a query parameter: a whole number from 0 to [`MAX_COUNT`];
/// `default` if the parameter is absent or empty.
fn count_query_param(query: Option<&str>, name: &str, default: u32) -> Result<u32, AppError> {
    let number = int_query_param(query, name, i64::from(default))?;
    u32::try_from(number)
        .ok()
        .filter(|count| *count <= MAX_COUNT)
        .ok_or_else(|| {
            let reason = if number < 0 {
                "below 0"
            } else {
                "out of range"
            };
            AppError::bad_request(format_args!("invalid `{name}` parameter: {reason}"))
                .with_parameter(name)
        })
}

/// Reads a whole number from a query parameter; `default` if the parameter is absent
/// or empty.
pub(super) fn int_query_param(
    query: Option<&str>,
    name: &str,
    default: i64,
) -> Result<i64, AppError> {
    let text = query_param(query, name);
    if text.is_empty() {
        return Ok(default);
    }
    text.parse().map_err(|error: std::num::ParseIntError| {
        let reason = match error.kind() {
            IntErrorKind::PosOverflow | IntErrorKind::NegOverflow => "out of range",
            _ => "not a whole number",
        };
        AppError::bad_request(format_args!("invalid `{name}` parameter: {reason}"))
            .with_parameter(name)
    })
}
