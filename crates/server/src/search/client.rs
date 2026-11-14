//! The requests to the search server, and what its answers mean.
//!
//! Elasticsearch and `OpenSearch` take the same requests here: making and removing an
//! index, describing it, and `_search`. Where the two differ, in how the server signs
//! in and in what an answer that refuses says, this file reads both. Nothing of an
//! answer is kept but its status, the name of its error, and what a search found:
//! never its text, which may repeat what a card says.

use std::time::Duration;

use fvoci_model::SnippetPart;
use serde_json::Value;

use super::index;
use super::query::{MATCH_END, MATCH_START, Scope};
use crate::config::search::{SearchCredentials, SearchServer};

/// The longest the connection to the search server may take to open.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
/// How long a request that makes, removes or describes the index may take.
const INDEX_TIMEOUT: Duration = Duration::from_secs(30);
/// The most of an answer that is read. An answer to a search holds as many pieces of
/// text as it was asked for, each about a line long.
const MOST_BODY: usize = 8 * 1024 * 1024;
/// The most of the name of an error that is kept.
const MOST_KIND: usize = 64;

/// Why no answer came from the search server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NoAnswer {
    /// The connection could not be opened: nothing listens there, or its certificate
    /// does not hold.
    Connection,
    /// The answer did not come in the time the request had.
    Timeout,
    /// Anything else: the connection broke, or the request could not be sent.
    Other,
}

impl std::fmt::Display for NoAnswer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Connection => "no connection",
            Self::Timeout => "no answer in time",
            Self::Other => "the request failed",
        })
    }
}

impl From<&reqwest::Error> for NoAnswer {
    fn from(error: &reqwest::Error) -> Self {
        if error.is_timeout() {
            Self::Timeout
        } else if error.is_connect() {
            Self::Connection
        } else {
            Self::Other
        }
    }
}

/// Why a request to the search server came to nothing.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum ClientError {
    /// No answer: the connection failed, or the answer did not come in time.
    #[error("the search server did not answer ({0})")]
    Unreachable(NoAnswer),
    /// A search was answered with a part of what matches: it ran out of the search
    /// server's own time, or some of the index did not answer.
    #[error("the search server answered a search in part")]
    Partial,
    /// The search server has no analyser of Korean text.
    #[error("the search server has no nori analysis plugin")]
    NoNori,
    /// The search server refused the request: its status, and the name of its error.
    #[error("the search server answered {status} ({kind})")]
    Refused {
        /// The status of the answer.
        status: u16,
        /// The `type` of its error, or `unknown`.
        kind: String,
    },
    /// An answer that is not what the search server answers such a request with.
    #[error("the search server's answer could not be read")]
    Unreadable,
}

impl ClientError {
    /// Whether the search server said that the index is not there.
    pub(crate) fn is_missing_index(&self) -> bool {
        matches!(self, Self::Refused { status: 404, kind } if kind == "index_not_found_exception")
    }
}

/// What is at the name of the index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum IndexFound {
    /// Nothing.
    Absent,
    /// An index this server made, from the definition of this version.
    Ours {
        /// The version of [`index::definition`] it was made from.
        version: u64,
    },
    /// An index that does not say it is this server's.
    Foreign,
}

/// What a search found of one document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Hit {
    /// The identifier of the document, which is the card's.
    pub(crate) id: String,
    /// The digest the document was indexed with.
    pub(crate) digest: String,
    /// A piece of the text it was indexed with.
    pub(crate) snippet: Vec<SnippetPart>,
}

/// The client of the search server: no redirect is followed and no proxy of the
/// environment is used. A request carries the server's credentials and what cards
/// say, and goes where `search_url` says, nowhere else.
#[derive(Debug, Clone)]
pub(crate) struct Client {
    http: reqwest::Client,
    /// `scheme://host[:port]`.
    base: String,
    index: String,
    credentials: SearchCredentials,
}

/// An answer of the search server, as much as is read of it.
struct Answer {
    status: u16,
    body: Value,
}

impl Answer {
    /// The refusal this answer is, if its status says it is one.
    fn refusal(&self) -> Option<ClientError> {
        if (200..300).contains(&self.status) {
            return None;
        }
        Some(ClientError::Refused {
            status: self.status,
            kind: kind_of(&self.body),
        })
    }
}

/// The name of the error of an answer: letters, digits and underscores, as both
/// search servers name theirs, and `unknown` for anything else.
fn kind_of(body: &Value) -> String {
    body.pointer("/error/type")
        .and_then(Value::as_str)
        .filter(|kind| {
            !kind.is_empty()
                && kind.len() <= MOST_KIND
                && kind.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
        .unwrap_or("unknown")
        .to_owned()
}

impl Client {
    /// The client of the search server the configuration names.
    ///
    /// # Errors
    ///
    /// Returns an error if the TLS of the client cannot be set up.
    pub(crate) fn new(server: &SearchServer) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .connect_timeout(CONNECT_TIMEOUT)
            .build()?;
        Ok(Self {
            http,
            base: server.url.clone(),
            index: server.index.clone(),
            credentials: server.credentials.clone(),
        })
    }

    /// The address of the index, or of `under` it (`/_search`).
    fn url(&self, under: &str) -> String {
        format!("{}/{}{under}", self.base, self.index)
    }

    /// Sends `request` as the server signs in, and reads the answer.
    async fn send(
        &self,
        request: reqwest::RequestBuilder,
        timeout: Duration,
    ) -> Result<Answer, ClientError> {
        let request = match &self.credentials {
            SearchCredentials::None => request,
            SearchCredentials::Basic { username, password } => {
                request.basic_auth(username, Some(password))
            }
            SearchCredentials::ApiKey(key) => {
                request.header("authorization", format!("ApiKey {key}"))
            }
        };
        let exchange = async {
            let mut response = request
                .send()
                .await
                .map_err(|error| ClientError::Unreachable(NoAnswer::from(&error)))?;
            let status = response.status().as_u16();
            let mut body = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|error| ClientError::Unreachable(NoAnswer::from(&error)))?
            {
                if body.len() + chunk.len() > MOST_BODY {
                    return Err(ClientError::Unreadable);
                }
                body.extend_from_slice(&chunk);
            }
            // An answer without a body, or with one that is no JSON (a proxy's page),
            // still has its status.
            let body = serde_json::from_slice(&body).unwrap_or(Value::Null);
            Ok(Answer { status, body })
        };
        tokio::time::timeout(timeout, exchange)
            .await
            .unwrap_or(Err(ClientError::Unreachable(NoAnswer::Timeout)))
    }

    /// What is at the name of the index.
    ///
    /// # Errors
    ///
    /// Returns why the search server did not say.
    pub(crate) async fn describe_index(&self) -> Result<IndexFound, ClientError> {
        let answer = self
            .send(self.http.get(self.url("")), INDEX_TIMEOUT)
            .await?;
        if answer.status == 404 {
            return Ok(IndexFound::Absent);
        }
        if let Some(refusal) = answer.refusal() {
            return Err(refusal);
        }
        // The description is under the name of the index, or under the name of the
        // index an alias of this name stands for: one index either way.
        let described = answer
            .body
            .as_object()
            .filter(|indices| indices.len() == 1)
            .and_then(|indices| indices.values().next())
            .ok_or(ClientError::Unreadable)?;
        Ok(match index::version_of(described) {
            Some(version) => IndexFound::Ours { version },
            None => IndexFound::Foreign,
        })
    }

    /// Makes the index, from [`index::definition`].
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::NoNori`] if the search server knows no nori, and
    /// otherwise why the index was not made: one that is there already is a refusal.
    pub(crate) async fn create_index(&self) -> Result<(), ClientError> {
        let request = self.http.put(self.url("")).json(&index::definition());
        let answer = self.send(request, INDEX_TIMEOUT).await?;
        let Some(refusal) = answer.refusal() else {
            return Ok(());
        };
        // Both say `Unknown tokenizer type [nori_tokenizer] for [...]`, or the same of
        // a filter of nori's, with the status 400.
        let names_nori = answer
            .body
            .pointer("/error/reason")
            .and_then(Value::as_str)
            .is_some_and(|reason| reason.contains("nori_"));
        if answer.status == 400 && names_nori {
            return Err(ClientError::NoNori);
        }
        Err(refusal)
    }

    /// Removes the index; one that is not there is removed.
    ///
    /// # Errors
    ///
    /// Returns why the index was not removed.
    pub(crate) async fn delete_index(&self) -> Result<(), ClientError> {
        let answer = self
            .send(self.http.delete(self.url("")), INDEX_TIMEOUT)
            .await?;
        match answer.refusal() {
            Some(refusal) if answer.status != 404 => Err(refusal),
            _ => Ok(()),
        }
    }

    /// Finds up to `size` documents of `scope` by `text`, past the `from` that match
    /// best, the best match first, within `timeout`.
    ///
    /// # Errors
    ///
    /// Returns why nothing was found out: an index that is not there is a refusal, and
    /// an answer that holds a part of what matches is none.
    pub(crate) async fn search(
        &self,
        scope: &Scope<'_>,
        text: &str,
        from: usize,
        size: usize,
        timeout: Duration,
    ) -> Result<Vec<Hit>, ClientError> {
        let request = self
            .http
            .post(self.url("/_search"))
            .json(&scope.search(text, from, size));
        let answer = self.send(request, timeout).await?;
        if let Some(refusal) = answer.refusal() {
            return Err(refusal);
        }
        let timed_out = answer.body["timed_out"].as_bool().unwrap_or(false);
        let failed_shards = answer
            .body
            .pointer("/_shards/failed")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        if timed_out || failed_shards > 0 {
            return Err(ClientError::Partial);
        }
        let hits = answer
            .body
            .pointer("/hits/hits")
            .and_then(Value::as_array)
            .ok_or(ClientError::Unreadable)?;
        hits.iter()
            .map(|hit| {
                let id = hit.get("_id").and_then(Value::as_str);
                Ok(Hit {
                    id: id.ok_or(ClientError::Unreadable)?.to_owned(),
                    digest: hit
                        .pointer("/_source/digest")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    snippet: hit
                        .pointer("/highlight/text/0")
                        .and_then(Value::as_str)
                        .map(snippet_parts)
                        .unwrap_or_default(),
                })
            })
            .collect()
    }
}

/// A piece of text as the search server marked it, in stretches: those between
/// [`MATCH_START`] and [`MATCH_END`] matched. Marks that do not pair change nothing
/// but which stretch a character falls in; no mark is ever text.
fn snippet_parts(marked: &str) -> Vec<SnippetPart> {
    let mut parts: Vec<SnippetPart> = Vec::new();
    let mut matched = false;
    for c in marked.chars() {
        match c {
            MATCH_START => matched = true,
            MATCH_END => matched = false,
            c => match parts.last_mut() {
                Some(part) if part.matched == matched => part.text.push(c),
                _ => parts.push(SnippetPart {
                    text: c.to_string(),
                    matched,
                }),
            },
        }
    }
    parts
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn plain(text: &str) -> SnippetPart {
        SnippetPart {
            text: text.to_owned(),
            matched: false,
        }
    }

    fn matched(text: &str) -> SnippetPart {
        SnippetPart {
            matched: true,
            ..plain(text)
        }
    }

    #[test]
    fn a_marked_piece_becomes_stretches_and_neighbouring_matches_one() {
        let marked =
            format!("지나면 {MATCH_START}환불{MATCH_END}{MATCH_START}규정{MATCH_END}을 따라");
        assert_eq!(
            snippet_parts(&marked),
            [plain("지나면 "), matched("환불규정"), plain("을 따라")]
        );
        assert_eq!(snippet_parts("no match"), [plain("no match")]);
        assert_eq!(snippet_parts(""), []);
        // Marks a body held itself, or that do not pair.
        assert_eq!(
            snippet_parts(&format!("a{MATCH_END}b{MATCH_START}c")),
            [plain("ab"), matched("c")]
        );
    }

    #[test]
    fn an_error_is_named_by_its_type_and_never_by_its_text() {
        let named = json!({"error": {"type": "index_not_found_exception", "reason": "no such index [secret-text]"}});
        assert_eq!(kind_of(&named), "index_not_found_exception");
        for unnamed in [
            json!(null),
            json!({"error": "a string"}),
            json!({"error": {"type": "has spaces and <text>"}}),
            json!({"error": {"type": "x".repeat(MOST_KIND + 1)}}),
        ] {
            assert_eq!(kind_of(&unnamed), "unknown", "{unnamed}");
        }
        let refused = ClientError::Refused {
            status: 404,
            kind: kind_of(&named),
        };
        assert!(!refused.to_string().contains("secret-text"), "{refused}");
    }
}
