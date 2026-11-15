//! The kinds of problem the API answers with, and the pages that describe them.
//!
//! The `type` of a problem is `/problems/<name>` under the path the server is served
//! at, and that address answers with a page of text about the kind. The list is made
//! from [`ErrorKind::ALL`] and [`Reason::ALL`], which is all there is: an error has the
//! type of its reason, or else of its kind.

use std::fmt::Write as _;

use axum::Router;
use axum::extract::{OriginalUri, Path};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, X_CONTENT_TYPE_OPTIONS};
use axum::http::{HeaderValue, Uri};
use axum::middleware;
use axum::response::{IntoResponse, Response};
use axum::routing::get;

use crate::error::{AppError, ErrorKind, PROBLEMS_PATH, Reason, type_reference};
use crate::error_response::problems_only;

/// What a kind of problem is the kind of: every error either has a reason, and is the
/// problem of that reason whatever its status, or has none and is the problem of its
/// status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Basis {
    /// The errors of this status that have no reason.
    Status(u16),
    /// The errors with this `reason`; the description says which statuses they come
    /// with.
    Reason(&'static str),
}

/// One kind of problem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProblemType {
    /// The last part of the `type`: `not-found`, `last-admin`.
    pub name: String,
    /// The `title` of every problem of the kind.
    pub title: &'static str,
    /// What the kind is the kind of.
    pub basis: Basis,
    /// What the kind means and what a client can do about it, in English.
    pub description: &'static str,
}

impl ProblemType {
    /// The `type` of the problems of this kind, on a server at the root of its host.
    pub fn type_reference(&self) -> String {
        type_reference(&self.name)
    }

    /// The page about the kind, on a server that is served under `base_path`.
    fn page(&self, base_path: &str) -> String {
        let mut page = format!(
            "{}\n\ntype: {base_path}{}\n",
            self.title,
            self.type_reference()
        );
        // Writing to a string cannot fail.
        let _ = match self.basis {
            Basis::Status(status) => writeln!(page, "status: {status}"),
            Basis::Reason(reason) => writeln!(page, "reason: {reason}"),
        };
        let _ = writeln!(page, "\n{}", self.description);
        page
    }
}

impl From<ErrorKind> for ProblemType {
    fn from(kind: ErrorKind) -> Self {
        Self {
            name: kind.name().to_owned(),
            title: kind.title(),
            basis: Basis::Status(kind.status()),
            description: describe_kind(kind),
        }
    }
}

impl From<Reason> for ProblemType {
    fn from(reason: Reason) -> Self {
        Self {
            name: reason.name(),
            title: reason.title(),
            basis: Basis::Reason(reason.as_str()),
            description: describe_reason(reason),
        }
    }
}

/// Every kind of problem: those of the statuses, in their order, then those of the
/// reasons.
pub fn all() -> Vec<ProblemType> {
    let kinds = ErrorKind::ALL.into_iter().map(ProblemType::from);
    let reasons = Reason::ALL.iter().copied().map(ProblemType::from);
    kinds.chain(reasons).collect()
}

/// The kind of problem with this name, if there is one.
pub fn find(name: &str) -> Option<ProblemType> {
    all().into_iter().find(|problem| problem.name == name)
}

fn describe_kind(kind: ErrorKind) -> &'static str {
    match kind {
        ErrorKind::BadRequest => {
            "The request is malformed or asks for what cannot be: a body that is not the JSON \
             the operation takes, or a member, a parameter or a header with a value it cannot \
             have. Where the refusal is about something of the request, `errors` lists it; a \
             check of the request's shape lists everything it refuses at once."
        }
        ErrorKind::Unauthorized => {
            "The request has no session, or its session is over: sign in and send the request \
             again. The websocket endpoint answers so to a handshake that has no `Origin` \
             header and no bearer token. A shared board's link needs no session: what it does \
             not open is not found."
        }
        ErrorKind::Forbidden => {
            "The user can see what the request is about, but may not do this with it."
        }
        ErrorKind::NotFound => {
            "What the request is about does not exist, or the user may not know that it does: \
             the two are answered alike. A path that is no operation of the API is answered so \
             as well."
        }
        ErrorKind::MethodNotAllowed => {
            "The path exists, but not with this method. The `Allow` header lists the methods \
             it has."
        }
        ErrorKind::Conflict => {
            "The request is fine, but the state of things is against it: what it would make is \
             there already, or what it changes has changed since it was read. Read again and \
             decide anew."
        }
        ErrorKind::PreconditionFailed => {
            "A condition the request sets in its headers does not hold. The download of a \
             file answers so to an `If-Unmodified-Since` before the time of the answer."
        }
        ErrorKind::PayloadTooLarge => {
            "The body of the request, or what the request names, is larger than the server \
             takes. `limit` is the most it takes."
        }
        ErrorKind::RangeNotSatisfiable => {
            "The `Range` header of the request is no range of bytes, or the range is not in \
             the file. For a range that is not in the file, the `Content-Range` header of the \
             answer tells the file's length."
        }
        ErrorKind::RateLimited => {
            "Too many attempts. `retryAfter`, and the `Retry-After` header, say how many \
             seconds to wait before trying again."
        }
        ErrorKind::Internal => {
            "The server failed. The answer does not say how; `requestId` is the name of the \
             request in the server's log."
        }
        ErrorKind::ServiceUnavailable => {
            "The server cannot serve for now: what it depends on does not answer. The \
             readiness check (`GET /api/v1/ready`) answers so while the database or the \
             file storage cannot be used; `detail` names which."
        }
    }
}

/// The description of [`Reason::OidcUnavailable`].
const OIDC_LOGIN_PAGE_UNAVAILABLE: &str = "Status 409 at the start of a sign-in through an \
    organization's provider; on the login page after one, whose reasons come back in the \
    `oidc_error` parameter of its address, by these names. This one: the organization has \
    no provider that is on, or the provider could not be reached, or it was switched off \
    or changed while the person was signing in; or the sign-in cannot end for a reason of \
    the server's: the client secret does not open with its key, the account of the \
    identity was deleted or changed meanwhile, no user name was free, or the server \
    failed. The server's log says which.";

fn describe_reason(reason: Reason) -> &'static str {
    match reason {
        Reason::Csrf => {
            "Status 403. A request that changes something lacks the header `X-Requested-With: \
             XMLHttpRequest`, by which the server knows that no page of another site made a \
             browser send it. A request with a bearer token needs none."
        }
        Reason::ForeignOrigin => {
            "Status 403. The request, or the websocket handshake, has an `Origin` header that \
             is neither the server's own origin nor one the server was configured to accept."
        }
        Reason::InvalidCredentials => {
            "The user name, the email address or the password is wrong; the answer does not \
             say which. Status 401 from a login. Status 400 from a change of the password, \
             whose session is good: `errors` names `/oldPassword`."
        }
        Reason::InvitationRequired
        | Reason::InvitationUnavailable
        | Reason::TooManyRecipients
        | Reason::InvitationMailLimit
        | Reason::MailQueueFull => describe_invitation(reason).unwrap_or_default(),
        Reason::UsernameTaken => {
            "Status 409. Another user has the user name. `errors` names `/username`."
        }
        Reason::EmailTaken => {
            "Status 409. Another user has the email address. `errors` names `/email`."
        }
        Reason::LastAdmin => {
            "Status 409. The change would leave the board, the workspace or the server \
             without an administrator. Make somebody else one first."
        }
        Reason::AdminRequired => "Status 403. Only an administrator of the server may do this.",
        Reason::TeamSelfJoining => {
            "Status 403. While joined as organization administrator, you cannot add yourself to a team. Another workspace administrator may add you. Direct board roles end with the administrative joining."
        }
        Reason::GuestRoleLimit => "Status 403. A guest of a workspace has no role above editor.",
        Reason::SharingDisabled => {
            "Status 403. Sharing a board with whoever has its link is turned off in the \
             configuration of this server."
        }
        Reason::TooManyUploads
        | Reason::UploadsPending
        | Reason::ServerUploadsPending
        | Reason::UploadInParts
        | Reason::ExportRunning
        | Reason::ServerExportsBusy
        | Reason::ImportRunning
        | Reason::ServerImportsBusy => describe_transfer(reason).unwrap_or_default(),
        Reason::SecretKeyRequired
        | Reason::OidcIssuerUnverified
        | Reason::OidcUnavailable
        | Reason::OidcStateInvalid
        | Reason::OidcDenied
        | Reason::OidcTokenRefused
        | Reason::OidcTokenInvalid
        | Reason::OidcEmailMissing
        | Reason::OidcEmailUnverified
        | Reason::OidcEmailTaken
        | Reason::OidcJoinClosed
        | Reason::OidcIdentityTaken
        | Reason::OidcEmailMismatch
        | Reason::OidcTrialAccount
        | Reason::OidcTrialStale
        | Reason::OidcCarryOverRefused
        | Reason::EmailUnverified
        | Reason::PasswordRequired
        | Reason::PasswordNotSet
        | Reason::LastSignIn
        | Reason::SsoRequired
        | Reason::SsoUnproven
        | Reason::SsoEnforced => describe_sign_in(reason).unwrap_or_default(),
        Reason::RelationNotAList
        | Reason::RelationToItself
        | Reason::RelationRepeated
        | Reason::RelationTooMany
        | Reason::RelationNotACard
        | Reason::RelationCardNotFound => describe_relation(reason).unwrap_or_default(),
        Reason::WorkspaceNameTaken
        | Reason::TeamNameTaken
        | Reason::DefaultWorkspace
        | Reason::LastOrgAdmin
        | Reason::PublicLinksOff => describe_organization(reason).unwrap_or_default(),
        // The rest are the reasons of the search of bodies, of the limits of a wiki's tree
        // and of a body edited together, which the functions below name one by one: a
        // reason that is none of theirs fails the tests.
        _ => describe_the_rest(reason),
    }
}

/// The description of a reason of the search of bodies, of the limits of a wiki's tree
/// or of a body edited together.
fn describe_the_rest(reason: Reason) -> &'static str {
    describe_search(reason)
        .or_else(|| describe_page_tree(reason))
        .unwrap_or_else(|| describe_editing_together(reason))
}

/// The descriptions of the reasons of the limits of a wiki's tree.
fn describe_page_tree(reason: Reason) -> Option<&'static str> {
    Some(match reason {
        Reason::PageTooDeep => {
            "Status 400. The page, or the lowest of the pages below it, would be deeper in its \
             wiki than the organization's limit, which `limit` tells (a page at the top is at \
             depth 1; `wiki_page_depth`, 10 unless configured): when a page is made, moved, \
             restored or duplicated. `errors` names `/parentId` when a page is made or moved; a \
             restore and a copy name no parent. Nothing was changed."
        }
        Reason::TooManyPages => {
            "Status 400. The wiki would have more pages than the organization's limit, which \
             `limit` tells (`wiki_pages`, 5000 unless configured; pages in the trash are not \
             counted): when a page is made, restored or duplicated. Nothing was changed. \
             Deleting pages makes room."
        }
        _ => return None,
    })
}

/// The descriptions of the reasons of the search of bodies.
fn describe_search(reason: Reason) -> Option<&'static str> {
    Some(match reason {
        Reason::SearchNotConfigured => {
            "Status 404. The administration socket's reindex, on a server that is configured \
             with no search server: there is no index to make. (A search on such a server \
             searches its database.)"
        }
        Reason::SearchUnavailable => {
            "Status 503. The search was not answered within three seconds: the search \
             server did not answer it, or refused it, or (on a server without a search \
             server) the database did not finish it in time. Or the search server the server \
             is configured with cannot hold its index (it has no nori plugin, or holds another \
             index under the name), which the server says in its log when it finds it: every \
             search is answered so until that is put right and the server started again, and \
             the database does not stand in for it. Nothing else of the server depends on \
             it: what is written meanwhile is found once the search answers again. Try again \
             later."
        }
        _ => return None,
    })
}

/// The descriptions of the reasons of uploads in parts and of archives.
fn describe_transfer(reason: Reason) -> Option<&'static str> {
    Some(match reason {
        Reason::TooManyUploads => {
            "Status 429. The user has as many uploads in parts open as one user may. Complete \
             or abandon one first. No wait is told: what makes room is the user's own doing."
        }
        Reason::UploadsPending => {
            "Status 429. The user's open uploads in parts hold back as many bytes as one \
             user's may. Complete or abandon one first."
        }
        Reason::ServerUploadsPending => {
            "Status 429. The open uploads in parts of all users hold back as many bytes as the \
             server lets them. Try again later."
        }
        Reason::UploadInParts => {
            "Status 413. The file is larger than one request takes (`limit`): send it as an \
             upload in parts, which takes a file up to the largest the server stores."
        }
        Reason::ExportRunning => {
            "Status 429. The user has an export running, and runs one at a time. Try again \
             when it is done."
        }
        Reason::ServerExportsBusy => {
            "Status 429. The server runs as many exports as it may at once. Try again later."
        }
        Reason::ImportRunning => {
            "Status 429. The user has an import running, and runs one at a time. Try again \
             when it is done."
        }
        Reason::ServerImportsBusy => {
            "Status 429. The server runs as many imports as it may at once. Try again later."
        }
        _ => return None,
    })
}

/// The descriptions of the reasons somebody is not let in for, and an invitation's mail is
/// not sent for.
fn describe_invitation(reason: Reason) -> Option<&'static str> {
    Some(match reason {
        Reason::InvitationRequired => {
            "Status 403. The server has one organization, which its first user started by \
             registering. Everybody after gets in by the link of an invitation, and this \
             registration is none."
        }
        Reason::InvitationUnavailable => {
            "Status 404. The link of the invitation lets nobody in. The answer is the same \
             whether there never was such a link, it ended after its seven days, it was \
             used as often as it may be, it was revoked, or who made it may no longer \
             invite. Ask for a new link."
        }
        Reason::TooManyRecipients => {
            "Status 400. An invitation mail request names more recipients than \
             `invitation_mail_recipients` allows (20 unless configured), which `limit` \
             tells. `errors` names `/recipients`. Nothing was sent: send fewer at a time."
        }
        Reason::InvitationMailLimit => {
            "Status 429. The workspace queued as many invitation messages within the last \
             hour as `invitation_mail_per_hour` allows (100 unless configured), counting the \
             recipients of this request. Nothing of it was sent. `Retry-After` and \
             `retryAfter` say when enough of the earlier messages are an hour old."
        }
        Reason::MailQueueFull => {
            "Status 503. The server holds as many invitation messages waiting to be sent as \
             `mail_invitation_queue` allows. Nothing of this request was sent. Try again later."
        }
        _ => return None,
    })
}

/// The descriptions of the reasons the administrators of an organization or of a
/// workspace are refused for.
fn describe_organization(reason: Reason) -> Option<&'static str> {
    Some(match reason {
        Reason::WorkspaceNameTaken => {
            "Status 409. Another workspace of the organization that is in use has the name; \
             one in the trash leaves its name to be taken. `errors` names `/name`, or \
             `/organizationName` where the first registration names the organization so."
        }
        Reason::TeamNameTaken => {
            "Status 409. Another team of the workspace has the name, in the same or another \
             letter case: `Design` and `DESIGN` are one name. `errors` names `/name`."
        }
        Reason::DefaultWorkspace => {
            "Status 409. The organization's default workspace, which whoever joins the \
             organization gets into, is not deleted."
        }
        Reason::LastOrgAdmin => {
            "Status 409. The change would leave the organization without an administrator. \
             Make somebody else one first."
        }
        Reason::PublicLinksOff => {
            "Status 403. The organization has turned its public links off: no board of it is \
             shared by a link until they are on again. Stopping a board's sharing is not \
             refused."
        }
        _ => return None,
    })
}

/// The descriptions of the reasons a value of a card relation is refused for.
fn describe_relation(reason: Reason) -> Option<&'static str> {
    Some(match reason {
        Reason::RelationNotAList => {
            "Status 400. The value of a card relation property (of the `type` `relation`) is \
             `null` or a list of identifiers of cards of the board. `errors` names where the \
             request has the card's values (a block's `fields` or `updatedFields`, a card's \
             `properties` or `updatedProperties`); the words name the property."
        }
        Reason::RelationToItself => {
            "Status 400. A card relation names its own card. `errors` as for \
             `relation-not-a-list`."
        }
        Reason::RelationRepeated => {
            "Status 400. A card relation names a card twice. `errors` as for \
             `relation-not-a-list`."
        }
        Reason::RelationTooMany => {
            "Status 400. A card relation names more cards than one may (`limit`) and one it \
             did not name before. `errors` as for `relation-not-a-list`."
        }
        Reason::RelationNotACard => {
            "Status 400. A card relation names a block of the board that is no card. \
             `errors` as for `relation-not-a-list`."
        }
        Reason::RelationCardNotFound => {
            "Status 400. A card relation names no card of the board: none of that identifier, \
             one removed from the trash, or another board's, which the answer does not tell \
             apart. A card in the trash may be named, and what a value named before is not \
             asked for again. `errors` as for `relation-not-a-list`."
        }
        _ => return None,
    })
}

/// What the reasons of a body edited together mean.
fn describe_editing_together(reason: Reason) -> &'static str {
    match reason {
        Reason::UpdateRefused => {
            "Status 400. The document of a card's body that is edited together does not take \
             the update: it is no update, writes outside the body, or would leave a document \
             that is no body. The document is as it was."
        }
        Reason::UpdateOutOfStep => {
            "Status 409. The update builds on changes the document of the body does not \
             have: the editor that made it holds another document, or one that was made \
             again. Load the document again and make the change there."
        }
        Reason::UpdateTooLarge => {
            "Status 413. The update is larger than the server takes (setting \
             `collab_update_bytes`); `limit` is the limit in bytes. Send the change in \
             smaller updates."
        }
        Reason::DocumentTooLarge => {
            "Status 413. With the update the document of the body, or the body, would be \
             larger than it may be (setting `collab_state_bytes`, and the limit of a body); \
             `limit` is the limit in bytes."
        }
        Reason::TooManyDocuments => {
            "Status 503. The server holds as many documents of bodies being edited as it \
             may (settings `collab_workers`, `collab_documents_per_worker`, \
             `collab_worker_memory_bytes`). Try again when others are closed."
        }
        Reason::DocumentUnavailable => {
            "Status 503. The document of the body cannot be worked on for now: the process \
             that holds the documents ended or did not answer in time, which drops what was \
             under way, or did so again and again with this document, which is closed for \
             a while; or a write of the card's whole body came while what its editors typed \
             could not be written to the card first, and was not done. The document is as \
             it was stored before. Try again later."
        }
        other => describe_rooms(other),
    }
}

/// What the reasons mean that only the rooms of the websocket give (`DOC_REFUSED`,
/// `DOC_NACK`, `DOC_LEFT`).
fn describe_rooms(reason: Reason) -> &'static str {
    match reason {
        Reason::DocumentReadOnly => {
            "Over the websocket only (`DOC_NACK`). The connection is in the room of the \
             card's body to read: its user reads the card's board and may not change its \
             cards. The update went to nobody; the editor that made it holds a change the \
             document does not, and comes into the room anew to drop it."
        }
        Reason::ConnectionTooSlow => {
            "Over the websocket only (`DOC_LEFT`, `DOC_REFUSED`). As many messages wait \
             for the connection as may (settings `collab_queue_messages`, and twice \
             `collab_state_bytes` in bytes), and it was put out of the room, or not let \
             in, so that the others go on. The frames that waited for it of the room \
             are dropped. Come into the room again: the whole document is sent anew."
        }
        Reason::TooManyDocumentsJoined => {
            "Over the websocket only (`DOC_REFUSED`). The connection is in as many rooms \
             as one connection may be (setting `collab_documents_per_connection`). Leave \
             one, or open another connection."
        }
        Reason::DocumentFull => {
            "Over the websocket only (`DOC_REFUSED`). As many connections are in the room \
             of the card's body as may be (setting `collab_connections_per_document`). \
             Try again when one has left."
        }
        Reason::TooManyConnectionsJoined => {
            "Over the websocket only (`DOC_REFUSED`). As many of the user's own connections \
             are in the room of the card's body as one user's may be (setting \
             `collab_connections_per_user`): the card is open in that many tabs or \
             devices. Close one of them."
        }
        Reason::TooManyFrames => {
            "Over the websocket only (`DOC_REFUSED`, `DOC_LEFT`). The connection sent the \
             rooms more than it may: as many frames a second as the setting \
             `collab_frames_per_second` says, and as many at once after a pause, a \
             `JOIN_DOC` and a sync step 1 counting as five. A `JOIN_DOC` past it is \
             refused; a frame past it puts the connection out of the room the frame was \
             for, since the room no longer has what the connection sent. Send less often, \
             and come in again."
        }
        other => unreachable!("{other:?} is no reason of the rooms"),
    }
}

/// The descriptions of the reasons of signing in through a provider.
fn describe_sign_in(reason: Reason) -> Option<&'static str> {
    Some(match reason {
        Reason::EmailUnverified => {
            "Status 403 from a login whose password is right, on a server that asks a \
             registration to confirm its email address (`email_verification`): the \
             registration has not been confirmed, and there is no account until the link \
             mailed to the address confirms it, within seven days. A wrong password is \
             answered as for no account."
        }
        Reason::PasswordRequired => {
            "Status 403 from `completeEmailVerification`, for a link that would confirm a \
             registration, opened in another browser than the one it was made from: send \
             the request again with `password`, the password the registration was made \
             with. The link stays as it was. Only who registered finishes a registration, \
             so that nobody is signed in to an account whose password somebody else \
             chose; `errors` names `password`."
        }
        Reason::SecretKeyRequired => {
            "Status 409. The server seals the client secret of a provider with its key, \
             `FVOCI_SECRET_KEY`, and has none: a provider cannot be stored until the server is \
             started with one."
        }
        Reason::OidcIssuerUnverified => {
            "Status 400. The discovery document of the issuer could not be read, or does not \
             name the issuer exactly, or its endpoints are not `https`. The server reaches \
             only `https` addresses, follows no redirect, and reaches only public addresses \
             unless its configuration lets it reach others. `errors` names `/issuer`; \
             `detail` says which of these it was, and nothing of the provider's answer."
        }
        Reason::OidcUnavailable => OIDC_LOGIN_PAGE_UNAVAILABLE,
        Reason::OidcStateInvalid => {
            "On the login page, after a sign-in through a provider. What came back is no \
             sign-in this browser started, or one that was used already or took longer than \
             ten minutes, or it came back to the address of another organization than the one \
             it was started for, or from another issuer (`iss`); or it was started to link \
             and the session it was started from has ended, or is not the one the browser \
             came back with. Start again from the login page."
        }
        Reason::OidcDenied => {
            "On the login page, after a sign-in through a provider: the provider sent the \
             person back without letting them sign in."
        }
        Reason::OidcTokenRefused => {
            "On the login page, after a sign-in through a provider: the provider did not \
             give an ID token for the code, or could not be reached for it."
        }
        Reason::OidcTokenInvalid => {
            "On the login page, after a sign-in through a provider: its ID token does not \
             hold (signature, algorithm, key, issuer, audience, time, nonce). The server's \
             log says which."
        }
        Reason::OidcEmailMissing => {
            "On the login page, after a sign-in through a provider: the person has no \
             account yet, and the provider gave no email address to make one with; or the \
             sign-in was to link, and the provider gave none to know the account by."
        }
        Reason::OidcEmailUnverified => {
            "On the login page, after a sign-in through a provider: the person has no \
             account yet, or the sign-in was to link, and the provider says it has not \
             checked their email address."
        }
        Reason::OidcEmailTaken => {
            "On the login page, after a sign-in through a provider: the person has no \
             account through the provider, and an account with a password has the email \
             address the provider gave as its own. The two are not joined by their \
             address: sign in with the password of that account, and link the provider to \
             it there (`startOidcLogin` with `linkPassword`); after that the provider \
             signs in to the same account. An address a provider gave for another account \
             is no account's own, and refuses nobody."
        }
        Reason::OidcJoinClosed => {
            "On the login page, after a sign-in through a provider: the person has no place \
             in the organization (no member of it, nor a guest of one of its workspaces), \
             and the organization does not let newcomers in through its provider."
        }
        other => return describe_linking_and_enforcing(other),
    })
}

/// The descriptions of the reasons of linking a provider to an account, and of an
/// organization that takes no password.
fn describe_linking_and_enforcing(reason: Reason) -> Option<&'static str> {
    Some(match reason {
        Reason::OidcIdentityTaken => {
            "On the login page, after a sign-in through a provider that was started to \
             link (`startOidcLogin` with `linkPassword`): who the provider vouches for is \
             another account's already. Nothing is linked, and the two accounts are not \
             joined."
        }
        Reason::OidcTrialAccount => {
            "On the organization's page, after a trial of its provider's pending settings \
             (`startOidcTrial`): the identity the provider vouched for is not the \
             administrator's who started the trial (it is another account's, or a deleted \
             account's, or tied to none where the settings are of the same issuer and so \
             tie nothing), or that administrator administers the organization no more. Nothing is \
             changed, no account is made and no identity is tied: settings no \
             administrator signs in through are not put in use."
        }
        Reason::OidcTrialStale => {
            "On the organization's page, after a trial of its provider's pending settings \
             (`startOidcTrial`) that are of another issuer: the identity that issuer \
             vouched for is tied to no account yet, and the trial ties it to the \
             administrator's own only when it was started from a session made through \
             the organization's provider, as it is stored, and the browser came back \
             with that session no more than five minutes after it was made. Who only \
             holds an older session, and does not sign in through the provider, ties \
             nothing. Nothing is changed. Sign in through the organization's provider \
             again (`startOidcLogin`), and finish the trial within five minutes \
             (`bindsIdentityUntil` of the pending settings says until when)."
        }
        Reason::OidcCarryOverRefused => {
            "Status 409 at `startOidcCarryOver` and `finishHeldOidcLogin`, and on the login \
             page after a sign-in through the organization's former provider. While the \
             provider an organization moved away from is kept (`former` of its provider), \
             a sign-in of somebody who has no account is held for the browser for ten \
             minutes, to be carried over to the account they had (by signing in through \
             the former provider once) or to end in a new account. This is the one answer \
             to everything that keeps that from happening: no sign-in is held for this \
             browser, or it ended or was taken already; the former provider is kept no \
             more; the provider changed meanwhile; the former provider vouched for nobody \
             who has an account with a place in the organization (a member, or a guest of \
             one of its workspaces in use); or the held identity \
             was tied to another account meanwhile. Nothing is changed: no identity is \
             tied and no session made. Start again from the login page."
        }
        Reason::OidcEmailMismatch => {
            "On the login page, after a sign-in through a provider that was started to \
             link (`startOidcLogin` with `linkPassword`): the email address the provider \
             vouches for is not the account's own. Nothing is linked: the account's \
             password ties to it only the person the provider knows by the account's \
             address. A provider that gave no address, or one it has not checked, is \
             refused as at a first sign-in (`oidc_email_missing`, \
             `oidc_email_unverified`)."
        }
        Reason::PasswordNotSet => {
            "Status 409, linking a provider to the account or removing an identity from \
             it: both ask for the account's password, and the account has none (it was \
             made by a sign-in through a provider)."
        }
        Reason::LastSignIn => {
            "Status 409, removing an identity from the account (`unlinkIdentity`): the \
             account has no password and no other identity, so nothing else would let \
             its owner in."
        }
        Reason::SsoRequired => {
            "Status 401, with a session that is good: the organization takes no password \
             (`ssoRequired` of its provider), and the session was not made through its \
             provider. Everything of the organization answers so (the routes under \
             `/w/{workspaceId}` and `/org/{orgId}`, and `getMyMemberships` and \
             `setUserAdmin`, which name neither and are of the organization the server \
             stands in, and `acceptInvitation`, for a link into it, member or not), from \
             the request after it was switched on; the session \
             stays, and opens what is of other organizations. `orgId` names the \
             organization: start a sign-in through its provider (`startOidcLogin`) and the \
             session it ends in opens it. `getMyWorkspaces` says beforehand which \
             workspaces are closed so. A board's public link is read without a session and \
             is not closed."
        }
        Reason::SsoUnproven => {
            "Status 409, setting an organization's provider. Taking no password \
             (`ssoRequired`) is switched on by a session that was made through that very \
             provider, switched on: who asks has then shown that it lets them in. Sign in \
             through the provider and send the request again."
        }
        Reason::SsoEnforced => {
            "Status 409, setting or removing an organization's provider. While the \
             organization takes no password (`ssoRequired`), its provider is not switched \
             off, removed, or set to another issuer or client: nobody would get in. \
             Switch `ssoRequired` off first. A refused setting names in `errors` what of \
             it was refused; a refused removal names nothing. The client secret can \
             still be set, and a wrong one lets nobody in either. If nobody gets in through the provider any more, who runs the \
             server releases the organization on its administration socket \
             (`POST /api/v1/admin/orgs/{orgId}/sso/release`)."
        }
        _ => return None,
    })
}

/// How long a cache may keep a page: the pages change with a release of the server.
const PAGE_CACHE_CONTROL: &str = "public, max-age=3600";

/// A page of plain text that anyone may read and a cache may keep for a while.
fn text_page(text: String) -> Response {
    (
        [
            (
                CONTENT_TYPE,
                HeaderValue::from_static("text/plain; charset=utf-8"),
            ),
            (X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff")),
            (CACHE_CONTROL, HeaderValue::from_static(PAGE_CACHE_CONTROL)),
        ],
        text,
    )
        .into_response()
}

/// The path the server is served under, from the address a page of this module was
/// asked at: what comes before [`PROBLEMS_PATH`].
fn base_path_of(asked: &Uri) -> &str {
    let path = asked.path();
    path.rfind(PROBLEMS_PATH).map_or("", |at| &path[..at])
}

/// The routes of the pages: the list at [`PROBLEMS_PATH`] and a page for each kind
/// below it. A name that is no text is refused as a problem, like any other error.
pub(crate) fn routes<S: Clone + Send + Sync + 'static>() -> Router<S> {
    Router::new()
        .route(PROBLEMS_PATH, get(handle_problems))
        .route(&format!("{PROBLEMS_PATH}/{{name}}"), get(handle_problem))
        .layer(middleware::from_fn(problems_only))
}

/// The page about the kind of problem `name`: what the `type` of a problem points at.
async fn handle_problem(
    OriginalUri(asked): OriginalUri,
    Path(name): Path<String>,
) -> Result<Response, AppError> {
    let problem = find(&name).ok_or_else(|| AppError::not_found("problem type"))?;
    Ok(text_page(problem.page(base_path_of(&asked))))
}

/// The list of every kind of problem, a line for each: its `type` and its title.
async fn handle_problems(OriginalUri(asked): OriginalUri) -> Response {
    let base_path = base_path_of(&asked);
    let mut page =
        "The kinds of problem this API answers with (RFC 9457). Each is described at its type.\n\n"
            .to_owned();
    for problem in all() {
        // Writing to a string cannot fail.
        let _ = writeln!(
            page,
            "{base_path}{}  {}",
            problem.type_reference(),
            problem.title
        );
    }
    text_page(page)
}

#[cfg(test)]
mod tests {
    use super::{all, find};
    use crate::error::{ErrorKind, Reason};

    #[test]
    fn every_kind_and_every_reason_is_a_problem_type_with_a_name_of_its_own() {
        let problems = all();
        assert_eq!(
            problems.len(),
            ErrorKind::ALL.len() + Reason::ALL.len(),
            "one for each"
        );
        for problem in &problems {
            assert_eq!(
                find(&problem.name).as_ref(),
                Some(problem),
                "found by its name, and the name is no other's"
            );
            assert!(
                problem
                    .name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '-'),
                "a name is lower-case words with hyphens: {}",
                problem.name
            );
            assert!(!problem.title.is_empty() && !problem.description.is_empty());
        }
        assert_eq!(find("nothing"), None);
    }

    #[test]
    fn the_page_of_a_kind_says_its_type_title_and_what_it_means() {
        let page = find("last-admin").expect("a kind").page("");
        assert!(
            page.starts_with("Last administrator\n\ntype: /problems/last-admin\n"),
            "{page}"
        );
        assert!(page.contains("reason: last_admin\n"), "{page}");
        assert!(
            page.ends_with("\n\nStatus 409. The change would leave the board, the workspace or the server without an administrator. Make somebody else one first.\n"),
            "{page}"
        );
        let page = find("not-found").expect("a kind").page("/base");
        assert!(
            page.contains("type: /base/problems/not-found\nstatus: 404\n"),
            "{page}"
        );
        assert!(!page.contains("reason:"), "{page}");
    }
}
