//! The settings of the metrics listener: where it listens (`metrics_address`), and the
//! bearer token a scrape must send (`metrics_token`).

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use sha2::{Digest as _, Sha256};
use subtle::ConstantTimeEq as _;

use serde_json::{Map, Value};

use super::Configuration;

/// The key of the token, as the file and the environment have it.
const TOKEN_KEY: &str = "metrics_token";

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

/// Where the metrics listener listens: `metrics_address` read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetricsAddress {
    /// The host name, IPv4 address or IPv6 address (without its brackets).
    pub host: String,
    /// The port; zero for any free one.
    pub port: u16,
}

/// The longest host name DNS can carry (RFC 1035, 2.3.4), and the longest label of one.
const MAX_HOST_NAME_BYTES: usize = 253;
const MAX_LABEL_BYTES: usize = 63;

impl MetricsAddress {
    /// Reads `host:port`: the host a name, an IPv4 address or an IPv6 address in
    /// brackets, the port a number up to 65535 with no leading zero. `None` for anything
    /// else, a missing host among them.
    fn parse(text: &str) -> Option<Self> {
        let (host, port) = text.rsplit_once(':')?;
        let is_number = (1..=5).contains(&port.len())
            && port.bytes().all(|byte| byte.is_ascii_digit())
            && (port == "0" || !port.starts_with('0'));
        if !is_number {
            return None;
        }
        let port: u16 = port.parse().ok()?;
        let host = if let Some(inner) = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
            inner.parse::<Ipv6Addr>().ok()?;
            inner
        } else if host.parse::<Ipv4Addr>().is_ok() || is_host_name(host) {
            host
        } else {
            return None;
        };
        Some(Self {
            host: host.to_owned(),
            port,
        })
    }

    /// Whether the host is the address of every interface (`0.0.0.0`, `[::]`).
    fn is_every_interface(&self) -> bool {
        self.host
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_unspecified())
    }
}

/// Whether `host` is a DNS host name: labels of letters, digits and `-` (none at a
/// label's ends) joined by dots, the last label not all digits (RFC 1123, 2.1; that
/// would be an IPv4 address, and one that is no address is no name either).
fn is_host_name(host: &str) -> bool {
    let is_label = |label: &str| {
        (1..=MAX_LABEL_BYTES).contains(&label.len())
            && label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            && !label.starts_with('-')
            && !label.ends_with('-')
    };
    host.len() <= MAX_HOST_NAME_BYTES
        && host.split('.').all(is_label)
        && !host
            .rsplit('.')
            .next()
            .is_some_and(|last| last.bytes().all(|byte| byte.is_ascii_digit()))
}

/// The bearer token a scrape of the metrics must send: `metrics_token` as its SHA-256
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
    /// Where the metrics are served: `metrics_address` read as `host:port`, or `None`
    /// if it is empty and no metrics are kept.
    ///
    /// # Errors
    ///
    /// Returns an error if the address is not empty and no `host:port`: the host a name,
    /// an IPv4 address or an IPv6 address in brackets, the port a number up to 65535.
    /// Also if it is every interface (`0.0.0.0`, `[::]`) while `listen_loopback_only`
    /// is set: the listener has no authentication unless `metrics_token` is set.
    pub fn metrics_listen_address(&self) -> anyhow::Result<Option<MetricsAddress>> {
        let text = self.metrics_address.as_str();
        if text.is_empty() {
            return Ok(None);
        }
        let address = MetricsAddress::parse(text).ok_or_else(|| {
            anyhow::anyhow!(
                "metrics_address must be empty or host:port, such as 127.0.0.1:9092, \
                 [::1]:9092 or 0.0.0.0:9092 for every interface, not {text:?}"
            )
        })?;
        if self.listen_loopback_only && address.is_every_interface() {
            anyhow::bail!(
                "metrics_address {text:?} is every interface, and listen_loopback_only is \
                 set: give a loopback address, such as 127.0.0.1:9092 or [::1]:9092"
            );
        }
        Ok(Some(address))
    }

    /// The token a scrape of the metrics must send; `None` if `metrics_token` is empty
    /// and a scrape needs none.
    pub fn metrics_scrape_token(&self) -> Option<MetricsToken> {
        (!self.metrics_token.is_empty()).then(|| MetricsToken::new(&self.metrics_token))
    }

    /// Checks that `metrics_token`, if it is set, has at least [`MIN_TOKEN_CHARS`]
    /// characters and can be sent as a bearer token. The error does not repeat it.
    pub(super) fn check_metrics_token(&self) -> anyhow::Result<()> {
        let token = self.metrics_token.as_str();
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
            metrics_token: TOKEN.to_owned(),
            ..Configuration::default()
        };
        let token = config.metrics_scrape_token().expect("a token is set");
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
            Configuration::default().metrics_scrape_token().is_none(),
            "empty: none"
        );
    }

    #[test]
    fn a_token_that_is_no_bearer_token_is_refused_without_repeating_it() {
        let long = |text: &str| format!("{}{text}", "x".repeat(MIN_TOKEN_CHARS));
        for refused in ["has space", "tab\there", "ünïcode", "a=b", "quote\"d"].map(long) {
            let config = Configuration {
                metrics_token: refused.clone(),
                ..Configuration::default()
            };
            let error = format!("{:#}", config.check().expect_err("not a bearer token"));
            assert!(error.contains("metrics_token"), "{error}");
            assert!(!error.contains(&refused), "{error}");
        }
        let only_equals = Configuration {
            metrics_token: "=".repeat(MIN_TOKEN_CHARS),
            ..Configuration::default()
        };
        let error = format!(
            "{:#}",
            only_equals
                .check()
                .expect_err("= only at the end of something")
        );
        assert!(error.contains("metrics_token"), "{error}");
        assert!(
            !error.contains("=".repeat(MIN_TOKEN_CHARS).as_str()),
            "{error}"
        );
        for accepted in [
            String::new(),
            TOKEN.to_owned(),
            long("abc=="),
            long("A-Z_0.9~+/"),
        ] {
            let config = Configuration {
                metrics_token: accepted,
                ..Configuration::default()
            };
            config.check().expect("a bearer token");
        }
    }

    #[test]
    fn a_token_has_at_least_thirty_two_characters_from_the_file_or_the_environment() {
        let short = "a".repeat(MIN_TOKEN_CHARS - 1);
        let enough = "a".repeat(MIN_TOKEN_CHARS);
        let file = |token: &str| format!(r#"{{"metrics_token": "{token}"}}"#);
        let env_with = |token: String| {
            move |name: &str| (name == "FVOCI_METRICS_TOKEN").then(|| token.clone())
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
            assert_eq!(error, "metrics_token must be at least 32 characters");
            assert!(!error.contains(&short), "{error}");
        }
        let accepted = [
            Configuration::parse(&file(&enough), |_| None),
            Configuration::parse("{}", env_with(enough.clone())),
        ];
        for config in accepted {
            let config = config.expect("read");
            config.check().expect("32 characters are enough");
            assert!(config.metrics_scrape_token().is_some());
        }
        let empty = Configuration::parse(&file(""), |_| None).expect("read");
        empty.check().expect("no token");
        assert!(
            empty.metrics_scrape_token().is_none(),
            "no token, no authentication"
        );
    }

    #[test]
    fn a_token_given_twice_is_refused_without_its_values() {
        let text = format!(r#"{{"metrics_token": "{TOKEN}", "metrics_token": ""}}"#);
        let error = format!(
            "{:#}",
            Configuration::parse(&text, |_| None).expect_err("given twice")
        );
        assert!(
            error.starts_with("metrics_token is given more than once"),
            "{error}"
        );
        assert!(!error.contains(TOKEN), "{error}");
        let error = format!(
            "{:#}",
            Configuration::parse(
                r#"{"file_storage_s3": {"bucket": "secret-a", "bucket": "secret-b"}}"#,
                |_| None
            )
            .expect_err("given twice in file_storage_s3")
        );
        assert!(
            error.starts_with("file_storage_s3.bucket is given more than once"),
            "{error}"
        );
        assert!(!error.contains("secret-"), "{error}");
    }

    #[test]
    fn an_empty_variable_leaves_the_token_of_the_file() {
        let env = |name: &str| (name == "FVOCI_METRICS_TOKEN").then(String::new);
        let config =
            Configuration::parse(r#"{"metrics_token": "from-the-file"}"#, env).expect("valid");
        assert_eq!(config.metrics_token, "from-the-file");
        assert!(
            config.metrics_scrape_token().is_some(),
            "a scrape still needs it"
        );
    }

    #[test]
    fn a_token_that_is_no_text_is_refused_without_repeating_it() {
        for value in ["98765432109876", "1.5e3", "true"] {
            let error = format!(
                "{:#}",
                Configuration::parse(&format!(r#"{{"metrics_token": {value}}}"#), |_| None)
                    .expect_err("no string")
            );
            assert!(error.contains("metrics_token"), "{error}");
            assert!(!error.contains(value), "{error}");
        }
    }

    #[test]
    fn a_token_without_an_address_is_a_warning() {
        let config = Configuration {
            metrics_token: TOKEN.to_owned(),
            ..Configuration::default()
        };
        let warnings = config.check().expect("valid");
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("metrics_token")),
            "{warnings:?}"
        );
        assert!(warnings.iter().all(|warning| !warning.contains(TOKEN)));
    }

    #[test]
    fn the_token_is_read_from_the_environment_too() {
        let env = |name: &str| (name == "FVOCI_METRICS_TOKEN").then(|| TOKEN.to_owned());
        let config =
            Configuration::parse(r#"{"metrics_token": "from-the-file"}"#, env).expect("valid");
        assert_eq!(config.metrics_token, TOKEN, "the environment wins");
    }
}
