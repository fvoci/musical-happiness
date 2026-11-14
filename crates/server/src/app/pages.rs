//! The pages of a wiki: their tree, and making one. A page is a block, and is changed
//! and deleted as one ([`App::patch_block`], [`App::delete_block`]).

use fvoci_model::{NewPage, Page, PageNode};

use super::App;
use crate::error::AppError;

impl App {
    /// The pages of the wiki without their bodies, as their tree lists them.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the pages cannot be read.
    pub async fn page_tree(&self, wiki_id: &str) -> Result<Vec<PageNode>, AppError> {
        Ok(self.store().get_page_tree(wiki_id).await?)
    }

    /// Makes a page of the wiki as `new` says, and tells websocket listeners and
    /// webhooks in the background, as for any block that is made.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if there is no such wiki, and a bad request if the
    /// parent is no page of it, the page to go after is no page of the same parent, or
    /// the title may not be stored.
    pub async fn create_page(
        &self,
        wiki_id: &str,
        new: &NewPage,
        user_id: &str,
    ) -> Result<Page, AppError> {
        let wiki = self.store().get_board(wiki_id).await?;
        let block = {
            let _structure = self.collab().changing_structure(self).await;
            self.store().create_page(wiki_id, new, user_id).await?
        };
        let app = self.clone();
        let made = block.clone();
        self.spawn_background(async move {
            app.ws()
                .broadcast_block_change(&wiki.workspace_id, &made)
                .await;
            app.webhook().notify_update(&made).await;
        });
        Ok(Page::from(block))
    }
}
