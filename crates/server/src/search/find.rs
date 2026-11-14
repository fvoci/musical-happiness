//! A search of a workspace's bodies, and the check of what the search server answers.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use fvoci_model::{SearchHit, SearchKind};
use fvoci_store::{PublicBoards, SearchDocument};

use super::client::{ClientError, Hit};
use super::query::{Containers, Scope};
use super::{Engine, IndexState, document};
use crate::app::App;
use crate::error::{AppError, Reason};

/// The most hits a search answers with.
pub const MAX_LIMIT: usize = 50;
/// The most characters a search takes.
pub const MAX_TEXT_CHARS: usize = 200;

/// How long the search server may take to answer a search, all its pages together.
const SEARCH_TIMEOUT: Duration = Duration::from_secs(3);
/// The most containers a request to the search server lists. A user who reads more
/// is searched for in the whole workspace, and what comes back is checked the same.
const MAX_LISTED_CONTAINERS: usize = 1000;
/// How many times the documents a search answers with a page asks for, where the
/// request lists the user's containers and where it cannot: the check below drops few
/// of the first, and may drop most of the second.
const ASKED_FOR_LISTED: usize = 2;
/// See [`ASKED_FOR_LISTED`].
const ASKED_FOR_ALL: usize = 5;
/// How far into what matches a search reads, a page after another: what both search
/// servers page to by default.
const DEEPEST: usize = 10_000;

/// What a search asks for: words, and at most so many of a kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asked {
    text: String,
    kind: Option<SearchKind>,
    limit: usize,
}

/// What of a search's parameters is out of bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AskedError {
    /// The words are more than [`MAX_TEXT_CHARS`] characters.
    #[error("the words are more than {MAX_TEXT_CHARS} characters")]
    TextTooLong,
    /// The number of hits is none, or more than [`MAX_LIMIT`].
    #[error("the number of hits is from 1 to {MAX_LIMIT}")]
    LimitOutOfRange,
}

impl Asked {
    /// A search for `text`, of `kind` if one is given, answering at most `limit` hits.
    ///
    /// # Errors
    ///
    /// Returns which of the two is out of bounds: more than [`MAX_TEXT_CHARS`]
    /// characters, or a limit that is not from 1 to [`MAX_LIMIT`].
    pub fn new(text: &str, kind: Option<SearchKind>, limit: usize) -> Result<Self, AskedError> {
        let text = text.trim();
        if text.chars().count() > MAX_TEXT_CHARS {
            return Err(AskedError::TextTooLong);
        }
        if !(1..=MAX_LIMIT).contains(&limit) {
            return Err(AskedError::LimitOutOfRange);
        }
        Ok(Self {
            text: text.to_owned(),
            kind,
            limit,
        })
    }
}

/// The 503 of a search the search server did not answer.
fn unavailable() -> AppError {
    AppError::service_unavailable("the search is not available for now")
        .with_reason(Reason::SearchUnavailable)
}

impl App {
    /// Finds the cards of a workspace whose title or body holds the words asked for,
    /// among those the user reads, the best match first. For a handle of the
    /// workspace ([`App::in_workspace`]). The user reads the cards of the boards they
    /// have a role on, their own or a team's: an open board they have not joined is
    /// theirs to join, and not searched.
    ///
    /// The search server is asked, and every document it answers with is then asked
    /// of the database (`check_hits`): what the user does not read now is left out.
    /// Where that leaves fewer than were asked for, the search server is asked for
    /// what comes next, until there are enough, or nothing more matches: how many an
    /// answer holds says nothing of what was left out. Words that are empty find
    /// nothing.
    ///
    /// # Errors
    ///
    /// Returns a 404 with the reason `search_not_configured` where bodies cannot be
    /// searched, a 429 for a user who searches too often, a 503 with the reason
    /// `search_unavailable` if the search server does not answer the search in three
    /// seconds, and the store's error if a fact cannot be read.
    pub async fn search_workspace(
        &self,
        user_id: &str,
        workspace_id: &str,
        asked: &Asked,
    ) -> Result<Vec<SearchHit>, AppError> {
        let engine = self.search().engine().ok_or_else(|| {
            AppError::not_found("search").with_reason(Reason::SearchNotConfigured)
        })?;
        engine
            .limiter
            .take(user_id, Instant::now())
            .map_err(AppError::rate_limited)?;
        if asked.text.is_empty() {
            return Ok(Vec::new());
        }
        let org_id = self
            .store()
            .tenant()
            .org_id()
            .ok_or_else(|| AppError::internal("a search outside an organization"))?;
        let theirs: HashSet<String> = self
            .boards_for_user_and_workspace(user_id, workspace_id, PublicBoards::Exclude)
            .await?
            .into_iter()
            .map(|board| board.id)
            .collect();
        if theirs.is_empty() {
            return Ok(Vec::new());
        }
        let listed: Vec<String> = theirs.iter().cloned().collect();
        let (containers, asked_for) = if listed.len() > MAX_LISTED_CONTAINERS {
            (Containers::All, ASKED_FOR_ALL)
        } else {
            (Containers::Listed(&listed), ASKED_FOR_LISTED)
        };
        let scope = Scope::Workspace {
            org_id: &org_id,
            workspace_id,
            containers,
            kind: asked.kind,
        };
        let size = asked.limit.saturating_mul(asked_for);
        let mut check = Check {
            user_id,
            workspace_id,
            kind: asked.kind,
            theirs: &theirs,
            reads: HashMap::new(),
            answered: HashSet::new(),
        };
        let by = Instant::now() + SEARCH_TIMEOUT;
        let mut found = Vec::new();
        let mut from = 0;
        loop {
            let left = by.saturating_duration_since(Instant::now());
            let hits = engine
                .client
                .search(&scope, &asked.text, from, size, left)
                .await
                .map_err(|error| search_failed(engine, &error))?;
            let more_may_match = hits.len() == size;
            found.extend(self.check_hits(&mut check, hits).await?);
            from += size;
            if found.len() >= asked.limit || !more_may_match || from + size > DEEPEST {
                break;
            }
        }
        found.truncate(asked.limit);
        Ok(found)
    }

    /// What the user reads now of what the search server answered, in its order.
    ///
    /// The search server's index may be behind what is stored, so a hit is no more
    /// than an identifier to ask about. A hit is kept if the database, as this handle
    /// reaches it, has a document of that identifier now ([`SearchDocument`]: a card
    /// that is no block of the trash, on a board that is no template), in this
    /// workspace, of the kind asked for, on a board the user has a role on and reads
    /// ([`App::board_access`], asked once a board). What is answered of it is what the
    /// database says: the container, the kind and the title. The piece of text alone
    /// is the search server's, and is left out unless the document was indexed from
    /// the title and the text the card has now.
    async fn check_hits(
        &self,
        check: &mut Check<'_>,
        hits: Vec<Hit>,
    ) -> Result<Vec<SearchHit>, AppError> {
        let ids: Vec<String> = hits.iter().map(|hit| hit.id.clone()).collect();
        let documents: HashMap<String, SearchDocument> = self
            .store()
            .search_documents(&ids)
            .await?
            .into_iter()
            .map(|document| (document.id.clone(), document))
            .collect();
        let mut found = Vec::new();
        for hit in hits {
            let Some(document) = documents.get(&hit.id) else {
                continue;
            };
            // A board the user has no role on is asked nothing about: what the index
            // answers of the boards of others costs the same as what it does not.
            if document.workspace_id != check.workspace_id
                || check.kind.is_some_and(|kind| kind != document.kind)
                || !check.theirs.contains(&document.container_id)
                || !check.answered.insert(hit.id)
            {
                continue;
            }
            let reads = if let Some(reads) = check.reads.get(&document.container_id) {
                *reads
            } else {
                let reads = self
                    .board_access(check.user_id, &document.container_id)
                    .await?
                    .is_some_and(|access| {
                        access.grant.reads()
                            && !access.board.is_template
                            && access.board.workspace_id == check.workspace_id
                    });
                check.reads.insert(document.container_id.clone(), reads);
                reads
            };
            if !reads {
                continue;
            }
            let current = hit.digest == document::digest(&document.title, &document.text);
            found.push(SearchHit {
                id: document.id.clone(),
                kind: document.kind,
                container_id: document.container_id.clone(),
                title: document.title.clone(),
                snippet: if current { hit.snippet } else { Vec::new() },
            });
        }
        Ok(found)
    }
}

/// What a search checks its hits against, and what it has learned so far: kept over
/// the pages of one search.
struct Check<'a> {
    user_id: &'a str,
    workspace_id: &'a str,
    kind: Option<SearchKind>,
    /// The boards of the workspace the user has a role on.
    theirs: &'a HashSet<String>,
    /// Whether the user reads a board, for the boards asked about.
    reads: HashMap<String, bool>,
    /// The identifiers answered already: a document comes back once.
    answered: HashSet<String>,
}

/// The 503 of a search that failed so, noted. An index that is gone is looked for
/// again by the upkeep.
fn search_failed(engine: &Engine, error: &ClientError) -> AppError {
    tracing::warn!(%error, "a search was not answered by the search server");
    if error.is_missing_index() {
        engine.set_index_state(IndexState::Unknown);
    }
    unavailable()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_asked_for_is_within_bounds() {
        let asked = Asked::new("  환불 규정 ", Some(SearchKind::Card), 50).expect("in bounds");
        assert_eq!(asked.text, "환불 규정");
        assert_eq!(Asked::new("", None, 1).expect("empty words").text, "");
        Asked::new(&"가".repeat(MAX_TEXT_CHARS), None, 20).expect("the longest");
        assert_eq!(
            Asked::new(&"가".repeat(MAX_TEXT_CHARS + 1), None, 20),
            Err(AskedError::TextTooLong)
        );
        for limit in [0, MAX_LIMIT + 1, usize::MAX] {
            assert_eq!(
                Asked::new("x", None, limit),
                Err(AskedError::LimitOutOfRange),
                "{limit}"
            );
        }
    }
}
