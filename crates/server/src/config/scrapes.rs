//! The settings of the metrics listener: where it listens (`prometheusaddress`), and the
//! bearer token a scrape must send (`prometheustoken`).

use sha2::{Digest as _, Sha256};
use subtle::ConstantTimeEq as _;

use serde_json::{Map, Value};

use super::Configuration;

/// The key of the token, as the file and the environment have it.
const TOKEN_KEY: &str = "prometheustoken";

/// Whether the environment's `text` for `key` leaves the key as the file has it: an
/// empty token. A variable set to nothing (a template whose value is missing) would
/// otherwise turn the token of the file off, and the listener would ask for none.
pub(super) fn is_unset_token(key: &str, text: &str) -> bool {
    key == TOKEN_KEY && text.is_empty()
}

/// Checks that the token, if the file has one, is text. The error of a value of
/// another type would repeat it (a number, a flag).
pub(super) fn check_token_is_text(keys: &Map<String, Value>) -> anyhow::Result<()> {
    match keys.get(TOKEN_KEY) {
        None | Some(Value::String(_) | Value::Null) => Ok(()),
        Some(_) => anyhow::bail!("{TOKEN_KEY} must be a string, in quotes"),
    }
}

/// Where the metrics listener listens: `prometheusaddress` read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetricsAddress {
    /// The host name or address; `None` for every interface.
    pub host: Option<String>,
    /// The port; zero for any free one.
    pub port: u16,
}

impl MetricsAddress {
    /// Reads `host:port`, `[ipv6]:port` or `:port`; `None` if the text is none of them.
    fn parse(text: &str) -> Option<Self> {
        let (host, port) = text.rsplit_once(':')?;
        if port.is_empty() || !port.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        let port: u16 = port.parse().ok()?;
        let host = if host.is_empty() {
            None
        } else if let Some(inner) = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
            inner.parse::<std::net::Ipv6Addr>().ok()?;
            Some(inner.to_owned())
        } else if host
            .chars()
            .any(|c| matches!(c, ':' | '[' | ']' | '/' | '@') || c.is_whitespace())
        {
            return None;
        } else {
            Some(host.to_owned())
        };
        Some(Self { host, port })
    }
}

/// The bearer token a scrape of the metrics must send: `prometheustoken` as its SHA-256
/// digest, compared in constant time. Neither the token nor its digest is written by
/// `Debug`.
#[derive(Clone)]
pub struct MetricsToken {
    digest: [u8; 32],
}

impl MetricsToken {
    fn new(token: &str) -> Self {
        Self {
            digest: Sha256::digest(token.as_bytes()).into(),
        }
    }

    /// Whether `presented` is the token. Both are compared as their digests, in time
    /// that depends on neither, so that the time of an answer tells nothing of how
    /// much of a guess was right, nor of the token's length. The comparison is to stay
    /// `ct_eq`: no test can tell `==` from it.
    pub fn admits(&self, presented: &str) -> bool {
        let presented: [u8; 32] = Sha256::digest(presented.as_bytes()).into();
        presented.ct_eq(&self.digest).into()
    }
}

impl std::fmt::Debug for MetricsToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MetricsToken(..)")
    }
}

/// Whether `token` can be sent as a bearer token (RFC 6750, `b64token`): letters,
/// digits and `-._~+/`, with `=` only at its end.
fn is_bearer_token(token: &str) -> bool {
    let body = token.trim_end_matches('=');
    !body.is_empty()
        && body
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-._~+/".contains(&byte))
}

impl Configuration {
    /// Where the metrics are served: `prometheusaddress` read as `host:port`, or `None`
    /// if it is empty and no metrics are kept. Without a host it is every interface, or
    /// `localhost` if `localOnly` is set.
    ///
    /// # Errors
    ///
    /// Returns an error if the address is not empty and no `host:port`: the host a name,
    /// an IPv4 address, an IPv6 address in brackets, or nothing; the port a number up to
    /// 65535.
    pub fn metrics_address(&self) -> anyhow::Result<Option<MetricsAddress>> {
        let text = self.prometheus_address.as_str();
        if text.is_empty() {
            return Ok(None);
        }
        let mut address = MetricsAddress::parse(text).ok_or_else(|| {
            anyhow::anyhow!(
                "prometheusaddress must be empty or host:port, such as 127.0.0.1:9092 or \
                 :9092 for every interface (the loopback with localOnly), not {text:?}"
            )
        })?;
        // A server that listens on the loopback only does so for its metrics too: the
        // listener has no authentication unless prometheustoken is set.
        if self.local_only && address.host.is_none() {
            address.host = Some("localhost".to_owned());
        }
        Ok(Some(address))
    }

    /// The token a scrape of the metrics must send; `None` if `prometheustoken` is
    /// empty and a scrape needs none.
    pub fn metrics_token(&self) -> Option<MetricsToken> {
        (!self.prometheus_token.is_empty()).then(|| MetricsToken::new(&self.prometheus_token))
    }

    /// Checks that `prometheustoken` can be sent as a bearer token. The error does not
    /// repeat it.
    pub(super) fn check_metrics_token(&self) -> anyhow::Result<()> {
        if self.prometheus_token.is_empty() || is_bearer_token(&self.prometheus_token) {
            Ok(())
        } else {
            anyhow::bail!(
                "prometheustoken must be empty or a bearer token: letters, digits and \
                 -._~+/, with = only at its end"
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "s3cr3t-T0ken_of.the~scrapes+/=";

    #[test]
    fn only_the_token_itself_is_admitted() {
        let token = MetricsToken::new(TOKEN);
        assert!(token.admits(TOKEN));
        for guess in [
            "",
            " ",
            "s3cr3t",
            &TOKEN[..TOKEN.len() - 1],
            &format!("{TOKEN}x"),
            &format!(" {TOKEN}"),
            &TOKEN.to_uppercase(),
        ] {
            assert!(!token.admits(guess), "{guess:?}");
        }
    }

    #[test]
    fn the_token_is_in_no_debug_output() {
        let config = Configuration {
            prometheus_token: TOKEN.to_owned(),
            ..Configuration::default()
        };
        let token = config.metrics_token().expect("a token is set");
        for shown in [
            format!("{config:?}"),
            format!("{token:?}"),
            format!("{token:#?}"),
        ] {
            assert!(!shown.contains(TOKEN), "{shown}");
            assert!(!shown.contains("s3cr3t"), "{shown}");
        }
        assert_eq!(format!("{token:?}"), "MetricsToken(..)");
        assert!(
            Configuration::default().metrics_token().is_none(),
            "empty: none"
        );
    }

    #[test]
    fn a_token_that_is_no_bearer_token_is_refused_without_repeating_it() {
        for refused in ["has space", "tab\there", "ünïcode", "a=b", "==", "quote\"d"] {
            let config = Configuration {
                prometheus_token: refused.to_owned(),
                ..Configuration::default()
            };
            let error = format!("{:#}", config.check().expect_err("not a bearer token"));
            assert!(error.contains("prometheustoken"), "{error}");
            assert!(!error.contains(refused), "{error}");
        }
        for accepted in ["", TOKEN, "abc", "abc==", "A-Z_0.9~+/"] {
            let config = Configuration {
                prometheus_token: accepted.to_owned(),
                ..Configuration::default()
            };
            config.check().expect("a bearer token");
        }
    }

    #[test]
    fn the_token_is_read_from_the_environment_too() {
        let env = |name: &str| (name == "FVOCI_PROMETHEUSTOKEN").then(|| TOKEN.to_owned());
        let config =
            Configuration::parse(r#"{"prometheustoken": "from-the-file"}"#, env).expect("valid");
        assert_eq!(config.prometheus_token, TOKEN, "the environment wins");
    }
}
