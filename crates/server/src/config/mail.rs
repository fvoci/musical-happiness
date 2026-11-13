//! Which way mail leaves by (`mail_transport`), and the keys of the mail providers'
//! HTTP APIs. Secrets never appear in diagnostics, and a key of a way that is not the
//! chosen one is refused rather than passed over.

use std::fmt;
use std::net::IpAddr;

use anyhow::{anyhow, bail};
use serde::{Deserialize, Deserializer};
use serde_json::{Map, Value};

use super::smtp::SmtpSettings;

pub(super) const ENV_KEYS: [(&str, super::EnvKind); 4] = [
    ("mail_transport", super::EnvKind::Text),
    ("mail_api_endpoint", super::EnvKind::Text),
    ("cloudflare_account_id", super::EnvKind::Text),
    ("cloudflare_api_token", super::EnvKind::Text),
];

/// The keys whose values are secrets: a value of the wrong type is refused without
/// the error repeating it.
const SECRET_KEYS: [&str; 1] = ["cloudflare_api_token"];

/// The way mail leaves the server; a value that is none of these is refused when the
/// configuration is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(serde::Serialize))]
#[serde(rename_all = "lowercase")]
pub enum MailTransport {
    /// An SMTP relay (`smtp_*`), for servers installed by their users.
    Smtp,
    /// Cloudflare Email Service's REST API (`cloudflare_*`).
    Cloudflare,
}

impl MailTransport {
    /// The value of `mail_transport` that names it.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Smtp => "smtp",
            Self::Cloudflare => "cloudflare",
        }
    }
}

/// The keys of the way of sending, flattened into the main configuration file.
#[derive(Clone, Default, Deserialize)]
#[cfg_attr(test, derive(serde::Serialize))]
#[serde(default)]
pub struct MailSettings {
    /// `smtp` or `cloudflare`; empty for SMTP when `smtp_host` is set and
    /// no mail otherwise.
    #[serde(deserialize_with = "transport_or_none")]
    #[cfg_attr(test, serde(serialize_with = "transport_as_text"))]
    pub mail_transport: Option<MailTransport>,
    /// The scheme, host and port that the provider's requests go to instead of its own
    /// (a fake provider of the tests, another realm); empty for the provider's.
    pub mail_api_endpoint: String,
    /// The Cloudflare account whose Email Service sends.
    pub cloudflare_account_id: String,
    /// The API token; omitted from debug output.
    pub cloudflare_api_token: String,
}

impl fmt::Debug for MailSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MailSettings")
            .field("mail_transport", &self.mail_transport)
            .field("mail_api_endpoint", &self.mail_api_endpoint)
            .field("cloudflare_account_id", &self.cloudflare_account_id)
            .finish_non_exhaustive()
    }
}

fn transport_or_none<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<MailTransport>, D::Error> {
    let text = String::deserialize(deserializer)?;
    if text.is_empty() {
        return Ok(None);
    }
    MailTransport::deserialize(serde::de::value::StrDeserializer::<D::Error>::new(&text)).map(Some)
}

#[cfg(test)]
#[expect(
    clippy::ref_option,
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's serialize_with hands the field by reference"
)]
fn transport_as_text<S: serde::Serializer>(
    transport: &Option<MailTransport>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(transport.map_or("", MailTransport::as_str))
}

/// A secret of the configuration: shown as `<hidden>` by `Debug`.
#[derive(Clone)]
pub struct Secret(String);

impl Secret {
    /// The secret itself, for the request it signs or authorizes.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<hidden>")
    }
}

/// What Cloudflare Email Service needs.
#[derive(Debug, Clone)]
pub struct CloudflareKeys {
    /// Where the requests go: `https://api.cloudflare.com`, or `mail_api_endpoint`.
    pub endpoint: String,
    /// `cloudflare_account_id`.
    pub account_id: String,
    /// `cloudflare_api_token`.
    pub api_token: Secret,
}

/// The way mail leaves, with what it needs, as [`MailSettings::way`] reads it.
#[derive(Debug, Clone)]
pub enum Way {
    /// The SMTP relay of `smtp_*`.
    Smtp,
    /// Cloudflare Email Service.
    Cloudflare(CloudflareKeys),
}

/// The keys of each provider, by the transport that reads them.
const PROVIDER_KEYS: [(MailTransport, &[&str]); 1] = [(
    MailTransport::Cloudflare,
    &["cloudflare_account_id", "cloudflare_api_token"],
)];

impl MailSettings {
    /// Whether mail is sent at all: a transport is named, or `smtp_host` is set.
    pub fn enabled(&self, smtp: &SmtpSettings) -> bool {
        self.mail_transport.is_some() || smtp.enabled()
    }

    fn value(&self, key: &str) -> &str {
        match key {
            "mail_api_endpoint" => &self.mail_api_endpoint,
            "cloudflare_account_id" => &self.cloudflare_account_id,
            "cloudflare_api_token" => &self.cloudflare_api_token,
            _ => unreachable!("{key} is a key of the mail settings"),
        }
    }

    /// The way mail leaves, or `None` for no mail, with the sender (`smtp_from`) checked
    /// for every way. Contacts nothing and reads no file.
    ///
    /// # Errors
    /// Returns an error naming the key that is missing, malformed, or set for a way that
    /// is not the chosen one; never a secret's value.
    pub fn way(&self, smtp: &SmtpSettings) -> anyhow::Result<Option<Way>> {
        let transport = match self.mail_transport {
            Some(transport) => transport,
            None if smtp.enabled() => MailTransport::Smtp,
            None => {
                self.refuse_unused(None)?;
                if !smtp.smtp_from.is_empty() || !smtp.smtp_user.is_empty() {
                    bail!(
                        "smtp_host or mail_transport is required when mail settings are supplied"
                    );
                }
                return Ok(None);
            }
        };
        self.refuse_unused(Some(transport))?;
        if !fvoci_model::is_email_valid(&smtp.smtp_from) {
            bail!("smtp_from must be a single email address: the sender of all mail");
        }
        if transport == MailTransport::Smtp {
            if !smtp.enabled() {
                bail!("mail_transport is \"smtp\", which needs smtp_host");
            }
            return Ok(Some(Way::Smtp));
        }
        if smtp.enabled() || !smtp.smtp_user.is_empty() {
            bail!(
                "smtp_host and smtp_user are for mail_transport \"smtp\", not \"{}\"",
                transport.as_str()
            );
        }
        let needed = |key: &str| -> anyhow::Result<&str> {
            let value = self.value(key);
            if value.is_empty() {
                bail!(
                    "mail_transport is \"{}\", which needs {key}",
                    transport.as_str()
                );
            }
            Ok(value)
        };
        Ok(Some(match transport {
            MailTransport::Smtp => unreachable!("returned above"),
            MailTransport::Cloudflare => {
                let account_id = needed("cloudflare_account_id")?;
                let api_token = needed("cloudflare_api_token")?;
                shaped("cloudflare_account_id", account_id, |c| {
                    c.is_ascii_alphanumeric()
                })?;
                secret_shaped("cloudflare_api_token", api_token)?;
                Way::Cloudflare(CloudflareKeys {
                    endpoint: self.endpoint("https://api.cloudflare.com".to_owned())?,
                    account_id: account_id.to_owned(),
                    api_token: Secret(api_token.to_owned()),
                })
            }
        }))
    }

    /// Refuses a provider's key, or the endpoint, set for a way that does not read it.
    fn refuse_unused(&self, transport: Option<MailTransport>) -> anyhow::Result<()> {
        let chosen = transport.map_or("not set", MailTransport::as_str);
        for (provider, keys) in PROVIDER_KEYS {
            if Some(provider) == transport {
                continue;
            }
            if let Some(key) = keys.iter().find(|key| !self.value(key).is_empty()) {
                bail!("{key} is set, but mail_transport is {chosen:?}");
            }
        }
        if !self.mail_api_endpoint.is_empty()
            && matches!(transport, None | Some(MailTransport::Smtp))
        {
            bail!("mail_api_endpoint is set, but mail_transport is {chosen:?}");
        }
        Ok(())
    }

    /// `mail_api_endpoint` when it is set, checked, or the provider's own `default`.
    /// Plain `http` goes to this machine only: the requests carry the provider's keys.
    fn endpoint(&self, default: String) -> anyhow::Result<String> {
        if self.mail_api_endpoint.is_empty() {
            return Ok(default);
        }
        let refused = || {
            anyhow!(
                "mail_api_endpoint must be https://host[:port] (http only to this machine), \
                 without a path, a query or credentials"
            )
        };
        let url = reqwest::Url::parse(&self.mail_api_endpoint).map_err(|_| refused())?;
        let plain_to_here = url.scheme() == "http" && is_loopback_url(&url);
        if !(url.scheme() == "https" || plain_to_here)
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.host_str().is_none()
        {
            return Err(refused());
        }
        Ok(url.as_str().trim_end_matches('/').to_owned())
    }
}

/// Refuses a value with a character `allowed` does not take: these become parts of a
/// host name, a path or a header.
fn shaped(key: &str, value: &str, allowed: impl Fn(char) -> bool) -> anyhow::Result<()> {
    if value.chars().all(allowed) {
        Ok(())
    } else {
        bail!("{key} has a character it cannot have")
    }
}

/// Refuses a secret that is not printable ASCII without spaces, without saying which
/// character it was.
fn secret_shaped(key: &str, value: &str) -> anyhow::Result<()> {
    shaped(key, value, |c| c.is_ascii_graphic())
}

/// Whether `url` names this machine: `localhost`, or a loopback or the unspecified
/// address (also one mapped into IPv6).
pub fn is_loopback_url(url: &reqwest::Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    // A URL writes an IPv6 address in brackets, and an IPv4 one in its usual form.
    if let Ok(address) = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<IpAddr>()
    {
        let address = address.to_canonical();
        address.is_loopback() || address.is_unspecified()
    } else {
        // The parser writes the name in lower case; a trailing dot names the same host.
        let name = host.trim_end_matches('.');
        name == "localhost" || name.ends_with(".localhost")
    }
}

/// Checks that the secrets, if the file has them, are text: the error of a value of
/// another type would repeat it.
pub(super) fn check_secrets_are_text(keys: &Map<String, Value>) -> anyhow::Result<()> {
    for key in SECRET_KEYS {
        match keys.get(key) {
            None | Some(Value::String(_)) => {}
            Some(_) => bail!("{key} must be a string"),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::config::Configuration;

    const TOKEN: &str = "private-cloudflare-token-0123456789";

    /// The configuration of `keys`, checked: the error's text, or the way.
    fn checked(keys: &Value) -> Result<Option<Way>, String> {
        let config =
            Configuration::parse(&keys.to_string(), |_| None).map_err(|e| e.to_string())?;
        config.check().map_err(|e| e.to_string())?;
        config.mail.way(&config.smtp).map_err(|e| e.to_string())
    }

    fn cloudflare() -> Value {
        json!({
            "mail_transport": "cloudflare",
            "cloudflare_account_id": "0123456789abcdef0123456789abcdef",
            "cloudflare_api_token": TOKEN,
            "smtp_from": "sender@example.test",
        })
    }

    #[test]
    fn the_transport_is_smtp_with_a_host_and_nothing_without() {
        assert!(matches!(checked(&json!({})), Ok(None)));
        let smtp = json!({"smtp_host": "smtp.example.test", "smtp_from": "sender@example.test"});
        assert!(matches!(checked(&smtp), Ok(Some(Way::Smtp))));
        let mut named = smtp;
        named["mail_transport"] = Value::from("smtp");
        assert!(matches!(checked(&named), Ok(Some(Way::Smtp))));
        let empty = json!({"mail_transport": ""});
        assert!(matches!(checked(&empty), Ok(None)), "empty is unset");
        let error = checked(&json!({"mail_transport": "smtp", "smtp_from": "sender@example.test"}))
            .expect_err("no relay");
        assert!(error.contains("smtp_host"), "{error}");
        let error = checked(&json!({"mail_transport": "postmark"})).expect_err("unknown");
        assert!(
            error.contains("wrong type") || error.contains("unknown variant"),
            "{error}"
        );
        let mut env = BTreeMapEnv::default();
        env.0.push(("FVOCI_MAIL_TRANSPORT", "cloudflare"));
        let config = Configuration::parse("{}", |name| env.get(name)).expect("parse");
        assert_eq!(config.mail.mail_transport, Some(MailTransport::Cloudflare));
    }

    #[test]
    fn a_provider_without_a_key_it_needs_is_refused_by_the_keys_name() {
        assert!(matches!(
            checked(&cloudflare()),
            Ok(Some(Way::Cloudflare(_)))
        ));
        for key in ["cloudflare_account_id", "cloudflare_api_token", "smtp_from"] {
            let mut keys = cloudflare();
            keys.as_object_mut().expect("an object").remove(key);
            let error = checked(&keys).expect_err("a missing key");
            assert!(error.contains(key), "{key}: {error}");
            assert!(!error.contains(TOKEN), "{error}");
        }
    }

    #[test]
    fn keys_the_chosen_way_does_not_read_are_refused() {
        for (key, value) in [("smtp_host", "smtp.example.test"), ("smtp_user", "user")] {
            let mut keys = cloudflare();
            keys[key] = Value::from(value);
            if key == "smtp_user" {
                keys["smtp_password"] = Value::from("password");
            }
            let error = checked(&keys).expect_err("a key of SMTP");
            assert!(error.contains(key), "{key}: {error}");
        }
        let smtp = json!({
            "smtp_host": "smtp.example.test",
            "smtp_from": "sender@example.test",
            "cloudflare_api_token": TOKEN,
        });
        let error = checked(&smtp).expect_err("a key of Cloudflare");
        assert!(error.contains("cloudflare_api_token"), "{error}");
        assert!(!error.contains(TOKEN), "{error}");
        let off = json!({"cloudflare_account_id": "abc"});
        let error = checked(&off).expect_err("a key without mail");
        assert!(error.contains("cloudflare_account_id"), "{error}");
        let mut endpoint =
            json!({"smtp_host": "smtp.example.test", "smtp_from": "sender@example.test"});
        endpoint["mail_api_endpoint"] = Value::from("http://127.0.0.1:9");
        let error = checked(&endpoint).expect_err("SMTP takes no endpoint");
        assert!(error.contains("mail_api_endpoint"), "{error}");
    }

    #[test]
    fn the_endpoint_is_https_or_plain_to_this_machine_and_names_a_host_only() {
        let endpoint = |value: &str| {
            let mut keys = cloudflare();
            keys["mail_api_endpoint"] = Value::from(value);
            match checked(&keys) {
                Ok(Some(Way::Cloudflare(keys))) => Ok(keys.endpoint),
                Ok(other) => panic!("{other:?}"),
                Err(error) => Err(error),
            }
        };
        assert_eq!(
            endpoint("https://mail.example.test:8443/"),
            Ok("https://mail.example.test:8443".to_owned())
        );
        assert_eq!(
            endpoint("http://127.0.0.1:8025"),
            Ok("http://127.0.0.1:8025".to_owned())
        );
        assert_eq!(
            endpoint("http://localhost:8025"),
            Ok("http://localhost:8025".to_owned())
        );
        for refused in [
            "http://mail.example.test",
            "ftp://127.0.0.1",
            "https://mail.example.test/client/v4",
            "https://mail.example.test/?x=1",
            "https://user:pass@mail.example.test",
            "not a url",
        ] {
            let error = endpoint(refused).expect_err(refused);
            assert!(error.contains("mail_api_endpoint"), "{refused}: {error}");
        }
        let mut keys = cloudflare();
        keys.as_object_mut()
            .expect("an object")
            .remove("mail_api_endpoint");
        let Ok(Some(Way::Cloudflare(keys))) = checked(&keys) else {
            panic!("the provider's own address");
        };
        assert_eq!(keys.endpoint, "https://api.cloudflare.com");
    }

    #[test]
    fn values_that_become_a_path_or_a_header_have_their_shape() {
        for (key, value) in [
            ("cloudflare_account_id", "abc/../def"),
            ("cloudflare_account_id", "abc?x"),
            ("cloudflare_api_token", "token with spaces"),
            ("cloudflare_api_token", "token\r\nX-Other: 1"),
        ] {
            let mut keys = cloudflare();
            keys[key] = Value::from(value);
            let error = checked(&keys).expect_err(value);
            assert!(error.contains(key), "{key}: {error}");
            assert!(!error.contains(value), "the value is not repeated: {error}");
        }
    }

    #[test]
    fn the_token_never_appears_in_debug_or_type_errors() {
        let config = Configuration::parse(&cloudflare().to_string(), |_| None).expect("parse");
        let debug = format!("{config:?}");
        assert!(!debug.contains(TOKEN), "{debug}");
        let Ok(Some(Way::Cloudflare(keys))) = config.mail.way(&config.smtp) else {
            panic!("Cloudflare");
        };
        let debug = format!("{keys:?}");
        assert!(!debug.contains(TOKEN), "{debug}");
        assert_eq!(keys.api_token.expose(), TOKEN);
        let error = Configuration::parse(r#"{"cloudflare_api_token": 1234567890}"#, |_| None)
            .expect_err("a number is no token")
            .to_string();
        assert!(!error.contains("1234567890"), "{error}");
        assert!(error.contains("cloudflare_api_token"), "{error}");
    }

    /// Environment variables for [`Configuration::parse`].
    #[derive(Default)]
    struct BTreeMapEnv(Vec<(&'static str, &'static str)>);

    impl BTreeMapEnv {
        fn get(&self, name: &str) -> Option<String> {
            self.0
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_owned())
        }
    }
}
