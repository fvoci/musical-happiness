//! Nodes an agent adds after the last of a body, while its editors may type in it: the
//! nodes are written into the card's document as one more editor writes, and the
//! editors are sent the update as another's, so that what they typed meanwhile stays
//! where they typed it and nobody loads the document anew. The body is not written
//! whole: what the editors typed and did not send yet is not lost to it.

use serde_json::Value;

use super::place::Unanswered;
use super::{Collab, whole};
use crate::app::App;
use crate::error::AppError;

/// Who adds nodes to a body: a user, by a session or through the personal API token
/// they gave an agent.
#[derive(Debug, Clone, Copy)]
pub struct Appender<'a> {
    /// The user.
    pub user_id: &'a str,
    /// The token; `None` for a session.
    pub token_id: Option<&'a str>,
}

impl Appender<'_> {
    /// The key a death of the worker over what it sent is put down to
    /// ([`super::senders`]): the token, so that one agent's updates held back hold back
    /// none of its owner's own, nor another agent's; the user, for a session.
    fn sender(self) -> String {
        self.token_id
            .map_or_else(|| self.user_id.to_owned(), |token| format!("token:{token}"))
    }
}

impl Collab {
    /// Adds `nodes`, top-level nodes of a body, after the last of the body of the card
    /// `card_id` of `app`'s organization, by `by`, and writes the body to the
    /// card. Opens the document if it is not open; an open room is sent the update.
    ///
    /// # Errors
    ///
    /// As [`Collab::take`]: the card is gone, the document would be larger than it may
    /// (`document_too_large`), the worker ended over it (`document_unavailable`), or
    /// the token's updates are held back for a while.
    pub async fn append(
        &self,
        app: &App,
        card_id: &str,
        by: Appender<'_>,
        nodes: &[Value],
    ) -> Result<(), AppError> {
        let sender = by.sender();
        self.held_back(app, &sender)?;
        let nodes = serde_json::to_vec(nodes)
            .map_err(|error| AppError::internal(format_args!("the nodes as JSON: {error}")))?;
        let user_id = by.user_id.to_owned();
        let appended = self
            .on_slot_while(
                app,
                card_id,
                None,
                move |collab, app, card_id, mut slot| async move {
                    let (collab, app, card_id, slot) = (&collab, &app, card_id.as_str(), &mut slot);
                    let appended = collab
                        .append_in(app, card_id, slot, &user_id, &sender, &nodes)
                        .await;
                    let written = match appended {
                        Ok(_) => {
                            collab
                                .write_pending(app, card_id, slot, whole::DueVersion::Keep)
                                .await
                        }
                        Err(_) => Ok(()),
                    };
                    collab.retire(slot);
                    written.map(|()| appended)
                },
            )
            .await?
            .map_err(Unanswered::into_error)?;
        let (update, epoch, taken) = appended;
        self.send_appended(app, card_id, epoch, &update, &taken)
            .await;
        Ok(())
    }
}
