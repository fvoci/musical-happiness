//! The contract of the API: the OpenAPI document of what the server takes and answers.
//!
//! The document is made from the routes themselves. Every handler carries a
//! `#[utoipa::path]` that names its path, its parameters, its bodies and its answers,
//! and the same attribute puts the handler on the router, so that a route cannot be
//! served without being in the document. [`document`] collects them and finishes what
//! the attributes cannot say: the head of the document, the answer every operation
//! gives when it refuses, and which members of an answer are always there.
//!
//! The document is committed as `contracts/openapi.json`; `cargo xtask openapi` fails
//! when the file is not what [`document`] makes.

pub(crate) mod bodies;
pub(crate) mod params;

use std::collections::BTreeSet;

use fvoci_model::{InvalidInput, PROBLEM_MEDIA_TYPE, Problem};
use serde_json::{Map, Value, json};
use utoipa::PartialSchema;
use utoipa_axum::router::OpenApiRouter;

use crate::app::App;
use crate::problems::Basis;

/// The names operations are grouped under.
pub(crate) mod tag {
    pub(crate) const SYSTEM: &str = "system";
    pub(crate) const AUTH: &str = "auth";
    pub(crate) const USERS: &str = "users";
    pub(crate) const WORKSPACES: &str = "workspaces";
    pub(crate) const CONTAINERS: &str = "boards";
    pub(crate) const WIKIS: &str = "wikis";
    pub(crate) const DIARIES: &str = "diaries";
    pub(crate) const MEMBERS: &str = "members";
    pub(crate) const SHARING: &str = "sharing";
    pub(crate) const BLOCKS: &str = "blocks";
    pub(crate) const CARDS: &str = "cards";
    pub(crate) const REFERENCES: &str = "references";
    pub(crate) const SIDEBAR: &str = "sidebar";
    pub(crate) const FILES: &str = "files";
    pub(crate) const ARCHIVES: &str = "archives";
    pub(crate) const SUBSCRIPTIONS: &str = "subscriptions";
    pub(crate) const ORGANIZATION: &str = "organization";
}

/// The tags in the order the document lists them, each with what it is about.
const TAGS: [(&str, &str); 17] = [
    (
        tag::SYSTEM,
        "Whether the server is there, and what a client must know before it signs in.",
    ),
    (
        tag::AUTH,
        "Signing in and out, registering, and changing the password.",
    ),
    (
        tag::USERS,
        "The user of the session, their settings, and other users.",
    ),
    (
        tag::WORKSPACES,
        "The workspaces a user is a member of, what is in a workspace, and its users.",
    ),
    (tag::CONTAINERS, "Boards and templates."),
    (
        tag::WIKIS,
        "Wikis and their pages. A wiki is a board of the kind `wiki`: who is on it, its \
         sharing, its files, its place in the sidebar and in the trash are asked as a \
         board's are, under `/containers/{containerId}` with the wiki's identifier.",
    ),
    (
        tag::DIARIES,
        "Diaries and their entries, in the workspace of a user's own space and nowhere \
         else. A diary is a board of the kind `diary` that holds one entry a day and \
         nothing else; its owner alone reads it. What it shares with a board is asked \
         under `/containers/{containerId}` with the diary's identifier: the diary itself \
         (`getContainer`), its title, description and icon (`patchContainer`), the trash \
         (`deleteContainer`, `undeleteContainer`, `getDeletedContainers`), its files and \
         their versions and leases, its place in the sidebar, and its archive \
         (`exportContainer`; the archive of the workspace holds its diaries, and an \
         archive that is imported makes each diary anew, in the user's own space only). \
         Its entries are found by `searchWorkspaceContent`, by their writer. What is about \
         the people of a board answers 404 for a diary, as for an identifier of nothing: \
         its members, joining and leaving it, its teams, its link (`/sharing`, \
         `/shared/{containerId}`), invitations to it, and `duplicateContainer`. So do the \
         operations under `/boards/{boardId}` and `/wikis/{wikiId}`: a diary has no \
         blocks but its entries, no cards, no views, no pages, no comments and no \
         templates; and no reference names an entry.",
    ),
    (tag::MEMBERS, "Who is on a board, and with which role."),
    (tag::SHARING, "Sharing a board with whoever has its link."),
    (
        tag::BLOCKS,
        "The blocks of a board: cards, views, comments, and what a card is made of.",
    ),
    (tag::CARDS, "The blocks of type `card`, as cards."),
    (
        tag::REFERENCES,
        "What the bodies of a workspace name: a body holds the identifier of a card or a \
         page (the node `reference`), and what that is called is told here, to each \
         user what they read.",
    ),
    (
        tag::SIDEBAR,
        "A user's categories and the boards filed under them.",
    ),
    (tag::FILES, "The files of a board."),
    (tag::ARCHIVES, "Exporting boards and importing them."),
    (tag::SUBSCRIPTIONS, "Subscriptions to blocks."),
    (
        tag::ORGANIZATION,
        "What an organization's administrators manage: the organization, its members' \
         roles, its workspaces and their trash, joining a workspace, and the audit logs.",
    ),
];

/// The name of the security scheme of the session cookie.
const SESSION_COOKIE: &str = "sessionCookie";
/// The name of the security scheme of the token in the `Authorization` header.
const BEARER_TOKEN: &str = "bearerToken";

/// What the head of the document says about the API as a whole.
const DESCRIPTION: &str = "\
The API of an fvoci server.

## Signing in

`login` answers with a `Set-Cookie` header that carries the session: the cookie \
`__Host-fvoci_session` where the server is reached over HTTPS, `fvoci_session` over plain \
HTTP. It is `HttpOnly`, so a page's scripts never see it, and a browser sends it by itself. \
A client that is no browser may send the cookie's value as `Authorization: Bearer <value>` \
instead. The operations under `/shared/{containerId}` need no session: they read a shared board with its \
`readToken`, and look at no session that comes along.

## Requests that change something

Every request whose method is not `GET`, `HEAD` or `OPTIONS` needs the header \
`X-Requested-With: XMLHttpRequest`, unless it has a bearer token, and is refused with 403 \
and the reason `csrf` without it. If it has an `Origin` header, the origin must be the \
server's own (or one the server was configured to accept); otherwise it is refused with 403 \
and the reason `foreign_origin`.

A page should make these requests with `fetch` in its default mode, `cors`. The server sends \
its pages with `Referrer-Policy: no-referrer`, and under that policy a browser writes \
`Origin: null` on a changing request of any other mode (`same-origin`, `no-cors`, or a form \
the browser posts itself), which is refused as a foreign origin.

## Answers

An operation answers 200 with a body, 201 with what it made, or 204 with no body. Every \
other status comes with a `Problem`, the problem details of RFC 9457, as \
`application/problem+json`: `type` names the kind of the problem and is the same for every \
problem of the kind, `title` is its name, `status` repeats the status and `detail` says what \
went wrong this time. `type` is `/problems/<name>` under the path the server is served at, \
the address of a page of text about the kind; `/problems` lists them, and so does \
`x-problem-types` of the schema `Problem`, as they are on a server at the root of its host \
(a server under a path has that path before each). `reason` tells apart causes a client must tell apart; a \
problem with a `reason` has the `type` of that name, written with hyphens, and any other the \
`type` of its status. `errors` lists what of the request was refused, each by a JSON Pointer \
into the body (`pointer`), the name of a parameter of the query or the path (`parameter`), or \
the name of a header (`header`). `limit` is the limit that was exceeded: bytes for a body \
that was too large (413), a count for too many of something. `instance` is the request, `urn:uuid:` and its `requestId`. A session that \
is missing or over is 401. What the user may not see is 404, as if it were not there; what \
they may see but not do is 403. A path that is no operation is 404 too, and a method an \
operation's path does not have is 405.

A request without a session whose JSON body has more than 64 KiB is 413 before it is 401. \
A body larger than its route reads is 413 with the reason `request_too_large` and the `limit` \
in bytes.

Any request may be answered 503 with the reason `database_busy`: the database did not take \
one of its statements in time (another write held the file, or no connection to it was to be \
had). That statement did nothing and may be taken a moment later. A request of one step may \
be sent again as it is; of one of several (`completeUpload`, `importArchive`) what was done \
before stays done, and sent again it is answered as its operation says.

The identifiers of users, workspaces, boards, blocks and categories are UUIDs, written \
lower-case with hyphens; a client takes every identifier as an opaque string. A `fileId` is \
no UUID: it is an unguessable name that ends in the file's extension. Times are \
milliseconds since the Unix epoch.

## Lists

Every list comes in an order, and the description of each operation says which. Where it \
says nothing else, the order is that of creation, oldest first, and by identifier among \
what was created in the same millisecond.

## The user's own space

Every account has a space of its own from the moment it is made, however it was made: \
an organization of the `kind` `personal` with the account as its only member and one \
workspace, which `getMyWorkspaces` lists with that `kind`. Its workspace is a workspace \
like another to the operations under `/w/{workspaceId}`: boards, wikis, files, search, \
the trash, export and import, and sharing by a public link (`setSharing`) work in it as \
anywhere. It has no name (a client calls it by the reader's language) and takes none.

What it has not is other people, and anybody to administer it. Its identifier is 404 \
for every operation under `/org/{orgId}` (the organization and its settings, its \
members, its workspaces, joining one as an administrator, its audit log, its provider \
of sign-ins), for its owner as for anybody else, and no answer of those operations \
names it. Under its workspace these are 404, as for a workspace that is not there: \
`createInvitation`, `getInvitations`, `revokeInvitation`, `emailInvitation`, \
`setGuestInvitations`, `setWorkspaceRole`, `removeWorkspaceMember`, the operations on teams (`/w/{workspaceId}/teams` and what is \
under it) and on the teams of a board or a wiki \
(`/w/{workspaceId}/containers/{containerId}/teams`). It has no login page \
(`getLoginOptions`) and no sign-in through a provider starts for it. Nothing in it is \
moved to or from another organization but by export and import, and no reference \
reaches into it from outside or out of it. What changes in it is told to nobody else: \
the server's operator is posted no block of it (`block_change_webhooks`), and it is in \
no audit log anybody but its owner reads.

An account that was made by a sign-in through an organization's provider has no \
password (`hasPassword` of `getMe`), and until it has one its space is reached by any \
session of the account. It is given one by `setPassword`, so that its person reaches \
the space when they are no longer in that organization. Once the account has a \
password, the space takes only sessions made with it (`login`): a provider is an \
organization's, and whoever holds it could make a session of the account. To a session \
made through a provider the routes under the space's `/w/{workspaceId}` then answer \
401 with the reason `password_sign_in_required`, `getMyWorkspaces` lists the space \
with `passwordRequired` true, `getMyMemberships` leaves its boards out, \
`getAccountDeletion` names it without what is in it, and the websocket follows \
nothing of it for such a session (`contracts/websocket.md`). The session stays and \
opens what it opened of the organizations; signing in with the password opens the \
space, and an organization that takes only sessions made through its provider \
(`sso_required`) then asks for a sign-in through that again. What is shared from the \
space by a public link is read without a session, and is not closed so.

It goes with its account: `getAccountDeletion` names it among `organizations` always, \
and while the account waits to be deleted it is closed with the account: what is \
shared from it by a public link answers 404 as a link that is not there, and answers \
again if the deletion is cancelled.

## Bodies edited together

While people edit a card's body together the server holds it as a Y document. What it \
refuses of an editor has the reason `update_refused`, `update_out_of_step`, \
`update_too_large` or `document_too_large`; `too_many_documents` and \
`document_unavailable` say that it cannot take the document for now. A write of a card's \
whole body here (a patch of the card or of blocks, a card stored again) first writes to the \
card what its editors typed: a `readAt` from before that is refused with 409, and if it \
cannot be written the write is refused with `document_unavailable`. The editors \
themselves speak to the server over the websocket, whose messages name the same reasons \
(`contracts/websocket.md`); `document_read_only`, `connection_too_slow`, \
`too_many_documents_joined`, `document_full`, `too_many_connections_joined` and \
`too_many_frames` come only there. The \
page of each kind says when it comes. `search_not_configured` comes from none of these \
operations: only from the reindex of the administration socket, on a server configured \
without a search server (a search there searches its database).

## Request bodies

A request schema lists the members a request is for. A member that is not required may be \
left out. `null` is a value only of a member whose schema allows it, and is then none; `null` \
for any other member is refused with 400, as a value of another type is. A member the schema \
marks as required is one the request cannot do without; \
where the server takes a missing one as empty instead of refusing the request, the result \
is that of the empty value (a login without a password is a failed login). A body with \
members the schema does not list is mostly not refused for them. The exceptions: \
`patchUserSettings` and `patchMe` refuse every member they do not know, `createBoard` and \
`createCategory` refuse an `id`, and `setSharing` refuses a `token`.";

/// What an operation's `default` response says.
const REFUSAL: &str = "The request was refused, or failed. The status says the kind: 400 for a \
request that is malformed or asks for what cannot be, 401 without a session, 403 for what the \
user may not do, 404 for what is not there for them, 405 for a method the path does not have, \
409 for what is in the way, 412 for a condition of the request that does not hold, 413 for a \
body that is too large, 416 for a range that is not there, 429 for too many requests, 500 for \
a failure of the server, 503 for a server that cannot serve for now.";

/// Which members of a schema are always there.
#[derive(Debug, Clone, Copy)]
enum Members {
    /// The schema says it itself: it was written for the contract, or has no members.
    AsWritten,
    /// Every member.
    All,
    /// Every member but these, which are left out when they are empty.
    AllBut(&'static [&'static str]),
    /// These members and no others.
    Only(&'static [&'static str]),
}

/// Every schema of the document, with the members that are always there.
///
/// The types the server answers with are decoded leniently where they are read, so
/// their derived schemas call every member optional. In an answer every member is
/// there, and that is what the contract says. A schema that is not listed here fails
/// [`document`], so that a new type is looked at once.
const SCHEMAS: &[(&str, Members)] = &[
    // What the server answers with.
    (
        "AcceptedInvitation",
        Members::AllBut(&["containerId", "containerKind"]),
    ),
    (
        "AccountDeletion",
        Members::AllBut(&["requestedAt", "deleteAfter"]),
    ),
    ("AccountDeletionAsked", Members::All),
    (
        "LeavingOrganization",
        Members::AllBut(&["boards", "wikis", "files"]),
    ),
    ("OrganizationName", Members::All),
    (
        "EmailVerified",
        Members::AllBut(&["containerId", "containerKind"]),
    ),
    ("Block", Members::AllBut(&["editedAt"])),
    ("Body", Members::AsWritten),
    ("Container", Members::All),
    (
        "ContainerMember",
        Members::AllBut(&["synthetic", "inheritedRole"]),
    ),
    ("ContainerTeam", Members::All),
    ("ContainerTeamRole", Members::AsWritten),
    ("ContainersAndBlocks", Members::All),
    ("BodyVersion", Members::All),
    ("BodyVersionContent", Members::All),
    ("Card", Members::AllBut(&["preview"])),
    ("CardPreview", Members::All),
    ("Category", Members::All),
    ("CategoryContainerMetadata", Members::All),
    ("CategoryContainers", Members::All),
    ("ClientConfig", Members::All),
    ("CommentVersion", Members::All),
    ("DeletedItem", Members::AllBut(&["kind"])),
    ("FileInfo", Members::All),
    ("FileLease", Members::All),
    ("FileLeaseRequest", Members::AsWritten),
    ("FileVersion", Members::All),
    ("FileVersionBase", Members::AsWritten),
    ("HeldFileLease", Members::All),
    ("ImportResult", Members::All),
    ("InvalidInput", Members::Only(&["detail", "reason"])),
    ("LinkedIdentity", Members::All),
    (
        "Invitation",
        Members::AllBut(&[
            "containerId",
            "containerRole",
            "maxUses",
            "parentId",
            "recipient",
            "token",
        ]),
    ),
    (
        "InvitationPreview",
        Members::AllBut(&["containerTitle", "containerKind"]),
    ),
    ("LoginOptions", Members::All),
    ("OidcStart", Members::All),
    ("LoginOrganization", Members::All),
    ("OidcProvider", Members::All),
    ("OidcFormerProvider", Members::All),
    ("OidcFormerProviderEnd", Members::All),
    ("OidcPendingProvider", Members::All),
    (
        "Problem",
        Members::Only(&["type", "title", "status", "detail"]),
    ),
    ("Readiness", Members::All),
    ("ReadinessCheck", Members::AsWritten),
    ("SearchHit", Members::All),
    ("SearchKind", Members::AsWritten),
    ("ServerMetadata", Members::AsWritten),
    ("Sharing", Members::AllBut(&["token", "updateAt"])),
    ("SnippetPart", Members::All),
    ("Subscription", Members::All),
    ("WelcomeBoard", Members::All),
    ("Workspace", Members::All),
    ("Team", Members::All),
    ("TeamMember", Members::All),
    ("TeamName", Members::AsWritten),
    ("AuditAction", Members::AsWritten),
    ("AuditPerson", Members::All),
    ("AuditRecord", Members::All),
    ("AuditRecordActor", Members::AsWritten),
    ("Organization", Members::All),
    ("OrgMember", Members::All),
    ("OrgWorkspace", Members::All),
    ("UploadOpened", Members::All),
    ("UploadState", Members::All),
    ("ReceivedPart", Members::All),
    ("UploadedFile", Members::AsWritten),
    ("User", Members::All),
    ("Me", Members::All),
    ("PersonName", Members::All),
    ("UserSettings", Members::AsWritten),
    ("UserWorkspace", Members::All),
    ("WorkspaceMember", Members::All),
    ("WorkspaceRole", Members::AsWritten),
    ("OrgRole", Members::AsWritten),
    ("OrgKind", Members::AsWritten),
    ("Page", Members::All),
    ("PageNode", Members::All),
    ("LinkedBlock", Members::All),
    ("WorkspaceRoleChange", Members::All),
    // What a request carries.
    ("AcceptInvitation", Members::Only(&["token"])),
    ("AccountChange", Members::AsWritten),
    ("AdminChange", Members::AsWritten),
    ("BlockPatch", Members::Only(&[])),
    (
        "BlockPatchBatch",
        Members::Only(&["blockIds", "blockPatches"]),
    ),
    ("ContainerPatch", Members::Only(&[])),
    ("CardPatch", Members::Only(&[])),
    ("CategoryUpdate", Members::AsWritten),
    (
        "ChangePasswordRequest",
        Members::Only(&["oldPassword", "newPassword"]),
    ),
    ("DeleteContainersAndBlocks", Members::Only(&["containers"])),
    ("FileForm", Members::AsWritten),
    ("FinishPasswordReset", Members::All),
    ("CompleteEmailVerification", Members::Only(&["token"])),
    ("RequestEmailVerification", Members::Only(&[])),
    ("RequestPasswordReset", Members::Only(&["email"])),
    ("GuestInvitationsChange", Members::All),
    ("ImportRequest", Members::AsWritten),
    ("InvitationMail", Members::Only(&["token", "recipients"])),
    ("InvitationToken", Members::All),
    ("LoginRequest", Members::Only(&["password"])),
    ("MemberRoles", Members::AsWritten),
    (
        "NewAccount",
        Members::Only(&["username", "email", "password"]),
    ),
    ("NewBlock", Members::AsWritten),
    ("NewBoard", Members::AsWritten),
    ("NewContainersAndBlocks", Members::AsWritten),
    ("NewWiki", Members::AsWritten),
    ("NewPage", Members::Only(&[])),
    ("PagePatch", Members::Only(&[])),
    ("NewPageComment", Members::Only(&["body"])),
    ("PageCommentPatch", Members::Only(&["body", "readAt"])),
    ("PageMove", Members::Only(&[])),
    ("PageDuplicate", Members::Only(&[])),
    ("DeletedPage", Members::All),
    ("DiaryEntry", Members::All),
    ("DiaryDay", Members::AllBut(&["mood"])),
    ("DeletedDiaryEntry", Members::AllBut(&["mood"])),
    ("Mood", Members::AsWritten),
    ("NewDiary", Members::Only(&[])),
    ("NewDiaryEntry", Members::Only(&["date"])),
    ("DiaryEntryPatch", Members::Only(&[])),
    ("NewCard", Members::AsWritten),
    ("NewCategory", Members::AsWritten),
    ("NewFileVersion", Members::AsWritten),
    ("NewInvitation", Members::Only(&["role"])),
    ("NewMember", Members::AsWritten),
    ("NewSubscription", Members::AsWritten),
    ("NewUpload", Members::Only(&["name", "size"])),
    ("OrganizationPatch", Members::Only(&[])),
    ("OrgRoleChange", Members::AsWritten),
    ("WorkspaceName", Members::AsWritten),
    (
        "OidcProviderSettings",
        Members::Only(&["issuer", "clientId"]),
    ),
    ("OidcStartRequest", Members::Only(&[])),
    ("DeleteAccountRequest", Members::Only(&["username"])),
    (
        "PatchContainersAndBlocks",
        Members::Only(&["containerIds", "containerPatches"]),
    ),
    (
        "RegisterRequest",
        Members::Only(&["username", "email", "password"]),
    ),
    ("NewOrganization", Members::All),
    ("SetPasswordRequest", Members::All),
    ("SettingsChange", Members::AsWritten),
    ("SharingRequest", Members::AsWritten),
    (
        "UnlinkRequest",
        Members::Only(&["issuer", "subject", "password"]),
    ),
    ("ReferenceIds", Members::AsWritten),
    ("UserIds", Members::AsWritten),
    ("UserPreferencesPatch", Members::Only(&[])),
    // Closed sets of words.
    ("CardOpen", Members::AsWritten),
    ("DeletionProof", Members::AsWritten),
    ("CategoryType", Members::AsWritten),
    ("ContainerKind", Members::AsWritten),
    ("ContainerType", Members::AsWritten),
    ("Credential", Members::AsWritten),
    ("Density", Members::AsWritten),
    ("HourCycle", Members::AsWritten),
    ("LinkedKind", Members::AsWritten),
    ("MinimumRole", Members::AsWritten),
    ("NameShown", Members::AsWritten),
    ("SubscriberType", Members::AsWritten),
    ("Theme", Members::AsWritten),
    ("Title", Members::AsWritten),
    ("WeekStart", Members::AsWritten),
];

/// Paths whose parameter names the router cannot have: a position of the router has one
/// name, and these two routes read the first position after `/subscriptions` as
/// different things. The document names each as what it is.
const RENAMED_PATHS: [(&str, &str); 2] = [
    (
        "/api/v1/w/{workspaceId}/subscriptions/{blockOrSubscriberId}/{subscriberId}",
        "/api/v1/w/{workspaceId}/subscriptions/{blockId}/{subscriberId}",
    ),
    (
        "/api/v1/w/{workspaceId}/subscriptions/{blockOrSubscriberId}",
        "/api/v1/w/{workspaceId}/subscriptions/{subscriberId}",
    ),
];

/// The methods an OpenAPI path item has operations under.
const METHODS: [&str; 8] = [
    "get", "put", "post", "delete", "options", "head", "patch", "trace",
];

/// The document could not be made: the routes or the schemas say something the contract
/// cannot be written from. The message names what.
#[derive(Debug, thiserror::Error)]
#[error("the API contract cannot be made: {0}")]
pub struct ContractError(String);

/// The OpenAPI document of the API, as the text of `contracts/openapi.json`: JSON with
/// two spaces of indentation, the members of every object in the order of their names,
/// and a line break at the end.
///
/// # Errors
///
/// Returns a [`ContractError`] if a route or a schema is not fit for the contract: an
/// operation without a name or a summary, a path parameter nothing describes, a schema
/// the table of schemas in this module does not list.
pub fn document() -> Result<String, ContractError> {
    let routes = super::routes();
    let api = OpenApiRouter::<App>::new()
        .merge(routes.json)
        .merge(routes.unauthenticated)
        .merge(routes.uploads)
        .merge(routes.shared);
    let (_, openapi) = OpenApiRouter::<App>::new()
        .nest(super::PREFIX, api)
        .split_for_parts();

    let mut document = serde_json::to_value(&openapi)
        .map_err(|error| ContractError(format!("the document is no JSON: {error}")))?;
    write_head(&mut document);
    finish_schemas(&mut document)?;
    finish_inline_schemas(object_at(&mut document, "/paths")?);
    plain_descriptions(&mut document);
    rename_paths(&mut document)?;
    finish_operations(&mut document)?;

    let mut text = serde_json::to_string_pretty(&sorted(document))
        .map_err(|error| ContractError(format!("the document is no JSON: {error}")))?;
    text.push('\n');
    Ok(text)
}

/// Writes what the document says about the API as a whole: what it is, how a request
/// shows who sends it, and the groups of its operations.
fn write_head(document: &mut Value) {
    document["info"] = json!({
        "title": "fvoci",
        "version": env!("CARGO_PKG_VERSION"),
        "description": DESCRIPTION,
        "license": { "name": env!("CARGO_PKG_LICENSE"), "identifier": env!("CARGO_PKG_LICENSE") },
    });
    // The paths are those of the server the document comes with, wherever that is.
    document["servers"] = json!([{ "url": "/", "description": "The server itself." }]);
    document["tags"] = TAGS
        .iter()
        .map(|(name, description)| json!({ "name": name, "description": description }))
        .collect();
    document["components"]["securitySchemes"] = json!({
        SESSION_COOKIE: {
            "type": "apiKey",
            "in": "cookie",
            "name": "__Host-fvoci_session",
            "description": "The session cookie `login` sets. Over plain HTTP its name is `fvoci_session`.",
        },
        BEARER_TOKEN: {
            "type": "http",
            "scheme": "bearer",
            "description": "The value of the session cookie, for clients that are no browsers.",
        },
    });
    // What an operation needs unless it says otherwise: one of the two.
    document["security"] = json!([{ SESSION_COOKIE: [] }, { BEARER_TOKEN: [] }]);
}

/// The members of a JSON object that is expected at `pointer`.
fn object_at<'v>(
    document: &'v mut Value,
    pointer: &str,
) -> Result<&'v mut Map<String, Value>, ContractError> {
    document
        .pointer_mut(pointer)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| ContractError(format!("the document has no object at {pointer}")))
}

/// Brings the schemas to what the contract says: `Problem` is among them, none
/// carries the defaults of the decoding, and each has the members [`SCHEMAS`] says are
/// always there.
fn finish_schemas(document: &mut Value) -> Result<(), ContractError> {
    let schemas = object_at(document, "/components/schemas")?;
    for (name, schema) in [
        ("Problem", Problem::schema()),
        ("InvalidInput", InvalidInput::schema()),
    ] {
        let schema = serde_json::to_value(schema)
            .map_err(|error| ContractError(format!("{name} has no schema: {error}")))?;
        schemas.insert(name.to_owned(), schema);
    }
    schemas["Problem"]["x-problem-types"] = problem_types();

    let listed: BTreeSet<&str> = SCHEMAS.iter().map(|(name, _)| *name).collect();
    if let Some(unlisted) = schemas.keys().find(|name| !listed.contains(name.as_str())) {
        return Err(ContractError(format!(
            "the schema {unlisted} is not listed in api::contract::SCHEMAS"
        )));
    }
    for (name, members) in SCHEMAS {
        let schema = schemas.get_mut(*name).ok_or_else(|| {
            ContractError(format!(
                "api::contract::SCHEMAS lists {name}, which no operation uses"
            ))
        })?;
        strip_defaults(schema);
        open_free_objects(schema);
        require_members(name, schema, *members)?;
    }
    Ok(())
}

/// Every kind of problem, as the contract lists them: the `type` on a server at the
/// root of its host (a server under a base path has the base path before it), the
/// `title`, and the `status` or the `reason` the kind is of.
fn problem_types() -> Value {
    crate::problems::all()
        .into_iter()
        .map(|problem| {
            let mut listed = json!({
                "type": problem.type_reference(),
                "title": problem.title,
            });
            match problem.basis {
                Basis::Status(status) => listed["status"] = json!(status),
                Basis::Reason(reason) => listed["reason"] = json!(reason),
            }
            listed
        })
        .collect()
}

/// Does for the schemas that are written where they are used, in the parameters, the
/// request bodies and the answers of the operations, what [`finish_schemas`] does for
/// the named ones: none carries a default of the decoding, and an object without
/// members may have any.
fn finish_inline_schemas(members: &mut Map<String, Value>) {
    for (name, member) in members {
        if name == "schema" {
            strip_defaults(member);
            open_free_objects(member);
        } else {
            match member {
                Value::Object(inner) => finish_inline_schemas(inner),
                Value::Array(items) => {
                    for item in items.iter_mut().filter_map(Value::as_object_mut) {
                        finish_inline_schemas(item);
                    }
                }
                _ => {}
            }
        }
    }
}

/// Removes the `default` of a schema and of every schema inside it. The defaults are
/// those of the decoding: what a member is taken as when a request leaves it out. In an
/// answer nothing is left out, and for requests the descriptions say it.
fn strip_defaults(schema: &mut Value) {
    let Some(members) = schema.as_object_mut() else {
        return;
    };
    members.remove("default");
    if let Some(properties) = members.get_mut("properties").and_then(Value::as_object_mut) {
        properties.values_mut().for_each(strip_defaults);
    }
    for name in ["items", "additionalProperties"] {
        if let Some(inner) = members.get_mut(name) {
            strip_defaults(inner);
        }
    }
    for name in ["allOf", "oneOf", "anyOf"] {
        if let Some(parts) = members.get_mut(name).and_then(Value::as_array_mut) {
            parts.iter_mut().for_each(strip_defaults);
        }
    }
}

/// Says of every object the schema gives no members for that it may have any: the
/// free-form objects, such as the fields of a block. An object schema that says nothing
/// reads the same to a validator, but a generator of types takes it for an object that
/// is always empty.
fn open_free_objects(schema: &mut Value) {
    let Some(members) = schema.as_object_mut() else {
        return;
    };
    let is_object = match members.get("type") {
        Some(Value::String(name)) => name == "object",
        Some(Value::Array(names)) => names.iter().any(|name| name == "object"),
        _ => false,
    };
    let says_its_members = [
        "properties",
        "additionalProperties",
        "allOf",
        "oneOf",
        "anyOf",
    ]
    .iter()
    .any(|name| members.contains_key(*name));
    if is_object && !says_its_members {
        members.insert("additionalProperties".to_owned(), Value::Bool(true));
    }
    if let Some(properties) = members.get_mut("properties").and_then(Value::as_object_mut) {
        properties.values_mut().for_each(open_free_objects);
    }
    // `additionalProperties: true` is no schema and is passed over as no object.
    for name in ["items", "additionalProperties"] {
        if let Some(inner) = members.get_mut(name) {
            open_free_objects(inner);
        }
    }
    for name in ["allOf", "oneOf", "anyOf"] {
        if let Some(parts) = members.get_mut(name).and_then(Value::as_array_mut) {
            parts.iter_mut().for_each(open_free_objects);
        }
    }
}

/// Sets which members of the schema `name` are always there.
fn require_members(name: &str, schema: &mut Value, members: Members) -> Result<(), ContractError> {
    if matches!(members, Members::AsWritten) {
        return Ok(());
    }
    // A type that has another's members inline is the two side by side; its own
    // members are in the part that is no reference.
    let own = match schema.get_mut("allOf").and_then(Value::as_array_mut) {
        Some(parts) => parts
            .iter_mut()
            .find(|part| part.get("properties").is_some()),
        None => Some(schema),
    };
    let own = own
        .and_then(Value::as_object_mut)
        .ok_or_else(|| ContractError(format!("the schema {name} has no members of its own")))?;
    let names: Vec<String> = own
        .get("properties")
        .and_then(Value::as_object)
        .map(|properties| properties.keys().cloned().collect())
        .unwrap_or_default();
    let unknown = |listed: &[&str]| {
        listed
            .iter()
            .find(|member| !names.iter().any(|name| name == *member))
            .map(|member| ContractError(format!("the schema {name} has no member {member}")))
    };
    let required: Vec<String> = match members {
        Members::AsWritten => return Ok(()),
        Members::All => names,
        Members::AllBut(optional) => {
            if let Some(error) = unknown(optional) {
                return Err(error);
            }
            names
                .into_iter()
                .filter(|name| !optional.contains(&name.as_str()))
                .collect()
        }
        Members::Only(required) => {
            if let Some(error) = unknown(required) {
                return Err(error);
            }
            required.iter().map(|&member| member.to_owned()).collect()
        }
    };
    if required.is_empty() {
        own.remove("required");
    } else {
        own.insert("required".to_owned(), Value::from(required));
    }
    Ok(())
}

/// Takes the brackets off the names that the descriptions link to in the Rust
/// documentation they are also: ``[`Container`]`` reads `` `Container` `` in the contract.
fn plain_descriptions(value: &mut Value) {
    match value {
        Value::Object(members) => {
            for (name, member) in members {
                match member {
                    Value::String(text) if name == "description" => {
                        *text = text.replace("[`", "`").replace("`]", "`");
                    }
                    other => plain_descriptions(other),
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(plain_descriptions),
        _ => {}
    }
}

/// Gives the paths of [`RENAMED_PATHS`] the names the contract has for them.
fn rename_paths(document: &mut Value) -> Result<(), ContractError> {
    let paths = object_at(document, "/paths")?;
    for (routed, named) in RENAMED_PATHS {
        let item = paths
            .remove(routed)
            .ok_or_else(|| ContractError(format!("there is no path {routed} to rename")))?;
        if paths.insert(named.to_owned(), item).is_some() {
            return Err(ContractError(format!("there are two paths {named}")));
        }
    }
    Ok(())
}

/// The names in braces of a path.
fn path_parameters(path: &str) -> impl Iterator<Item = &str> {
    path.split('{')
        .skip(1)
        .filter_map(|rest| rest.split_once('}'))
        .map(|(name, _)| name)
}

/// Gives every operation the answer of a refusal, and checks what the attributes of the
/// handlers cannot be made to check: that each operation has a name no other has, a
/// summary and a group, that every parameter of its path is described and every path
/// parameter it describes is in its path, and that what it says a request needs is one
/// of the security schemes of the document.
fn finish_operations(document: &mut Value) -> Result<(), ContractError> {
    let schemes: BTreeSet<String> = document
        .pointer("/components/securitySchemes")
        .and_then(Value::as_object)
        .map(|schemes| schemes.keys().cloned().collect())
        .unwrap_or_default();
    let refusal = json!({
        "description": REFUSAL,
        "content": {
            PROBLEM_MEDIA_TYPE: { "schema": { "$ref": "#/components/schemas/Problem" } },
        },
    });
    let mut names = BTreeSet::new();
    for (path, item) in object_at(document, "/paths")? {
        for method in METHODS {
            let Some(operation) = item.get_mut(method).and_then(Value::as_object_mut) else {
                continue;
            };
            let what = format!("{method} {path}");
            let name = operation
                .get("operationId")
                .and_then(Value::as_str)
                .ok_or_else(|| ContractError(format!("{what} has no operationId")))?;
            if !names.insert(name.to_owned()) {
                return Err(ContractError(format!(
                    "two operations have the operationId {name}"
                )));
            }
            for member in ["summary", "tags"] {
                if !operation.contains_key(member) {
                    return Err(ContractError(format!("{what} has no {member}")));
                }
            }
            for parameter in path_parameters(path) {
                let described = operation
                    .get("parameters")
                    .and_then(Value::as_array)
                    .is_some_and(|parameters| {
                        parameters.iter().any(|described| {
                            described["in"] == "path" && described["name"] == parameter
                        })
                    });
                if !described {
                    return Err(ContractError(format!(
                        "{what} does not describe its path parameter {parameter}"
                    )));
                }
            }
            let stray = operation
                .get("parameters")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|described| described["in"] == "path")
                .filter_map(|described| described["name"].as_str())
                .find(|name| !path_parameters(path).any(|parameter| parameter == *name));
            if let Some(stray) = stray {
                return Err(ContractError(format!(
                    "{what} describes a path parameter {stray} that its path does not have"
                )));
            }
            let unknown_scheme = operation
                .get("security")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_object)
                .flat_map(serde_json::Map::keys)
                .find(|scheme| !schemes.contains(*scheme));
            if let Some(scheme) = unknown_scheme {
                return Err(ContractError(format!(
                    "{what} asks for {scheme}, which is no security scheme of the document"
                )));
            }
            let responses = operation
                .entry("responses")
                .or_insert_with(|| json!({}))
                .as_object_mut()
                .ok_or_else(|| ContractError(format!("the responses of {what} are no object")))?;
            // A route a browser is sent to succeeds by sending it on (303).
            if !responses
                .keys()
                .any(|status| status.starts_with('2') || status == "303")
            {
                return Err(ContractError(format!("{what} has no answer of success")));
            }
            responses.insert("default".to_owned(), refusal.clone());
        }
    }
    Ok(())
}

/// The value with the members of every object in the order of their names, whatever
/// order they were put in.
///
/// As this workspace builds `serde_json`, an object keeps its members in that order by
/// itself, and this changes nothing. It is here for the day a dependency turns on the
/// feature that keeps them in the order they were put in: the file would then change
/// from top to bottom without the API having changed at all.
fn sorted(value: Value) -> Value {
    match value {
        Value::Object(members) => {
            let mut members: Vec<(String, Value)> = members.into_iter().collect();
            members.sort_by(|(left, _), (right, _)| left.cmp(right));
            Value::Object(
                members
                    .into_iter()
                    .map(|(name, value)| (name, sorted(value)))
                    .collect(),
            )
        }
        Value::Array(items) => Value::Array(items.into_iter().map(sorted).collect()),
        other => other,
    }
}

#[cfg(test)]
mod tests;
