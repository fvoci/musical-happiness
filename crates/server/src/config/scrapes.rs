//! The settings of the metrics listener: where it listens (`prometheusaddress`), and the
//! bearer token a scrape must send (`prometheustoken`).

use sha2::{Digest as _, Sha256};
use subtle::ConstantTimeEq as _;

use serde_json::{Map, Value};

use super::Configuration;

/// The key of the token, as the file and the environment have it.
const TOKEN_KEY: &str = "prometheustoken";

/// The fewest characters a token may have: a guess at it must not be feasible, and
/// the listener does not limit how often a scrape is refused.
const MIN_TOKEN_CHARS: usize = 32;

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

    /// Checks that `prometheustoken`, if it is set, has at least [`MIN_TOKEN_CHARS`]
    /// characters and can be sent as a bearer token. The error does not repeat it.
    pub(super) fn check_metrics_token(&self) -> anyhow::Result<()> {
        let token = self.prometheus_token.as_str();
        if token.is_empty() {
            return Ok(());
        }
        if token.chars().count() < MIN_TOKEN_CHARS {
            anyhow::bail!("{TOKEN_KEY} must be at least {MIN_TOKEN_CHARS} characters");
        }
        if !is_bearer_token(token) {
            anyhow::bail!(
                "{TOKEN_KEY} must be a bearer token: letters, digits and -._~+/, with = \
                 only at its end"
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A token of 34 characters.
    const TOKEN: &str = "s3cr3t-T0ken_of.the~scrapes+/abc==";

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
        let long = |text: &str| format!("{}{text}", "x".repeat(MIN_TOKEN_CHARS));
        for refused in ["has space", "tab\there", "ünïcode", "a=b", "quote\"d"].map(long) {
            let config = Configuration {
                prometheus_token: refused.clone(),
                ..Configuration::default()
            };
            let error = format!("{:#}", config.check().expect_err("not a bearer token"));
            assert!(error.contains("prometheustoken"), "{error}");
            assert!(!error.contains(&refused), "{error}");
        }
        for accepted in [
            String::new(),
            TOKEN.to_owned(),
            long("abc=="),
            long("A-Z_0.9~+/"),
        ] {
            let config = Configuration {
                prometheus_token: accepted,
                ..Configuration::default()
            };
            config.check().expect("a bearer token");
        }
    }

    #[test]
    fn a_token_has_at_least_thirty_two_characters_from_the_file_or_the_environment() {
        let short = "a".repeat(MIN_TOKEN_CHARS - 1);
        let enough = "a".repeat(MIN_TOKEN_CHARS);
        let file = |token: &str| format!(r#"{{"prometheustoken": "{token}"}}"#);
        let env_with = |token: String| {
            move |name: &str| (name == "FVOCI_PROMETHEUSTOKEN").then(|| token.clone())
        };
        let refused = [
            Configuration::parse(&file(&short), |_| None),
            Configuration::parse("{}", env_with(short.clone())),
        ];
        for config in refused {
            let error = format!(
                "{:#}",
                config
                    .expect("read")
                    .check()
                    .expect_err("31 characters are too few")
            );
            assert_eq!(error, "prometheustoken must be at least 32 characters");
            assert!(!error.contains(&short), "{error}");
        }
        let accepted = [
            Configuration::parse(&file(&enough), |_| None),
            Configuration::parse("{}", env_with(enough.clone())),
        ];
        for config in accepted {
            let config = config.expect("read");
            config.check().expect("32 characters are enough");
            assert!(config.metrics_token().is_some());
        }
        let empty = Configuration::parse(&file(""), |_| None).expect("read");
        empty.check().expect("no token");
        assert!(
            empty.metrics_token().is_none(),
            "no token, no authentication"
        );
    }

    #[test]
    fn an_empty_variable_leaves_the_token_of_the_file() {
        let env = |name: &str| (name == "FVOCI_PROMETHEUSTOKEN").then(String::new);
        let config =
            Configuration::parse(r#"{"prometheustoken": "from-the-file"}"#, env).expect("valid");
        assert_eq!(config.prometheus_token, "from-the-file");
        assert!(config.metrics_token().is_some(), "a scrape still needs it");
    }

    #[test]
    fn a_token_that_is_no_text_is_refused_without_repeating_it() {
        for value in ["98765432109876", "1.5e3", "true"] {
            let error = format!(
                "{:#}",
                Configuration::parse(&format!(r#"{{"prometheustoken": {value}}}"#), |_| None)
                    .expect_err("no string")
            );
            assert!(error.contains("prometheustoken"), "{error}");
            assert!(!error.contains(value), "{error}");
        }
    }

    #[test]
    fn a_token_without_an_address_is_a_warning() {
        let config = Configuration {
            prometheus_token: TOKEN.to_owned(),
            ..Configuration::default()
        };
        let warnings = config.check().expect("valid");
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("prometheustoken")),
            "{warnings:?}"
        );
        assert!(warnings.iter().all(|warning| !warning.contains(TOKEN)));
    }

    #[test]
    fn the_token_is_read_from_the_environment_too() {
        let env = |name: &str| (name == "FVOCI_PROMETHEUSTOKEN").then(|| TOKEN.to_owned());
        let config =
            Configuration::parse(r#"{"prometheustoken": "from-the-file"}"#, env).expect("valid");
        assert_eq!(config.prometheus_token, TOKEN, "the environment wins");
    }
}
