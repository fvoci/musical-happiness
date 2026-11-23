//! How a request says who sends it, and from where: the session token in a header or in
//! the session cookie, and the origin of the page that sent it.
//!
//! A browser gets the token in a cookie that scripts cannot read and sends it back by
//! itself. That is why a request that comes with the cookie must show that the server's
//! own pages sent it (see [`Origins`] and the request-forgery header in `crate::api`).
//! A client that is no browser reads the token from the `Set-Cookie` header of the login
//! and sends it in the `Authorization` header, which no browser adds by itself.

use axum::http::header::{AUTHORIZATION, COOKIE, ORIGIN};
use axum::http::{HeaderMap, HeaderValue, Uri};

/// The session token of a request, by how the request carries it.
#[derive(Clone, PartialEq, Eq)]
pub enum Credential {
    /// In the `Authorization` header, as `Bearer <token>`: sent on purpose.
    Bearer(String),
    /// In the session cookie: sent by a browser, whether or not the page meant to.
    Cookie(String),
}

impl Credential {
    /// The token.
    pub fn token(&self) -> &str {
        match self {
            Self::Bearer(token) | Self::Cookie(token) => token,
        }
    }
}

impl std::fmt::Debug for Credential {
    /// Leaves out the token.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Bearer(_) => "Credential::Bearer(..)",
            Self::Cookie(_) => "Credential::Cookie(..)",
        })
    }
}

/// Returns the token of an `Authorization: Bearer <token>` header, the scheme in any
/// letter case. A header of another scheme is not a session token.
pub fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let authorization = headers.get(AUTHORIZATION)?.to_str().ok()?;
    // The scheme alone is a bearer token too, an empty one: whoever sends the header
    // means to present a token, and is not to be taken for a browser with a cookie.
    let (scheme, token) = authorization.split_once(' ').unwrap_or((authorization, ""));
    scheme
        .eq_ignore_ascii_case("bearer")
        .then_some(token.trim())
}

/// The session cookie: its name and attributes, which depend on whether the server is
/// reached over HTTPS, and how long a browser keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionCookie {
    secure: bool,
    max_age_seconds: i64,
}

impl SessionCookie {
    /// The name of the cookie where the server is reached over HTTPS. The `__Host-`
    /// prefix makes browsers accept it only as a `Secure` cookie of exactly this host
    /// with `Path=/`, so that no other host of the domain can plant one.
    pub const SECURE_NAME: &'static str = "__Host-fvoci_session";
    /// The name of the cookie where the server is reached over plain HTTP, as in
    /// development: browsers refuse the prefixed name there.
    pub const PLAIN_NAME: &'static str = "fvoci_session";

    /// A session cookie that a browser keeps for `max_age_seconds`, `Secure` and with
    /// the prefixed name if the server is reached over HTTPS.
    pub fn new(https: Https, max_age_seconds: i64) -> Self {
        Self {
            secure: https == Https::Yes,
            max_age_seconds,
        }
    }

    /// The name of the cookie.
    pub fn name(&self) -> &'static str {
        if self.secure {
            Self::SECURE_NAME
        } else {
            Self::PLAIN_NAME
        }
    }

    /// Returns the session token a request carries, and how. A token in the
    /// `Authorization` header counts before one in the cookie: it was sent on purpose.
    pub fn credential(&self, headers: &HeaderMap) -> Option<Credential> {
        if let Some(token) = bearer_token(headers) {
            return Some(Credential::Bearer(token.to_owned()));
        }
        cookie_value(headers, self.name())
            .filter(|token| !token.is_empty())
            .map(Credential::Cookie)
    }

    /// The `Set-Cookie` header that gives a browser the token: not readable by scripts,
    /// sent along with requests from the same site only, for the whole host.
    pub fn set(&self, token: &str) -> HeaderValue {
        self.header(token, self.max_age_seconds)
    }

    /// The `Set-Cookie` header that makes a browser forget the token.
    pub fn clear(&self) -> HeaderValue {
        self.header("", 0)
    }

    fn header(&self, token: &str, max_age_seconds: i64) -> HeaderValue {
        let secure = if self.secure { "; Secure" } else { "" };
        let cookie = format!(
            "{}={token}; Max-Age={max_age_seconds}; Path=/; HttpOnly; SameSite=Lax{secure}",
            self.name()
        );
        // A token is base64url text and the rest is fixed: always a valid header value.
        HeaderValue::from_str(&cookie).unwrap_or_else(|_| HeaderValue::from_static(""))
    }
}

/// Whether the server is reached over HTTPS, which its `server_url` says: behind a
/// proxy that ends TLS the server itself listens on plain HTTP.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Https {
    /// `server_url` begins with `https://`.
    Yes,
    /// `server_url` begins with `http://`.
    No,
}

/// The origins whose pages may send requests that change something, and may open
/// websocket connections: the server's own, from `server_url`, and for development the
/// ones listed in `allowed_origins`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origins {
    allowed: Vec<String>,
    https: Https,
}

impl Origins {
    /// Reads the origin of `server_url` and of each of `allowed_origins`.
    ///
    /// # Errors
    ///
    /// Returns the first of them that is no `http` or `https` URL with a host.
    pub fn new(server_url: &str, allowed_origins: &[String]) -> Result<Self, String> {
        let own = origin_of(server_url)
            .ok_or_else(|| format!("server_url {server_url:?} is no http or https URL"))?;
        let https = if own.starts_with("https://") {
            Https::Yes
        } else {
            Https::No
        };
        let mut allowed = vec![own];
        for origin in allowed_origins {
            allowed.push(origin_of(origin).ok_or_else(|| {
                format!("allowed_origins has {origin:?}, which is no http or https origin")
            })?);
        }
        Ok(Self { allowed, https })
    }

    /// Whether the server's own origin is an HTTPS one.
    pub fn https(&self) -> Https {
        self.https
    }

    /// Whether a page of the origin a request names in its `Origin` header may send it.
    /// A request without the header names none and is not judged here.
    pub fn allows(&self, origin: &HeaderValue) -> bool {
        origin
            .to_str()
            .ok()
            .and_then(origin_of)
            .is_some_and(|origin| self.allowed.contains(&origin))
    }

    /// Returns the `Origin` header of a request if it names an origin that is not
    /// allowed.
    pub fn foreign_origin<'h>(&self, headers: &'h HeaderMap) -> Option<&'h HeaderValue> {
        headers.get(ORIGIN).filter(|origin| !self.allows(origin))
    }
}

/// The origin of a URL in one spelling: scheme and host in lower case, the port unless
/// it is the scheme's own. `None` for what is no `http` or `https` URL with a host; the
/// `null` that browsers send for pages without an origin is none.
pub(crate) fn origin_of(url: &str) -> Option<String> {
    let uri: Uri = url.trim().parse().ok()?;
    let scheme = uri.scheme_str()?.to_ascii_lowercase();
    let default_port = match scheme.as_str() {
        "http" => 80,
        "https" => 443,
        _ => return None,
    };
    let host = uri
        .host()
        .filter(|host| !host.is_empty())?
        .to_ascii_lowercase();
    Some(match uri.port_u16() {
        Some(port) if port != default_port => format!("{scheme}://{host}:{port}"),
        _ => format!("{scheme}://{host}"),
    })
}

/// Returns the value of the named cookie, if the request carries it.
pub(crate) fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
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
    use super::*;

    fn headers(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.append(*name, HeaderValue::from_static(value));
        }
        headers
    }

    #[test]
    fn the_token_is_read_from_the_bearer_header_or_the_cookie_of_this_mode() {
        let plain = SessionCookie::new(Https::No, 60);
        let secure = SessionCookie::new(Https::Yes, 60);
        let bearer = |token: &str| Some(Credential::Bearer(token.to_owned()));
        let cookie = |token: &str| Some(Credential::Cookie(token.to_owned()));

        assert_eq!(plain.credential(&headers(&[])), None, "nothing");
        for header in ["Bearer mytoken", "BEARER mytoken", "bearer  mytoken "] {
            assert_eq!(
                plain.credential(&headers(&[("authorization", header)])),
                bearer("mytoken"),
                "{header}"
            );
        }
        // A header of the scheme without a token is a credential, and no good one.
        for header in ["Bearer ", "Bearer", "bearer"] {
            assert_eq!(
                plain.credential(&headers(&[("authorization", header)])),
                bearer(""),
                "{header:?}"
            );
        }
        // Other schemes are no session tokens; the original's `token` scheme is none.
        for header in ["token mytoken", "Basic dTpw", "Bearer:mytoken", "mytoken"] {
            assert_eq!(
                plain.credential(&headers(&[("authorization", header)])),
                None,
                "{header}"
            );
        }

        assert_eq!(
            plain.credential(&headers(&[("cookie", "a=b; fvoci_session=mytoken; c=d")])),
            cookie("mytoken")
        );
        assert_eq!(
            secure.credential(&headers(&[("cookie", "__Host-fvoci_session=mytoken")])),
            cookie("mytoken")
        );
        // Each mode reads its own name only: a cookie under the plain name could be
        // planted over HTTP, and the prefixed one is not set without HTTPS.
        assert_eq!(
            secure.credential(&headers(&[("cookie", "fvoci_session=mytoken")])),
            None
        );
        assert_eq!(
            plain.credential(&headers(&[("cookie", "__Host-fvoci_session=mytoken")])),
            None
        );
        assert_eq!(
            plain.credential(&headers(&[("cookie", "fvoci_session=")])),
            None,
            "an empty cookie is none"
        );

        // Both: the header, which was sent on purpose.
        assert_eq!(
            plain.credential(&headers(&[
                ("cookie", "fvoci_session=of-the-cookie"),
                ("authorization", "Bearer of-the-header"),
            ])),
            bearer("of-the-header")
        );
    }

    #[test]
    fn the_cookie_is_http_only_same_site_and_secure_over_https() {
        assert_eq!(
            SessionCookie::new(Https::No, 1_209_600).set("tok"),
            "fvoci_session=tok; Max-Age=1209600; Path=/; HttpOnly; SameSite=Lax"
        );
        assert_eq!(
            SessionCookie::new(Https::Yes, 1_209_600).set("tok"),
            "__Host-fvoci_session=tok; Max-Age=1209600; Path=/; HttpOnly; SameSite=Lax; Secure"
        );
        assert_eq!(
            SessionCookie::new(Https::No, 1_209_600).clear(),
            "fvoci_session=; Max-Age=0; Path=/; HttpOnly; SameSite=Lax"
        );
        assert_eq!(
            SessionCookie::new(Https::Yes, 60).clear(),
            "__Host-fvoci_session=; Max-Age=0; Path=/; HttpOnly; SameSite=Lax; Secure"
        );
    }

    #[test]
    fn the_token_is_not_in_the_debug_form_of_a_credential() {
        let debug = format!("{:?}", Credential::Cookie("SECRET".to_owned()));
        assert!(!debug.contains("SECRET"), "{debug}");
    }

    #[test]
    fn origins_are_compared_by_scheme_host_and_port() {
        let origins = Origins::new(
            "https://Boards.Example.com/boards",
            &["http://localhost:5173".to_owned()],
        )
        .expect("valid origins");
        assert_eq!(origins.https(), Https::Yes);
        let allows = |origin: &'static str| origins.allows(&HeaderValue::from_static(origin));

        for origin in [
            "https://boards.example.com",
            "https://BOARDS.example.com",
            "https://boards.example.com:443",
            "http://localhost:5173",
        ] {
            assert!(allows(origin), "{origin}");
        }
        for origin in [
            "http://boards.example.com",
            "https://boards.example.com:8443",
            "https://evil.example.com",
            "https://boards.example.com.evil.example",
            "https://example.com",
            "http://localhost",
            "http://localhost:5174",
            "null",
            "",
            "boards.example.com",
            "file:///boards",
        ] {
            assert!(!allows(origin), "{origin}");
        }

        let plain = Origins::new("http://localhost:8000", &[]).expect("valid");
        assert_eq!(plain.https(), Https::No);
        assert!(plain.allows(&HeaderValue::from_static("http://localhost:8000")));
        assert!(!plain.allows(&HeaderValue::from_static("http://localhost:8001")));
        assert!(!plain.allows(&HeaderValue::from_static("http://127.0.0.1:8000")));
    }

    #[test]
    fn a_request_without_an_origin_header_names_no_foreign_origin() {
        let origins = Origins::new("http://localhost:8000", &[]).expect("valid");
        assert_eq!(origins.foreign_origin(&headers(&[])), None);
        assert_eq!(
            origins.foreign_origin(&headers(&[("origin", "http://localhost:8000")])),
            None
        );
        assert!(
            origins
                .foreign_origin(&headers(&[("origin", "https://evil.example")]))
                .is_some()
        );
        assert!(
            origins
                .foreign_origin(&headers(&[("origin", "null")]))
                .is_some(),
            "a page without an origin is a foreign one"
        );
    }

    #[test]
    fn what_is_no_http_origin_is_refused_at_start() {
        for server_url in [
            "",
            "localhost:8000",
            "ftp://example.com",
            "https://",
            "/boards",
        ] {
            assert!(Origins::new(server_url, &[]).is_err(), "{server_url:?}");
        }
        assert!(
            Origins::new("http://localhost:8000", &["*".to_owned()]).is_err(),
            "no wildcard among the allowed origins"
        );
    }

    #[test]
    fn query_values_are_percent_decoded() {
        assert_eq!(
            query_value("a=1&readToken=x%2By", "readToken").as_deref(),
            Some("x+y")
        );
        assert_eq!(query_value("a=1", "readToken"), None);
        assert_eq!(
            query_value("readToken=a+b", "readToken").as_deref(),
            Some("a b")
        );
    }
}
