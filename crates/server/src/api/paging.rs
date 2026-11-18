//! The numbers of a query: `page` and `perPage` of every list answered in pages, and
//! any other whole number a route reads from its query.

use std::num::IntErrorKind;
use std::ops::RangeInclusive;

use super::query_param;
use crate::app::{DEFAULT_PER_PAGE, ListPage, MAX_PER_PAGE};
use crate::error::AppError;

/// The largest `page`: the contract's `int32`. A page times [`MAX_PER_PAGE`], the first
/// item of a page, stays within the `i64` of an SQL offset.
const MAX_PAGE: u32 = i32::MAX.unsigned_abs();

/// Reads the `page` and `perPage` of any list answered in pages: `page` a whole number
/// from 0 to [`MAX_PAGE`], 0 when left out; `perPage` one from 1 to [`MAX_PER_PAGE`],
/// [`DEFAULT_PER_PAGE`] when left out. Both are read, so that a request with both wrong
/// is told of both.
pub(super) fn paging_params(query: Option<&str>) -> Result<ListPage, AppError> {
    let page = count_query_param(query, "page", 0, 0..=MAX_PAGE);
    let per_page = count_query_param(query, "perPage", DEFAULT_PER_PAGE, 1..=MAX_PER_PAGE);
    match (page, per_page) {
        (Ok(page), Ok(per_page)) => Ok(ListPage { page, per_page }),
        (Err(refusal), Ok(_)) | (Ok(_), Err(refusal)) => Err(refusal),
        (Err(page), Err(per_page)) => Err(page.and(per_page)),
    }
}

/// Reads a count from a query parameter: a whole number in `range`; `default` if the
/// parameter is absent or empty.
fn count_query_param(
    query: Option<&str>,
    name: &str,
    default: u32,
    range: RangeInclusive<u32>,
) -> Result<u32, AppError> {
    let number = int_query_param(query, name, i64::from(default))?;
    u32::try_from(number)
        .ok()
        .filter(|count| range.contains(count))
        .ok_or_else(|| {
            let reason = if number < i64::from(*range.start()) {
                format!("below {}", range.start())
            } else {
                format!("above {}", range.end())
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
