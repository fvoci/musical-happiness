//! The pages of a wiki: their tree, making one and moving one. A page is a block, and
//! is changed and deleted as one ([`App::patch_block`], [`App::delete_block`]).

use fvoci_model::{Block, NewPage, Page, PageMove, PageNode};
use fvoci_store::PageWrite;

use super::App;
use super::blocks::{ChangeAction, ChangeNotification, log_notification};
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

    /// Makes a page of the wiki as `new` says, held to the organization's limits, and
    /// tells websocket listeners and webhooks in the background of it and of the pages
    /// numbered anew for it, as of any block that is written.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if there is no such wiki, and a bad request if the
    /// parent is no page of it, the page would be deeper or the wiki larger than the
    /// limits let them, the page to go after is no page of the same parent, or the
    /// title may not be stored.
    pub async fn create_page(
        &self,
        wiki_id: &str,
        new: &NewPage,
        user_id: &str,
    ) -> Result<Page, AppError> {
        let wiki = self.store().get_board(wiki_id).await?;
        let limits = self.limits().pages;
        let written = {
            let _structure = self.collab().changing_structure(self).await;
            self.store()
                .create_page(wiki_id, new, limits, user_id)
                .await?
        };
        log_notification(
            ChangeNotification::Send,
            ChangeAction::Add,
            &written.page.id,
        );
        Ok(self.announce_pages(&wiki.workspace_id, written))
    }

    /// Moves a page of the wiki, with every page below it, where `to` says, and tells
    /// websocket listeners and webhooks in the background of it and of the pages
    /// numbered anew for it. The pages below it are not told of: their parents stay.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if the wiki has no such page, and a bad request if the
    /// parent is no page of the wiki, the page itself or one below it, or would leave a
    /// page deeper than the limit, or the page to go after is no other page there.
    pub async fn move_page(
        &self,
        wiki_id: &str,
        page_id: &str,
        to: &PageMove,
        user_id: &str,
    ) -> Result<Page, AppError> {
        let wiki = self.store().get_board(wiki_id).await?;
        let limits = self.limits().pages;
        let written = {
            let _structure = self.collab().changing_structure(self).await;
            self.store()
                .move_page(wiki_id, page_id, to, limits, user_id)
                .await?
        };
        log_notification(ChangeNotification::Send, ChangeAction::Update, page_id);
        Ok(self.announce_pages(&wiki.workspace_id, written))
    }

    /// Tells websocket listeners and webhooks in the background of the pages written,
    /// the siblings numbered anew first, and returns the page the write was about.
    fn announce_pages(&self, workspace_id: &str, written: PageWrite) -> Page {
        let PageWrite { page, renumbered } = written;
        let mut told: Vec<Block> = renumbered;
        told.push(page.clone());
        let app = self.clone();
        let workspace_id = workspace_id.to_owned();
        self.spawn_background(async move {
            for block in &told {
                app.ws().broadcast_block_change(&workspace_id, block).await;
                app.webhook().notify_update(block).await;
            }
        });
        Page::from(page)
    }
}
