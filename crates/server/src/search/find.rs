//! A search of a workspace's bodies, and the check of what the search server answers.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use fvoci_model::{SearchHit, SearchKind};
use fvoci_store::{PublicBoards, SearchDocument};

use super::client::Hit;
use super::document;
use super::query::{Containers, Scope};
use crate::app::App;
use crate::error::{AppError, Reason};

/// The most hits a search answers with.
pub const MAX_LIMIT: usize = 50;
/// The most characters a search takes.
pub const MAX_TEXT_CHARS: usize = 200;

/// How long the search server may take to answer a search.
const SEARCH_TIMEOUT: Duration = Duration::from_secs(3);
/// The most containers a request to the search server lists. A user who reads more
/// is searched for in the whole workspace, and more is asked for to make up for what
/// the check below drops.
const MAX_LISTED_CONTAINERS: usize = 1000;
/// How many times the documents a search answers with are asked for, where the
/// request lists the user's containers and where it cannot.
const ASKED_FOR_LISTED: usize = 2;
/// See [`ASKED_FOR_LISTED`].
const ASKED_FOR_ALL: usize = 5;

/// What a search asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asked {
    /// The words to find; at most [`MAX_TEXT_CHARS`] characters.
    pub text: String,
    /// The kind of thing to find; `None` for any.
    pub kind: Option<SearchKind>,
    /// How many hits to answer with at most; at most [`MAX_LIMIT`].
    pub limit: usize,
}

impl App {
    /// Finds the cards of a workspace whose title or body holds the words asked for,
    /// among those the user reads, the best match first. For a handle of the
    /// workspace ([`App::in_workspace`]). The user reads the cards of the boards they
    /// have a role on, their own or a team's: an open board they have not joined is
    /// theirs to join, and not searched.
    ///
    /// The search server is asked, and every document it answers with is then asked
    /// of the database (`check_hits`): what the user does not read now is left
    /// out, and nothing says how many were. Words that are empty find nothing.
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
        let text = asked.text.trim();
        if text.is_empty() {
            return Ok(Vec::new());
        }
        let org_id = self
            .store()
            .tenant()
            .org_id()
            .ok_or_else(|| AppError::internal("a search outside an organization"))?;
        let containers: Vec<String> = self
            .boards_for_user_and_workspace(user_id, workspace_id, PublicBoards::Exclude)
            .await?
            .into_iter()
            .map(|board| board.id)
            .collect();
        if containers.is_empty() {
            return Ok(Vec::new());
        }
        let (containers, asked_for) = if containers.len() > MAX_LISTED_CONTAINERS {
            (Containers::All, ASKED_FOR_ALL)
        } else {
            (Containers::Listed(&containers), ASKED_FOR_LISTED)
        };
        let scope = Scope::Workspace {
            org_id: &org_id,
            workspace_id,
            containers,
            kind: asked.kind,
        };
        let size = asked.limit.saturating_mul(asked_for);
        let hits = engine
            .client
            .search(&scope, text, size, SEARCH_TIMEOUT)
            .await
            .map_err(|error| {
                tracing::warn!(%error, "a search was not answered by the search server");
                AppError::service_unavailable("the search is not available for now")
                    .with_reason(Reason::SearchUnavailable)
            })?;
        let mut found = self
            .check_hits(user_id, workspace_id, asked.kind, hits)
            .await?;
        found.truncate(asked.limit);
        Ok(found)
    }

    /// What the user reads now of what the search server answered, in its order.
    ///
    /// The search server's index may be behind what is stored, so a hit is no more
    /// than an identifier to ask about. A hit is kept if the database, as this handle
    /// reaches it, has a document of that identifier now ([`SearchDocument`]: a card
    /// that is no block of the trash, on a board that is no template), in this
    /// workspace, of the kind asked for, on a board the user reads
    /// ([`App::board_access`], asked once a board). What is answered of it is what the
    /// database says: the container, the kind and the title. The piece of text alone
    /// is the search server's, and is left out unless the document was indexed from
    /// the title and the text the card has now.
    async fn check_hits(
        &self,
        user_id: &str,
        workspace_id: &str,
        kind: Option<SearchKind>,
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
        let mut read: HashMap<String, bool> = HashMap::new();
        let mut answered = HashSet::new();
        let mut found = Vec::new();
        for hit in hits {
            let Some(document) = documents.get(&hit.id) else {
                continue;
            };
            if document.workspace_id != workspace_id
                || kind.is_some_and(|kind| kind != document.kind)
                || !answered.insert(hit.id)
            {
                continue;
            }
            let reads = if let Some(reads) = read.get(&document.container_id) {
                *reads
            } else {
                let reads = self
                    .board_access(user_id, &document.container_id)
                    .await?
                    .is_some_and(|access| {
                        access.grant.reads()
                            && !access.board.is_template
                            && access.board.workspace_id == workspace_id
                    });
                read.insert(document.container_id.clone(), reads);
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
