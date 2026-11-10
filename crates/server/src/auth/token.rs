//! Finding the session token in a request.

use axum::http::HeaderMap;
use axum::http::header::{AUTHORIZATION, COOKIE};

/// The name of the cookie that may carry the session token.
pub const SESSION_COOKIE: &str = "FVOCIAUTHTOKEN";

/// Where in a request the session token was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenLocation {
    /// The `Authorization` header.
    Header,
    /// The session cookie.
    Cookie,
    /// The `access_token` query parameter.
    QueryString,
}

/// Returns the session token of a request and where it was found.
///
/// The session cookie wins over the `Authorization` header (`Bearer <token>` or
/// `token <token>`, the scheme in any letter case), which wins over the `access_token`
/// query parameter. `query` is the raw query string of the request, if any.
pub fn parse_auth_token(
    headers: &HeaderMap,
    query: Option<&str>,
) -> Option<(String, TokenLocation)> {
    if let Some(token) = cookie_value(headers, SESSION_COOKIE) {
        return Some((token, TokenLocation::Cookie));
    }

    let authorization = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    // The original checks the scheme and then skips one more character without looking at
    // it, so "Bearer:abc" and "tokenXabc" are accepted like "Bearer abc" and "token abc".
    for scheme in ["bearer", "token"] {
        let has_scheme = authorization
            .get(..scheme.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(scheme));
        if has_scheme && authorization.len() > scheme.len() {
            let token = authorization.get(scheme.len() + 1..).unwrap_or_default();
            return Some((token.to_owned(), TokenLocation::Header));
        }
    }

    query
        .and_then(|query| query_value(query, "access_token"))
        .filter(|token| !token.is_empty())
        .map(|token| (token, TokenLocation::QueryString))
}

/// Returns the value of the named cookie, if the request carries it.
fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value.trim_matches('"').to_owned())
}

/// Returns the percent-decoded value of the first query parameter with the given name.
pub(crate) fn query_value(query: &str, name: &str) -> Option<String> {
    query
        .split('&')
        .map(|pair| pair.split_once('=').unwrap_or((pair, "")))
        .find(|(key, _)| percent_decode(key) == name)
        .map(|(_, value)| percent_decode(value))
}

/// Decodes `%XX` escapes and `+` as space; malformed escapes are kept as they are.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while let Some(&byte) = bytes.get(index) {
        let escaped = (byte == b'%')
            .then(|| bytes.get(index + 1..index + 3))
            .flatten()
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match (byte, escaped) {
            (_, Some(value)) => {
                decoded.push(value);
                index += 3;
            }
            (b'+', None) => {
                decoded.push(b' ');
                index += 1;
            }
            (other, None) => {
                decoded.push(other);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    /// Port of `TestParseAuthTokenFromRequest` in
    /// `server/services/auth/request_parser_test.go`, one row per case of its table.
    #[test]
    fn parse_auth_token_from_request() {
        let cases = [
            ("", "", "", None),
            (
                "token mytoken",
                "",
                "",
                Some(("mytoken", TokenLocation::Header)),
            ),
            (
                "BEARER mytoken",
                "",
                "",
                Some(("mytoken", TokenLocation::Header)),
            ),
            ("", "mytoken", "", Some(("mytoken", TokenLocation::Cookie))),
            (
                "",
                "",
                "mytoken",
                Some(("mytoken", TokenLocation::QueryString)),
            ),
        ];
        for (number, (header, cookie, query, want)) in cases.into_iter().enumerate() {
            let mut headers = HeaderMap::new();
            if !header.is_empty() {
                headers.insert(AUTHORIZATION, HeaderValue::from_static(header));
            }
            if !cookie.is_empty() {
                let value = format!("{SESSION_COOKIE}={cookie}");
                headers.insert(
                    COOKIE,
                    HeaderValue::from_str(&value).expect("a valid header"),
                );
            }
            let query = (!query.is_empty()).then(|| format!("access_token={query}"));

            let got = parse_auth_token(&headers, query.as_deref());

            let want = want.map(|(token, location)| (token.to_owned(), location));
            assert_eq!(got, want, "case {number}");
        }
    }

    #[test]
    fn cookie_wins_over_header_and_header_over_query() {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_static("Bearer from-header"),
        );
        assert_eq!(
            parse_auth_token(&headers, Some("access_token=from-query")),
            Some(("from-header".to_owned(), TokenLocation::Header))
        );

        let cookie = format!("other=1; {SESSION_COOKIE}=from-cookie; more=2");
        headers.insert(
            COOKIE,
            HeaderValue::from_str(&cookie).expect("a valid header"),
        );
        assert_eq!(
            parse_auth_token(&headers, Some("access_token=from-query")),
            Some(("from-cookie".to_owned(), TokenLocation::Cookie))
        );
    }

    #[test]
    fn header_schemes_ignore_case_and_need_a_token_part() {
        let token_of = |header: &'static str| {
            let mut headers = HeaderMap::new();
            headers.insert(AUTHORIZATION, HeaderValue::from_static(header));
            parse_auth_token(&headers, None).map(|(token, _)| token)
        };
        assert_eq!(token_of("bearer abc"), Some("abc".to_owned()));
        assert_eq!(token_of("TOKEN abc"), Some("abc".to_owned()));
        // Preserved quirk: a scheme followed by only a separator yields an empty token.
        assert_eq!(token_of("Bearer "), Some(String::new()));
        assert_eq!(token_of("Bearer"), None);
        assert_eq!(token_of("Basic abc"), None);
    }

    #[test]
    fn query_values_are_percent_decoded() {
        assert_eq!(
            query_value("a=1&access_token=x%20y%2Bz", "access_token"),
            Some("x y+z".to_owned())
        );
        assert_eq!(
            query_value("read_token=a+b", "read_token"),
            Some("a b".to_owned())
        );
        assert_eq!(query_value("flag&x=1", "flag"), Some(String::new()));
        assert_eq!(query_value("x=1", "y"), None);
        assert_eq!(query_value("x=%zz", "x"), Some("%zz".to_owned()));
    }
}
