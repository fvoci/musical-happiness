//! Starting an organization from an account that is there already: the one route about
//! organizations that is under none. What is done to an organization that exists is
//! under `/org/{orgId}` (`org`, `org_admin`).

use axum::body::Bytes;
use axum::extract::{Extension, State};
use fvoci_model::{NewOrganization, Problem, UserWorkspace};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::auth::address_of;
use super::contract::tag;
use super::{ApiResult, RequiredSession, created, decode_object};
use crate::app::App;
use crate::client_address::ClientAddress;

pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new().routes(routes!(handle_create_organization))
}

/// Start an organization
///
/// Makes an organization that people work in together, and its first workspace, both
/// under `name`, for the account of the session, which administers both: what
/// `register` does with `organizationName`, for an account that is there. Answers the
/// workspace as `getMyWorkspaces` lists it from then on.
///
/// An account makes a few organizations and no more: the ones in use that it started
/// are counted, by `register` or here, and not the ones it was invited into, nor its
/// own space. One more than the limit is 400 with the reason `too_many_organizations`,
/// and `limit` tells how many that is. On a server that takes one organization
/// (`org_creation` is `first_user_only`; `registrationOpen` of `getLoginOptions` is
/// false once it has its first user) nobody starts another: 403 with the reason
/// `invitation_required`. The name is read as `organizationName` of `register` is:
/// blank, too long or with a character it may not have is 400, and `errors` names
/// `name`. Counted by where it comes from, with the registrations: too many are 429.
#[utoipa::path(
    post,
    path = "/org",
    operation_id = "createOrganization",
    tag = tag::ORGANIZATION,
    request_body = NewOrganization,
    responses(
        (status = 201, description = "The first workspace of the organization that was made.", body = UserWorkspace),
        (
            status = 429,
            description = "Too many registrations and organizations from where this one comes from.",
            body = Problem,
            content_type = "application/problem+json",
            headers(("Retry-After" = u64, description = "How many seconds to wait before trying again.")),
        ),
    ),
)]
async fn handle_create_organization(
    State(app): State<App>,
    RequiredSession(session): RequiredSession,
    client: Option<Extension<ClientAddress>>,
    body: Bytes,
) -> ApiResult {
    let request: NewOrganization = decode_object(&body)?;
    let name = request.stored_name()?;
    let workspace = app
        .start_organization(&session, &name, address_of(client))
        .await?;
    Ok(created(&workspace))
}
