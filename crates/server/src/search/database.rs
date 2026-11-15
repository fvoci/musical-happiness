//! The search of a workspace's bodies in the database, for a server without a search
//! server (`search_url` empty).
//!
//! A card or a page is found if every word asked for (what was typed, split at white
//! space) is in its title or in its body as it is written there: "환불" finds
//! "환불규정을", and "배포하다" does not find "배포합니다". ASCII letters match in either
//! case, and no other letter does. Those whose title holds every word come first, then
//! the most recently changed. What the database answers is checked as what a search
//! server answers is ([`App::check_hits`]), and a search that takes longer than its
//! time is stopped in the database and answered with a 503.

use std::sync::Arc;
use std::time::Instant;

use fvoci_model::{SearchHit, SearchKind};
use fvoci_store::{Store, TextFound, TextSearch};

use super::Search;
use super::find::{Asked, Check, DEEPEST, Pages, found_hit, unavailable};
use super::query::Containers;
use super::snippet::snippet;
use crate::app::App;
use crate::error::AppError;

/// The most containers a statement names, one bound value each, well within what both
/// databases bind: a user who reads more is searched for in the whole workspace, a page
/// after another, and what comes back is checked the same.
pub(super) const MAX_BOUND_CONTAINERS: usize = 10_000;

/// The words of what was typed, each once, in the order typed.
fn words(text: &str) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        if !words.iter().any(|seen| seen == word) {
            words.push(word.to_owned());
        }
    }
    words
}

/// What a search asks the database, owned: each page is asked in a task of its own.
struct Asking {
    workspace_id: String,
    containers: Option<Vec<String>>,
    kind: Option<SearchKind>,
    words: Vec<String>,
}

impl Asking {
    /// A page of what the database finds, `size` past the `from` best, by `by`: once
    /// one of the searches of the database that may run at once is free, and
    /// [`TextFound::OutOfTime`] if none is by then.
    ///
    /// Asked in a task of its own, which runs to its end however soon the request is
    /// given up (a browser drops the search of every word typed before the last):
    /// the statement is stopped by `by` all the same, and the connection goes back to
    /// the pool as it came.
    async fn page(
        &self,
        search: &Search,
        store: &Store,
        from: i64,
        size: i64,
        by: Instant,
    ) -> Result<TextFound, AppError> {
        let turn = Arc::clone(&search.database_searches);
        let Ok(Ok(turn)) = tokio::time::timeout_at(by.into(), turn.acquire_owned()).await else {
            return Ok(TextFound::OutOfTime);
        };
        let store = store.clone();
        let workspace_id = self.workspace_id.clone();
        let containers = self.containers.clone();
        let kind = self.kind;
        let words = self.words.clone();
        let page = tokio::spawn(async move {
            let _turn = turn;
            let search = TextSearch {
                workspace_id: &workspace_id,
                containers: containers.as_deref(),
                kind,
                words: &words,
            };
            store.search_text(&search, from, size, by).await
        });
        page.await
            .map_err(|error| AppError::internal(format_args!("the search task failed: {error}")))?
            .map_err(AppError::from)
    }
}

impl App {
    /// The pages of a search in the database, checked, until there are enough.
    pub(super) async fn search_database(
        &self,
        check: &mut Check<'_>,
        asked: &Asked,
        pages: &Pages<'_>,
    ) -> Result<Vec<SearchHit>, AppError> {
        let words = words(&asked.text);
        let asking = Asking {
            workspace_id: check.workspace_id.to_owned(),
            containers: match pages.containers {
                Containers::Listed(ids) => Some(ids.to_vec()),
                Containers::All => None,
            },
            kind: asked.kind,
            words: words.clone(),
        };
        let size = i64::try_from(pages.size).unwrap_or(i64::MAX);
        let mut found = Vec::new();
        let mut from = 0_i64;
        loop {
            let TextFound::Ids(ids) = asking
                .page(self.search(), self.store(), from, size, pages.by)
                .await?
            else {
                tracing::warn!(
                    words = words.len(),
                    "a search of the database did not end in its time"
                );
                return Err(unavailable());
            };
            let more_may_match = ids.len() == pages.size;
            let kept = self.check_hits(check, ids, |id| id).await?;
            found.extend(kept.into_iter().map(|(_, document)| {
                let snippet = snippet(&document.text, &words);
                found_hit(document, snippet)
            }));
            from = from.saturating_add(size);
            let deepest = i64::try_from(DEEPEST).unwrap_or(i64::MAX);
            if found.len() >= asked.limit || !more_may_match || from.saturating_add(size) > deepest
            {
                break;
            }
        }
        found.truncate(asked.limit);
        Ok(found)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_words_are_what_is_between_white_space_each_once() {
        assert_eq!(
            words("환불  규정\t환불\u{3000}API"),
            ["환불", "규정", "API"]
        );
        assert_eq!(words("   "), Vec::<String>::new());
    }
}
