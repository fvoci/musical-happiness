//! The error type of the application and API layers.
//!
//! Every error answer of the API has one body, the [`Problem`] of RFC 9457, sent as
//! `application/problem+json`: a `type` that names the kind of problem and is the same
//! for every problem of the kind, its `title`, the `status`, a `detail` for developers,
//! and where a client needs them a `reason`, what of the request was refused (`errors`)
//! or the exceeded `limit`.
//!
//! Here is what an error is and what its body says. The answer it becomes, with its
//! status line and headers, is made in `error_response`: this module knows a status
//! as a number and nothing else of HTTP.

use fvoci_model::{
    AuthParamError, BlockError, BoardError, CategoryError, InvalidInput, Problem, Refusal,
    Refusals, SubscriptionError, json_pointer,
};
use fvoci_store::StoreError;

/// Where the server describes its kinds of problem: `/problems/<name>` is the `type`
/// of a problem, and the path of the page about it.
pub const PROBLEMS_PATH: &str = "/problems";

/// What kind of error a request ended with. The HTTP status follows from it, and the
/// `type` of the problem where the error has no [`Reason`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// The request is malformed or asks for something invalid: 400.
    BadRequest,
    /// The request carries no valid session, or the login failed: 401.
    Unauthorized,
    /// The user can see the thing but may not do this with it: 403.
    Forbidden,
    /// The thing does not exist, or the user may not know that it does: 404.
    NotFound,
    /// The path exists but not with this method: 405.
    MethodNotAllowed,
    /// The request is fine but the state is against it: 409.
    Conflict,
    /// A condition the request set on what it asks for does not hold: 412.
    PreconditionFailed,
    /// The request body is larger than allowed: 413.
    PayloadTooLarge,
    /// The range the request asks for is not in what it asks for: 416.
    RangeNotSatisfiable,
    /// Too many attempts; try again later: 429.
    RateLimited,
    /// The server's own fault: 500. The cause is logged and not sent to the client.
    Internal,
    /// The server cannot serve for now, through no fault of the request's: 503.
    ServiceUnavailable,
}

impl ErrorKind {
    /// The HTTP status of this kind of error.
    pub fn status(self) -> u16 {
        match self {
            Self::BadRequest => 400,
            Self::Unauthorized => 401,
            Self::Forbidden => 403,
            Self::NotFound => 404,
            Self::MethodNotAllowed => 405,
            Self::Conflict => 409,
            Self::PreconditionFailed => 412,
            Self::PayloadTooLarge => 413,
            Self::RangeNotSatisfiable => 416,
            Self::RateLimited => 429,
            Self::Internal => 500,
            Self::ServiceUnavailable => 503,
        }
    }

    /// Every kind, in the order of the statuses.
    pub const ALL: [Self; 12] = [
        Self::BadRequest,
        Self::Unauthorized,
        Self::Forbidden,
        Self::NotFound,
        Self::MethodNotAllowed,
        Self::Conflict,
        Self::PreconditionFailed,
        Self::PayloadTooLarge,
        Self::RangeNotSatisfiable,
        Self::RateLimited,
        Self::Internal,
        Self::ServiceUnavailable,
    ];

    /// The kind of error a status is of; `None` for a status the API does not answer
    /// with.
    pub fn of_status(status: u16) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.status() == status)
    }

    /// The name of the problem an error of this kind is when it has no [`Reason`]: the
    /// last part of its `type`.
    pub fn name(self) -> &'static str {
        match self {
            Self::BadRequest => "bad-request",
            Self::Unauthorized => "unauthorized",
            Self::Forbidden => "forbidden",
            Self::NotFound => "not-found",
            Self::MethodNotAllowed => "method-not-allowed",
            Self::Conflict => "conflict",
            Self::PreconditionFailed => "precondition-failed",
            Self::PayloadTooLarge => "payload-too-large",
            Self::RangeNotSatisfiable => "range-not-satisfiable",
            Self::RateLimited => "rate-limited",
            Self::Internal => "internal",
            Self::ServiceUnavailable => "service-unavailable",
        }
    }

    /// The `title` of that problem.
    pub fn title(self) -> &'static str {
        match self {
            Self::BadRequest => "Bad request",
            Self::Unauthorized => "No valid session",
            Self::Forbidden => "Forbidden",
            Self::NotFound => "Not found",
            Self::MethodNotAllowed => "Method not allowed",
            Self::Conflict => "Conflict",
            Self::PreconditionFailed => "Precondition failed",
            Self::PayloadTooLarge => "Payload too large",
            Self::RangeNotSatisfiable => "Range not satisfiable",
            Self::RateLimited => "Too many requests",
            Self::Internal => "Internal server error",
            Self::ServiceUnavailable => "Service unavailable",
        }
    }
}

/// Defines [`Reason`] with, for each variant, the value of the `reason` member and the
/// `title` of its problem, and the list of every variant made from the same lines: a
/// reason cannot be there without being listed and titled.
macro_rules! reasons {
    ($($(#[$doc:meta])* $variant:ident => $value:literal, $title:literal;)+) => {
        /// Why a request was refused, where clients must tell causes of one kind apart.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum Reason {
            $($(#[$doc])* $variant,)+
        }

        impl Reason {
            /// Every reason, in the order they are defined in.
            pub const ALL: &'static [Self] = &[$(Self::$variant,)+];

            /// The value of the body's `reason` member.
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $value,)+
                }
            }

            /// The `title` of the problem an error with this reason is.
            pub fn title(self) -> &'static str {
                match self {
                    $(Self::$variant => $title,)+
                }
            }
        }
    };
}

reasons! {
    /// The request lacks the header of the cross-site request check.
    Csrf => "csrf", "Cross-site request check failed";
    /// The request, or the websocket handshake, comes from a page of another origin.
    ForeignOrigin => "foreign_origin", "Request from another origin";
    /// The user name, email address or password of a login is wrong.
    InvalidCredentials => "invalid_credentials", "Invalid credentials";
    /// The server has its organization: who is not its first user gets in by an invitation.
    InvitationRequired => "invitation_required", "Invitation required";
    /// The link of an invitation lets nobody in: there is none, or it ended.
    InvitationUnavailable => "invitation_unavailable", "Invitation not available";
    /// The user name is taken.
    UsernameTaken => "username_taken", "User name taken";
    /// The email address is taken.
    EmailTaken => "email_taken", "Email address taken";
    /// The change would leave the board, the workspace or the instance without an
    /// administrator.
    LastAdmin => "last_admin", "Last administrator";
    /// Only an administrator of the instance may do this.
    AdminRequired => "admin_required", "Administrator required";
    /// A guest of the workspace is given no role on a board above editor.
    GuestRoleLimit => "guest_role_limit", "A guest is no more than an editor";
    /// A temporary organization administrator cannot add themselves to a team.
    TeamSelfJoining => "team_self_joining", "Cannot add yourself to a team during administrative joining";
    /// Public sharing is turned off in the server's configuration.
    SharingDisabled => "sharing_disabled", "Public sharing turned off";
    /// The user has as many uploads in parts open as one may.
    TooManyUploads => "too_many_uploads", "Too many open uploads";
    /// The user's open uploads in parts hold back as many bytes as one user's may.
    UploadsPending => "uploads_pending", "Open uploads hold too much";
    /// The open uploads in parts of the whole server hold back as many bytes as they may.
    ServerUploadsPending => "server_uploads_pending", "The server's open uploads hold too much";
    /// The file is larger than one request takes; it is to be sent in parts.
    UploadInParts => "upload_in_parts", "File to be uploaded in parts";
    /// The user has an export running.
    ExportRunning => "export_running", "An export is running";
    /// The server runs as many exports as it may.
    ServerExportsBusy => "server_exports_busy", "The server is busy exporting";
    /// The user has an import running.
    ImportRunning => "import_running", "An import is running";
    /// The server runs as many imports as it may.
    ServerImportsBusy => "server_imports_busy", "The server is busy importing";
    /// The body's document does not take the update.
    UpdateRefused => "update_refused", "Update refused";
    /// The update builds on changes the body's document does not have.
    UpdateOutOfStep => "update_out_of_step", "Update out of step";
    /// The update is larger than the server takes.
    UpdateTooLarge => "update_too_large", "Update too large";
    /// With the update the body's document, or its body, would be larger than it may be.
    DocumentTooLarge => "document_too_large", "Document too large";
    /// The server holds as many documents of bodies being edited as it may.
    TooManyDocuments => "too_many_documents", "Too many documents open";
    /// The body's document cannot be worked on for now.
    DocumentUnavailable => "document_unavailable", "Document unavailable";
    /// The connection reads the body's document and does not write it.
    DocumentReadOnly => "document_read_only", "Document read only";
    /// The websocket connection does not read what it is sent of the body's document.
    ConnectionTooSlow => "connection_too_slow", "Connection too slow";
    /// The websocket connection is in as many rooms of bodies as one may be.
    TooManyDocumentsJoined => "too_many_documents_joined", "Connection in too many documents";
    /// As many websocket connections are in the room of the body as may be.
    DocumentFull => "document_full", "Document full";
    /// As many of the user's websocket connections are in the room of the body as may be.
    TooManyConnectionsJoined => "too_many_connections_joined", "Too many connections of one user";
    /// The websocket connection sends more of the rooms in a second than it may.
    TooManyFrames => "too_many_frames", "Too many frames";
    /// The server has no `secret_key` to seal a provider's client secret with.
    SecretKeyRequired => "secret_key_required", "Secret key required";
    /// The issuer of a provider could not be read and checked.
    OidcIssuerUnverified => "oidc_issuer_unverified", "Provider not verified";
    /// The organization has no provider that is on, or it could not be reached.
    OidcUnavailable => "oidc_unavailable", "Sign-in through the provider not available";
    /// A sign-in came back that is not one this browser started, was used or ended.
    OidcStateInvalid => "oidc_state_invalid", "Sign-in not recognized";
    /// The provider did not let the person sign in.
    OidcDenied => "oidc_denied", "Sign-in refused by the provider";
    /// The provider gave no ID token for the sign-in.
    OidcTokenRefused => "oidc_token_refused", "No token from the provider";
    /// The ID token of the provider does not hold.
    OidcTokenInvalid => "oidc_token_invalid", "Token of the provider not valid";
    /// The provider gave no email address for a person who has no account yet.
    OidcEmailMissing => "oidc_email_missing", "No email address from the provider";
    /// The provider says it has not checked the person's email address.
    OidcEmailUnverified => "oidc_email_unverified", "Email address not verified";
    /// Another account has the email address the provider gave.
    OidcEmailTaken => "oidc_email_taken", "Email address of another account";
    /// The organization does not let in who is no member through its provider.
    OidcJoinClosed => "oidc_join_closed", "Organization closed to newcomers";
    /// The identity a provider vouches for is another account's: it is not linked.
    OidcIdentityTaken => "oidc_identity_taken", "Sign-in of another account";
    /// The email address a provider vouches for is not the one of the account it was
    /// to be linked to: it is not linked.
    OidcEmailMismatch => "oidc_email_mismatch", "Email address of the provider not the account's";
    /// A trial of a provider's pending settings ended in another account than the one
    /// of the administrator who started it: nothing is changed.
    OidcTrialAccount => "oidc_trial_account", "Trial in another account";
    /// Nothing was carried over to the organization's provider: no sign-in is held for
    /// the browser, the former provider is kept no more, or it vouched for nobody who
    /// is a member with an account to carry over.
    OidcCarryOverRefused => "oidc_carry_over_refused", "Account not carried over";
    /// The account has no password to confirm with.
    PasswordNotSet => "password_not_set", "Account without a password";
    /// The identity is the only way in to an account without a password.
    LastSignIn => "last_sign_in", "Last way to sign in";
    /// The organization takes only sessions made through its provider, and this one
    /// was not.
    SsoRequired => "sso_required", "Sign-in through the provider required";
    /// Taking no password is switched on from a session made through the provider.
    SsoUnproven => "sso_unproven", "Provider not signed in through";
    /// The organization takes no password, and its provider stays as it is meanwhile.
    SsoEnforced => "sso_enforced", "Provider held while no password is taken";
    /// The value of a card relation is neither `null` nor a list of identifiers.
    RelationNotAList => "relation_not_a_list", "Card relation that is no list";
    /// A card relation names the card it is the value of.
    RelationToItself => "relation_to_itself", "Card relation to the card itself";
    /// A card relation names a card twice.
    RelationRepeated => "relation_repeated", "Card named twice in a relation";
    /// A card relation names more cards than one may.
    RelationTooMany => "relation_too_many", "Card relation names too many cards";
    /// A card relation names a block of the board that is no card.
    RelationNotACard => "relation_not_a_card", "Card relation names no card";
    /// A card relation names a card that is not on the board.
    RelationCardNotFound => "relation_card_not_found", "Card relation names a card not on the board";
    /// Another workspace in use of the organization has the name.
    WorkspaceNameTaken => "workspace_name_taken", "Workspace name taken";
    /// Another team of the workspace has the name, in some letter case.
    TeamNameTaken => "team_name_taken", "Team name taken";
    /// The organization's default workspace is not deleted.
    DefaultWorkspace => "default_workspace", "Default workspace";
    /// The change would leave the organization without an administrator.
    LastOrgAdmin => "last_org_admin", "Last administrator of the organization";
    /// The organization has turned its public links off.
    PublicLinksOff => "public_links_off", "Public links turned off";
    /// An invitation mail request names more recipients than one may.
    TooManyRecipients => "too_many_recipients", "Too many recipients";
    /// The workspace queued as many invitation messages within the hour as it may.
    InvitationMailLimit => "invitation_mail_limit", "Hourly invitation mail limit reached";
    /// The server holds as many invitation messages waiting to be sent as it may.
    MailQueueFull => "mail_queue_full", "Mail queue full";
}

impl Reason {
    /// The name of the problem an error with this reason is, the last part of its
    /// `type`: the reason, written with hyphens.
    pub fn name(self) -> String {
        self.as_str().replace('_', "-")
    }
}

/// The `type` of the problems of the kind `name`, on a server at the root of its host:
/// the path of the page about the kind. A server under a base path has the base path
/// before it, which the request log adds.
pub fn type_reference(name: &str) -> String {
    format!("{PROBLEMS_PATH}/{name}")
}

/// The text every server error is answered with; what went wrong is in the log.
const INTERNAL_MESSAGE: &str = "internal server error";

/// The `reason` of what was refused when neither the error nor the check that refused
/// it tells a finer one.
const INVALID: &str = "invalid";

/// The most entries an answer lists of what a check refused: a request can be made to
/// have thousands of members that are wrong, and each entry repeats a name of the
/// client's own.
const MAX_ENTRIES: usize = 100;

/// Where in a request something that was refused is.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Place {
    /// In the body, at this JSON Pointer.
    Body(String),
    /// The parameter of this name, of the query or of the path.
    Parameter(String),
    /// The header of this name.
    Header(String),
}

/// What the store refused the members of.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Subject {
    /// The block with this identifier.
    Block(String),
    /// A board; the store does not say which.
    Board,
}

/// One thing of a request that was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Refused {
    place: Place,
    /// What is wrong with it; the message of the error if `None`.
    detail: Option<String>,
    /// Why; the reason of the error, or [`INVALID`], if `None`.
    reason: Option<&'static str>,
}

/// An error of a request.
///
/// The message of every kind except [`ErrorKind::Internal`] is sent to the client.
#[derive(Debug)]
pub struct AppError {
    kind: ErrorKind,
    message: String,
    reason: Option<Reason>,
    /// What of the request was refused, where the error is about something of it.
    refused: Vec<Refused>,
    /// What the refused members are of, where the store refused a block or a board: a
    /// request with a list of them says with it which one, or that it cannot say
    /// ([`AppError::among_blocks`]). Never sent.
    subject: Option<Subject>,
    /// What few errors say besides: boxed, so that an error stays small to hand back.
    more: Option<Box<More>>,
    /// The cause of a server error, for the log.
    cause: Option<anyhow::Error>,
}

/// What few errors say besides their words and their reason.
#[derive(Debug, Default)]
struct More {
    /// The limit that was exceeded.
    limit: Option<u64>,
    /// How many seconds to wait before trying again.
    retry_after: Option<u64>,
    /// The organization to sign in to, where that is what the error asks for.
    org_id: Option<String>,
}

impl std::fmt::Display for AppError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.cause {
            Some(cause) => write!(formatter, "{cause:#}"),
            None => formatter.write_str(&self.message),
        }
    }
}

impl std::error::Error for AppError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.cause.as_ref().map(|cause| {
            let cause: &(dyn std::error::Error + Send + Sync + 'static) = cause.as_ref();
            let cause: &(dyn std::error::Error + 'static) = cause;
            cause
        })
    }
}

impl AppError {
    fn new(kind: ErrorKind, message: impl std::fmt::Display) -> Self {
        Self {
            kind,
            message: message.to_string(),
            reason: None,
            refused: Vec::new(),
            subject: None,
            more: None,
            cause: None,
        }
    }

    /// What few errors say besides, to be set.
    fn more(&mut self) -> &mut More {
        self.more.get_or_insert_default()
    }

    /// An error of the kind `kind`, for an answer that no code of this server made:
    /// `words` is what it tells the client, but for a server error, which says what
    /// every server error says.
    pub(crate) fn of_kind(kind: ErrorKind, words: &str) -> Self {
        Self::new(
            kind,
            if kind == ErrorKind::Internal {
                INTERNAL_MESSAGE
            } else {
                words
            },
        )
    }

    /// A 503 error: the server cannot serve for now. `message` is sent to the client: it
    /// says what is not there in words of the server's, never a cause.
    pub fn service_unavailable(message: &'static str) -> Self {
        Self::new(ErrorKind::ServiceUnavailable, message)
    }

    /// A 429 error: too many attempts, try again after `wait`. The wait is told in
    /// whole seconds, rounded up, in the `Retry-After` header and in the body.
    pub fn rate_limited(wait: std::time::Duration) -> Self {
        let seconds = wait.as_secs() + u64::from(wait.subsec_nanos() > 0);
        let mut error = Self::new(ErrorKind::RateLimited, "too many attempts; try again later");
        error.more().retry_after = Some(seconds.max(1));
        error
    }

    /// A 429 error for a quota of what is done within a time: `message` says which, and
    /// `wait` is told as [`AppError::rate_limited`] tells it.
    pub fn quota_reached(
        message: impl std::fmt::Display,
        reason: Reason,
        wait: std::time::Duration,
    ) -> Self {
        let mut error = Self::rate_limited(wait).with_reason(reason);
        error.message = message.to_string();
        error
    }

    /// A 429 error for a limit on what is held at once, rather than on attempts: no
    /// wait is told, since what frees room is the client's own doing or another's.
    pub fn limited(message: impl std::fmt::Display, reason: Reason) -> Self {
        Self::new(ErrorKind::RateLimited, message).with_reason(reason)
    }

    /// A 400 error with the given message.
    pub fn bad_request(message: impl std::fmt::Display) -> Self {
        Self::new(ErrorKind::BadRequest, message)
    }

    /// A 401 error with the given message.
    pub fn unauthorized(message: impl std::fmt::Display) -> Self {
        Self::new(ErrorKind::Unauthorized, message)
    }

    /// The 401 of a session that is good and does not open what is the organization
    /// `org_id`'s: that one takes only sessions made through its provider. The answer
    /// names the organization, which the sign-in that opens it is started for.
    pub fn sso_required(org_id: &str) -> Self {
        let mut error = Self::new(
            ErrorKind::Unauthorized,
            "sign in through the organization's provider to open this",
        )
        .with_reason(Reason::SsoRequired);
        error.more().org_id = Some(org_id.to_owned());
        error
    }

    /// A 403 error with the given message.
    pub fn forbidden(message: impl std::fmt::Display) -> Self {
        Self::new(ErrorKind::Forbidden, message)
    }

    /// A 404 error for a kind of thing, such as `board`: `board not found`.
    ///
    /// The message names the kind and nothing else, so that it is the same for a thing
    /// that does not exist and for one the requester may not know about.
    pub fn not_found(what: impl std::fmt::Display) -> Self {
        Self::new(ErrorKind::NotFound, format_args!("{what} not found"))
    }

    /// The 403 error of a registration on a server that has its organization: who is not its
    /// first user gets in by an invitation.
    pub fn invitation_required() -> Self {
        Self::forbidden("this server has its organization; an invitation lets you in")
            .with_reason(Reason::InvitationRequired)
    }

    /// The 404 error of a link that lets nobody in, whatever is wrong with it: no such
    /// link, one that ended, one that was used up, one that was revoked.
    pub fn invitation_unavailable() -> Self {
        Self::not_found("invitation").with_reason(Reason::InvitationUnavailable)
    }

    /// A 405 error.
    pub fn method_not_allowed() -> Self {
        Self::new(ErrorKind::MethodNotAllowed, "method not allowed")
    }

    /// A 409 error with the given message.
    pub fn conflict(message: impl std::fmt::Display) -> Self {
        Self::new(ErrorKind::Conflict, message)
    }

    /// A 412 error with the given message.
    pub fn precondition_failed(message: impl std::fmt::Display) -> Self {
        Self::new(ErrorKind::PreconditionFailed, message)
    }

    /// A 416 error with the given message.
    pub fn range_not_satisfiable(message: impl std::fmt::Display) -> Self {
        Self::new(ErrorKind::RangeNotSatisfiable, message)
    }

    /// A 413 error that names the limit, in bytes, that the request body exceeds.
    pub fn payload_too_large(limit: u64) -> Self {
        let mut error = Self::new(
            ErrorKind::PayloadTooLarge,
            format_args!("the request body is larger than {limit} bytes"),
        );
        error.more().limit = Some(limit);
        error
    }

    /// A 413 error for what the request names, not its body, being over `limit`: an
    /// archive's entries, for one.
    pub fn too_large(message: impl std::fmt::Display, limit: u64) -> Self {
        let mut error = Self::new(ErrorKind::PayloadTooLarge, message);
        error.more().limit = Some(limit);
        error
    }

    /// A 500 error with the given cause, which is logged and not sent to the client.
    pub fn internal(cause: impl std::fmt::Display) -> Self {
        Self::from(anyhow::anyhow!("{cause}"))
    }

    /// Adds the stable reason a client can act on.
    #[must_use]
    pub fn with_reason(mut self, reason: Reason) -> Self {
        self.reason = Some(reason);
        self
    }

    /// Names the member of the request body that was refused, by its name: a member of
    /// the object the body is.
    #[must_use]
    pub fn with_field(self, field: &str) -> Self {
        self.with_pointer(json_pointer(field))
    }

    /// Names what of the request body was refused by a JSON Pointer to it, for what is
    /// deeper than a member of the body: `/blocks/0/title`.
    #[must_use]
    pub fn with_pointer(self, pointer: impl Into<String>) -> Self {
        self.refusing(Place::Body(pointer.into()))
    }

    /// Names the parameter of the query or of the path that was refused.
    #[must_use]
    pub fn with_parameter(self, name: &str) -> Self {
        self.refusing(Place::Parameter(name.to_owned()))
    }

    /// Names the request header that was refused.
    #[must_use]
    pub fn with_header(self, name: &str) -> Self {
        self.refusing(Place::Header(name.to_owned()))
    }

    /// One error for what this one and `other` refuse, where a check looks at two
    /// things and has an error for each that names what it refuses: the entries of
    /// both, each with the words and the reason of its own error. The error is of this
    /// one's kind and says how many things were refused.
    #[must_use]
    pub fn and(self, other: Self) -> Self {
        debug_assert!(
            self.kind == other.kind && !self.refused.is_empty() && !other.refused.is_empty(),
            "two errors of one kind that each name what they refuse"
        );
        let kind = self.kind;
        let refused: Vec<Refused> = self.settled().chain(other.settled()).collect();
        let mut error = Self::new(
            kind,
            format_args!("{} things of the request were refused", refused.len()),
        );
        error.refused = refused;
        error
    }

    /// What the error refuses, each entry with the error's words and reason where it
    /// has none of its own.
    fn settled(self) -> impl Iterator<Item = Refused> {
        let reason = self.reason.map(Reason::as_str);
        let message = self.message;
        self.refused.into_iter().map(move |refused| Refused {
            detail: refused.detail.or_else(|| Some(message.clone())),
            reason: refused.reason.or(reason),
            ..refused
        })
    }

    /// Says that what was refused in the body is under `prefix`, a JSON Pointer: for a
    /// request that carries a list of what the error is about, `/blocks/2`.
    #[must_use]
    pub fn under(mut self, prefix: &str) -> Self {
        for refused in &mut self.refused {
            if let Place::Body(pointer) = &mut refused.place {
                pointer.insert_str(0, prefix);
            }
        }
        self
    }

    /// Says where in a request with a list of blocks the members are that the store
    /// refused of one of them: under `list` (a JSON Pointer, empty if the body is the
    /// list) at the place its identifier has among `ids`. A refusal of a block that is
    /// not among them, or of one of the request's boards, names no member: where it is
    /// cannot be said. Any other error is left as it is.
    #[must_use]
    pub fn among_blocks<'a>(mut self, ids: impl IntoIterator<Item = &'a str>, list: &str) -> Self {
        let index = match self.subject.take() {
            None => return self,
            Some(Subject::Block(block)) => ids.into_iter().position(|id| id == block),
            Some(Subject::Board) => None,
        };
        if let Some(index) = index {
            return self.under(&format!("{list}/{index}"));
        }
        self.refused
            .retain(|refused| !matches!(refused.place, Place::Body(_)));
        self
    }

    /// Says that what was refused is no member of the request body: for what is
    /// checked of something the request only names, as an archive is by its upload.
    /// The words of the error still say what is wrong.
    #[must_use]
    pub fn outside_the_request(mut self) -> Self {
        self.subject = None;
        self.refused
            .retain(|refused| !matches!(refused.place, Place::Body(_)));
        self
    }

    /// Says that what was refused at the JSON Pointer `from` is at `to` in the request:
    /// for a request that names a member otherwise than what it changes does, as a
    /// patch's `updatedProperties` are a board's `properties`.
    #[must_use]
    pub fn moved(mut self, from: &str, to: &str) -> Self {
        for refused in &mut self.refused {
            if refused.place == Place::Body(from.to_owned()) {
                refused.place = Place::Body(to.to_owned());
            }
        }
        self
    }

    fn refusing(mut self, place: Place) -> Self {
        self.refused.push(Refused {
            place,
            detail: None,
            reason: None,
        });
        self
    }

    /// Names the limit the request went over: bytes for a body, a count for a
    /// collection.
    #[must_use]
    pub fn with_limit(mut self, limit: u64) -> Self {
        self.more().limit = Some(limit);
        self
    }

    /// Says where the error came up, for the log. A server error carries it in its
    /// cause. What a client is told does not change: the words of the app layer about
    /// its own steps are not for clients.
    #[must_use]
    pub fn context(mut self, context: impl std::fmt::Display) -> Self {
        if let Some(cause) = self.cause.take() {
            self.cause = Some(cause.context(context.to_string()));
        } else {
            tracing::debug!(%context, error = %self.message, "API error");
        }
        self
    }

    /// The kind of this error.
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// The reason a client can act on, if the error has one.
    pub fn reason(&self) -> Option<Reason> {
        self.reason
    }

    /// Returns `true` if this is a 404 error.
    pub fn is_not_found(&self) -> bool {
        self.kind == ErrorKind::NotFound
    }

    /// Returns `true` if this error refuses the requester (401, 403 or 404) rather than
    /// failing: what an access check answers to a user who may not do what they ask.
    pub fn is_refusal(&self) -> bool {
        matches!(
            self.kind,
            ErrorKind::NotFound | ErrorKind::Forbidden | ErrorKind::Unauthorized
        )
    }

    /// The HTTP status this error is reported with.
    pub fn status(&self) -> u16 {
        self.kind.status()
    }

    /// Returns `true` if the request is at fault and not the server: a status from 400
    /// to 499.
    pub fn is_client_error(&self) -> bool {
        (400..500).contains(&self.status())
    }

    /// Writes the error to the log as it is answered: a server error with its cause,
    /// which the client is not told; any other with its status and its message, for
    /// who debugs.
    pub(crate) fn log(&self) {
        if let Some(cause) = &self.cause {
            tracing::error!(error = %format!("{cause:#}"), "API error");
        } else {
            tracing::debug!(status = self.status(), error = %self.message, "API error");
        }
    }

    /// The body this error is reported with, as it is before the request log has it:
    /// without the identifier of the request, and with its `type` as on a server at the
    /// root of its host.
    pub fn body(&self) -> Problem {
        let (name, title) = match self.reason {
            Some(reason) => (reason.name(), reason.title()),
            None => (self.kind.name().to_owned(), self.kind.title()),
        };
        let reason = self.reason.map(Reason::as_str);
        let errors = self
            .refused
            .iter()
            .map(|refused| {
                let mut input = InvalidInput {
                    detail: refused.detail.as_ref().unwrap_or(&self.message).clone(),
                    reason: refused.reason.or(reason).unwrap_or(INVALID).to_owned(),
                    ..InvalidInput::default()
                };
                match &refused.place {
                    Place::Body(pointer) => input.pointer = Some(pointer.clone()),
                    Place::Parameter(name) => input.parameter = Some(name.clone()),
                    Place::Header(name) => input.header = Some(name.clone()),
                }
                input
            })
            .collect();
        let more = self.more.as_deref();
        Problem {
            problem_type: type_reference(&name),
            title: title.to_owned(),
            status: self.status(),
            detail: self.message.clone(),
            instance: None,
            reason: reason.map(str::to_owned),
            errors,
            limit: more.and_then(|more| more.limit),
            retry_after: more.and_then(|more| more.retry_after),
            request_id: None,
            org_id: more.and_then(|more| more.org_id.clone()),
        }
    }
}

impl From<anyhow::Error> for AppError {
    fn from(cause: anyhow::Error) -> Self {
        Self {
            cause: Some(cause),
            ..Self::new(ErrorKind::Internal, INTERNAL_MESSAGE)
        }
    }
}

impl From<StoreError> for AppError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::TeamSelfJoining => {
                Self::forbidden(error).with_reason(Reason::TeamSelfJoining)
            }
            // What was not found is in the store's message, which names identifiers; the
            // answer does not repeat it. Routes say what kind of thing they miss.
            StoreError::NotFound(_) | StoreError::NotAllFound { .. } | StoreError::NoRows => {
                tracing::debug!(%error, "not found in the store");
                Self::new(ErrorKind::NotFound, "not found")
            }
            // More client state than one account keeps.
            StoreError::TooManyPreferences { limit } => Self::bad_request(error)
                .with_field("updatedFields")
                .with_limit(u64::try_from(limit).unwrap_or(u64::MAX)),
            // A registration that was not the first after all.
            StoreError::NotFirstUser => Self::invitation_required(),
            // The one lookup that names its kind: a password is set for a user.
            StoreError::UserNotFound(_) => {
                tracing::debug!(%error, "not found in the store");
                Self::not_found("user")
            }
            // What a client sent and the store refused.
            StoreError::InvalidBlock {
                source:
                    BlockError::TitleSizeLimitExceeded
                    | BlockError::FieldsSizeLimitExceeded
                    | BlockError::InvalidCardProperties(_),
                ..
            }
            | StoreError::InvalidBoard(
                BoardError::InvalidCardProperty(_)
                | BoardError::PropertyNamedTwice { .. }
                | BoardError::PropertyUnknown { .. }
                | BoardError::PropertiesLeftOut { .. },
            )
            | StoreError::InvalidBoardsAndBlocks(_)
            | StoreError::BlockNotInDeletedBoards { .. } => Self::bad_request(error),
            StoreError::InvalidCategory(refused) => Self::from(refused),
            StoreError::InvalidSubscription(refused) => Self::from(refused),
            StoreError::InvalidBoard(BoardError::NulCharacter { member }) => {
                let mut error = Self::bad_request(error).with_field(member);
                error.subject = Some(Subject::Board);
                error
            }
            // A parent that is no block of the board, whether it is none at all or
            // another board's: the answer does not tell the two apart.
            StoreError::InvalidBlock {
                id,
                source:
                    source @ (BlockError::ParentNotOnBoard
                    | BlockError::ParentBelowBlock
                    | BlockError::CommentNotOnCard
                    | BlockError::ParentIsComment
                    | BlockError::TypeStrandsComments
                    | BlockError::BodyNotAllowed
                    | BlockError::ImageWithoutFile
                    | BlockError::NulInTitle
                    | BlockError::InvalidCardField { .. }
                    | BlockError::InvalidRelation(_)
                    | BlockError::TypeNotCurrent(_)),
            } => {
                let mut error = Self::from(source);
                error.subject = Some(Subject::Block(id));
                error
            }
            // What a request names by something that is no identifier does not exist.
            // The text is not echoed: it is the client's own, and it may be long.
            StoreError::InvalidId(_) => {
                tracing::debug!(%error, "a write named something that is no identifier");
                Self::bad_request("an identifier in the request is not a valid identifier")
            }
            StoreError::Conflict(_) => Self::conflict(error),
            StoreError::NotRestorable { source, .. } => Self::conflict(format_args!(
                "the block cannot come back where it was: {source}"
            )),
            // The message names no times: the client has the time it read the block
            // at, and reads the block again for the one it has now.
            StoreError::Stale { .. } => {
                Self::conflict("the block has changed since it was read").context(error)
            }
            other => Self::from(anyhow::Error::from(other)),
        }
    }
}

impl From<Refusals> for AppError {
    /// A bad request that lists every member the check refused, each with its own
    /// words and cause.
    fn from(refusals: Refusals) -> Self {
        let mut error = if refusals.len() > MAX_ENTRIES {
            Self::bad_request(format_args!(
                "{refusals}; the first {MAX_ENTRIES} are listed"
            ))
        } else {
            Self::bad_request(&refusals)
        };
        error.refused = refusals
            .into_iter()
            .take(MAX_ENTRIES)
            .map(|refusal| Refused {
                place: Place::Body(refusal.pointer),
                detail: Some(refusal.detail),
                reason: Some(refusal.reason.as_str()),
            })
            .collect();
        error
    }
}

impl From<AuthParamError> for AppError {
    fn from(error: AuthParamError) -> Self {
        Self::from(Refusals::from(Refusal::from(error)))
    }
}

impl From<CategoryError> for AppError {
    fn from(error: CategoryError) -> Self {
        Self::from(error.into_refusals())
    }
}

impl From<SubscriptionError> for AppError {
    fn from(error: SubscriptionError) -> Self {
        Self::from(error.into_refusals())
    }
}

mod model_errors;
#[cfg(test)]
mod tests;
