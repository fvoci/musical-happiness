//! What the user of the session was told: mentions, cards they were given, and
//! comments on what they watch.

use axum::extract::{Path, RawQuery, State};
use fvoci_model::{Notification, UnreadNotifications};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::contract::params::{NotificationId, Page, PerPage, ReadThrough};
use super::contract::tag;
use super::paging::{int_query_param, paging_params};
use super::{ApiResult, RequiredSession, json_response, no_content};
use crate::app::App;
use crate::auth::now_millis;

pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new()
        .routes(routes!(handle_get_notifications))
        .routes(routes!(handle_get_unread_notifications))
        .routes(routes!(handle_read_notifications))
        .routes(routes!(handle_read_notification))
}

/// The notifications of the user
///
/// One page of what the user of the session was told, in every organization, the
/// newest first: that somebody mentioned them in a card, a page or a comment
/// (`mention`), or gave them a card (`assignment`). Nobody is told of their own doing,
/// nor of anything in a user's own space.
///
/// A notification is made only for somebody who reads the card or the page at that
/// time, and each one listed is asked again: its `subject` says what it is about, as
/// that is now, only where the user reads it now with this session. Where they do not
/// (it is in the trash or gone, they left the board or the workspace, or its
/// organization takes a sign-in this session was not made through) `subject` is
/// `null`, and nothing says what it was about, where or by whom. Such a one is still
/// listed, and counted while unread.
///
/// A notification that was read is kept for ninety days, and a user has ten thousand
/// at most: past that the oldest go, those that were read first.
#[utoipa::path(
    get,
    path = "/notifications",
    operation_id = "getNotifications",
    tag = tag::NOTIFICATIONS,
    params(Page, PerPage),
    responses((status = 200, description = "The notifications of the page, the newest first.", body = Vec<Notification>)),
)]
async fn handle_get_notifications(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    RawQuery(query): RawQuery,
) -> ApiResult {
    let page = paging_params(query.as_deref())?;
    Ok(json_response(&app.notifications(&session, page).await?))
}

/// How many notifications the user has not read
///
/// The number of notifications of the user of the session that are not read, in every
/// organization: those whose `subject` is `null` among them.
#[utoipa::path(
    get,
    path = "/notifications/unread",
    operation_id = "getUnreadNotifications",
    tag = tag::NOTIFICATIONS,
    responses((status = 200, description = "How many.", body = UnreadNotifications)),
)]
async fn handle_get_unread_notifications(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
) -> ApiResult {
    let count = app.unread_notifications(&session.user_id).await?;
    Ok(json_response(&UnreadNotifications { count }))
}

/// Mark every notification as read
///
/// Marks as read every notification of the user of the session that was made up to
/// `through`, or up to now where that is left out: a client gives the `createAt` of
/// the newest it has shown, so that one that came since stays unread.
#[utoipa::path(
    post,
    path = "/notifications/read",
    operation_id = "readNotifications",
    tag = tag::NOTIFICATIONS,
    params(ReadThrough),
    responses((status = 204, description = "They are read.")),
)]
async fn handle_read_notifications(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    RawQuery(query): RawQuery,
) -> ApiResult {
    let through = int_query_param(query.as_deref(), "through", now_millis())?;
    app.read_notifications(&session.user_id, through).await?;
    Ok(no_content())
}

/// Mark a notification as read
///
/// Marks one notification of the user of the session as read; one that was read stays
/// read since when it was. 404 for a notification of somebody else, as for one that
/// is not there.
#[utoipa::path(
    post,
    path = "/notifications/{notificationId}/read",
    operation_id = "readNotification",
    tag = tag::NOTIFICATIONS,
    params(NotificationId),
    responses((status = 204, description = "It is read.")),
)]
async fn handle_read_notification(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    Path(notification_id): Path<String>,
) -> ApiResult {
    app.read_notification(&session.user_id, &notification_id)
        .await?;
    Ok(no_content())
}
