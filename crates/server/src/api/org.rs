//! The routes of an organization, under `/org/{orgId}`, for its administrators: the
//! organization's provider of sign-ins.
//!
//! A route here takes [`OrgAdmin`]: the session's user as an administrator of the
//! organization, with the application reaching the rows of that organization. For
//! anybody else the organization is not there (404), whether it exists or not.

use axum::body::Bytes;
use axum::extract::rejection::RawPathParamsRejection;
use axum::extract::{FromRequestParts, RawPathParams};
use axum::http::request::Parts;
use fvoci_model::{OidcProvider, OidcProviderSettings, Session};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::contract::params::OrgId;
use super::contract::tag;
use super::session::session_of;
use super::{ApiResult, decode_object, json_response, no_content};
use crate::app::App;
use crate::error::AppError;

/// The path parameter that names the organization of a route.
const ORG_ID: &str = "orgId";

pub(super) fn routes() -> OpenApiRouter<App> {
    OpenApiRouter::new().routes(routes!(
        handle_get_oidc_provider,
        handle_put_oidc_provider,
        handle_delete_oidc_provider
    ))
}

/// The organization a request names in its path, for an administrator of it.
///
/// Extraction fails with 401 without a session, and with 404 unless the session's user
/// administers the organization: a member who is no administrator, somebody of another
/// organization, and an organization that does not exist are answered alike.
#[derive(Debug)]
pub(crate) struct OrgAdmin {
    /// The application, reaching the rows of the organization.
    pub(crate) app: App,
    /// The session of the request.
    pub(crate) session: Session,
    /// The organization the path names.
    pub(crate) org_id: String,
}

impl FromRequestParts<App> for OrgAdmin {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, app: &App) -> Result<Self, Self::Rejection> {
        let session = session_of(app, &parts.headers, &parts.extensions).await?;
        let params = match RawPathParams::from_request_parts(parts, app).await {
            Ok(params) => params,
            Err(RawPathParamsRejection::InvalidUtf8InPathParam(_)) => {
                return Err(org_not_found());
            }
            Err(_) => return Err(AppError::internal("the route has no path parameters")),
        };
        let org_id = params
            .iter()
            .find(|(name, _)| *name == ORG_ID)
            .map(|(_, value)| value.to_owned())
            .ok_or_else(|| AppError::internal("the route names no organization"))?;
        let app = app
            .enter_org_as_admin(&session.user_id, &org_id)
            .await?
            .ok_or_else(org_not_found)?;
        Ok(Self {
            app,
            session,
            org_id,
        })
    }
}

fn org_not_found() -> AppError {
    AppError::not_found("organization")
}

/// The provider of an organization
///
/// The `OpenID Connect` provider people of the organization sign in through, without
/// its client secret: `hasClientSecret` says whether there is one. 404 if the
/// organization has none.
#[utoipa::path(
    get,
    path = "/org/{orgId}/oidc",
    operation_id = "getOidcProvider",
    tag = tag::AUTH,
    params(OrgId),
    responses((status = 200, description = "The provider.", body = OidcProvider)),
)]
async fn handle_get_oidc_provider(org: OrgAdmin) -> ApiResult {
    let provider = org
        .app
        .oidc_provider()
        .await?
        .ok_or_else(|| AppError::not_found("oidc provider"))?;
    Ok(json_response(&provider))
}

/// Set the provider of an organization
///
/// Sets the organization's `OpenID Connect` provider, in place of the one it has. The
/// issuer's discovery document is read first: it must name the issuer exactly and have
/// `https` endpoints, or the request is 400 with the reason `oidc_issuer_unverified`
/// and `errors` naming `/issuer`. The server reaches only `https` addresses, follows no
/// redirect, and reaches only public addresses unless its configuration lets it
/// (`oidc_allow_private_addresses`).
///
/// `clientSecret` left out keeps the stored secret while the issuer and the client stay
/// the same; with another issuer or client it is required when a secret is stored (400,
/// `required` at `/clientSecret`), as the secret is the old client's. The empty string
/// stores none. The secret is stored sealed with the server's key and never answered:
/// without the key the request is 409 with the reason `secret_key_required`. `autoJoin`
/// and `enabled` left out stay as they are, and are on for a new provider. The
/// answer's `redirectUri` is the address to register with the provider: the provider's
/// own, which another issuer or client (another provider) changes.
#[utoipa::path(
    put,
    path = "/org/{orgId}/oidc",
    operation_id = "putOidcProvider",
    tag = tag::AUTH,
    params(OrgId),
    request_body = OidcProviderSettings,
    responses((status = 200, description = "The provider as stored.", body = OidcProvider)),
)]
async fn handle_put_oidc_provider(org: OrgAdmin, body: Bytes) -> ApiResult {
    let settings: OidcProviderSettings = decode_object(&body)?;
    let provider = org.app.put_oidc_provider(&org.org_id, &settings).await?;
    tracing::debug!(user_id = %org.session.user_id, "the provider was set by an administrator");
    Ok(json_response(&provider))
}

/// Remove the provider of an organization
///
/// Removes the organization's provider and the sign-ins under way through it. The
/// sessions made through it go on. An organization without one answers the same.
#[utoipa::path(
    delete,
    path = "/org/{orgId}/oidc",
    operation_id = "deleteOidcProvider",
    tag = tag::AUTH,
    params(OrgId),
    responses((status = 204, description = "The organization has no provider.")),
)]
async fn handle_delete_oidc_provider(org: OrgAdmin) -> ApiResult {
    org.app.delete_oidc_provider().await?;
    Ok(no_content())
}
