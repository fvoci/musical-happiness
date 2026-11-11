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

use fvoci_model::ErrorBody;
use serde_json::{Map, Value, json};
use utoipa::PartialSchema;
use utoipa_axum::router::OpenApiRouter;

use crate::app::App;

/// The names operations are grouped under.
pub(crate) mod tag {
    pub(crate) const SYSTEM: &str = "system";
    pub(crate) const AUTH: &str = "auth";
    pub(crate) const USERS: &str = "users";
    pub(crate) const TEAMS: &str = "teams";
    pub(crate) const BOARDS: &str = "boards";
    pub(crate) const MEMBERS: &str = "members";
    pub(crate) const SHARING: &str = "sharing";
    pub(crate) const BLOCKS: &str = "blocks";
    pub(crate) const CARDS: &str = "cards";
    pub(crate) const SIDEBAR: &str = "sidebar";
    pub(crate) const FILES: &str = "files";
    pub(crate) const ARCHIVES: &str = "archives";
    pub(crate) const SUBSCRIPTIONS: &str = "subscriptions";
}

/// The tags in the order the document lists them, each with what it is about.
const TAGS: [(&str, &str); 13] = [
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
    (tag::TEAMS, "The team and its users."),
    (tag::BOARDS, "Boards and templates."),
    (tag::MEMBERS, "Who is on a board, and with which role."),
    (tag::SHARING, "Sharing a board with whoever has its link."),
    (
        tag::BLOCKS,
        "The blocks of a board: cards, views, comments, and what a card is made of.",
    ),
    (tag::CARDS, "The blocks of type `card`, as cards."),
    (
        tag::SIDEBAR,
        "A user's categories and the boards filed under them.",
    ),
    (tag::FILES, "The files of a board."),
    (tag::ARCHIVES, "Exporting boards and importing them."),
    (tag::SUBSCRIPTIONS, "Subscriptions to blocks."),
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
instead. A few operations that read a shared board need no session when the request has the \
board's `readToken`.

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
other status comes with an `ErrorBody`, unless the operation says otherwise (`getFile` \
does): `error` names the kind of the refusal and goes with the status, `reason` tells apart \
causes a client must tell apart, `field` names the member of the request that was refused, \
and `limit` is the number of bytes a body that was too large (413) may have. A session that \
is missing or over is 401. What the user may not see is 404, as if it were not there; what \
they may see but not do is 403. A path that is no operation is 404 too, and a method an \
operation's path does not have is 405.

A request without a session whose JSON body has more than 64 KiB is 413 before it is 401.

The identifiers of users, teams, boards, blocks and categories are UUIDs, written \
lower-case with hyphens; a client takes every identifier as an opaque string. A `fileId` is \
no UUID: it is an unguessable name that ends in the file's extension. Times are \
milliseconds since the Unix epoch.

## Lists

Every list comes in an order, and the description of each operation says which. Where it \
says nothing else, the order is that of creation, oldest first, and by identifier among \
what was created in the same millisecond.

## Request bodies

A request schema lists the members a request is for. An optional member may be left out or \
be `null`. A member the schema marks as required is one the request cannot do without; \
where the server takes a missing one as empty instead of refusing the request, the result \
is that of the empty value (a login without a password is a failed login). A body with \
members the schema does not list is mostly not refused for them. The exceptions: \
`patchUserSettings` and `patchMe` refuse every member they do not know, `createBoard` and \
`createCategory` refuse an `id`, and `setSharing` refuses a `token`.";

/// What an operation's `default` response says.
const REFUSAL: &str = "The request was refused, or failed. The status says the kind: 400 for a \
request that is malformed or asks for what cannot be, 401 without a session, 403 for what the \
user may not do, 404 for what is not there for them, 405 for a method the path does not have, \
409 for what is in the way, 413 for a body that is too large, 429 for too many requests, 500 \
for a failure of the server.";

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
    ("Block", Members::AllBut(&["editedAt"])),
    ("Body", Members::AsWritten),
    ("Board", Members::All),
    ("BoardMember", Members::All),
    ("BoardsAndBlocks", Members::All),
    ("Card", Members::All),
    ("Category", Members::All),
    ("CategoryBoardMetadata", Members::All),
    ("CategoryBoards", Members::All),
    ("ClientConfig", Members::All),
    ("DeletedItem", Members::All),
    ("CommentVersion", Members::All),
    ("ErrorBody", Members::Only(&["error", "message"])),
    ("FileInfo", Members::All),
    ("OnboardingStart", Members::AsWritten),
    ("ServerMetadata", Members::AsWritten),
    ("Sharing", Members::AllBut(&["token", "updateAt"])),
    ("Subscription", Members::All),
    ("Team", Members::AllBut(&["signupToken"])),
    ("UploadOpened", Members::All),
    ("UploadState", Members::All),
    ("ReceivedPart", Members::All),
    ("UploadedFile", Members::AsWritten),
    ("User", Members::All),
    ("UserSettings", Members::AsWritten),
    // What a request carries.
    ("AccountChange", Members::AsWritten),
    ("AdminChange", Members::AsWritten),
    ("BlockPatch", Members::Only(&[])),
    (
        "BlockPatchBatch",
        Members::Only(&["blockIds", "blockPatches"]),
    ),
    ("BoardPatch", Members::Only(&[])),
    ("CardPatch", Members::Only(&[])),
    ("CategoryUpdate", Members::AsWritten),
    (
        "ChangePasswordRequest",
        Members::Only(&["oldPassword", "newPassword"]),
    ),
    ("DeleteBoardsAndBlocks", Members::Only(&["boards"])),
    ("FileForm", Members::AsWritten),
    ("ImportRequest", Members::AsWritten),
    ("LoginRequest", Members::Only(&["password"])),
    ("MemberRoles", Members::AsWritten),
    ("NewBlock", Members::AsWritten),
    ("NewBoard", Members::AsWritten),
    ("NewBoardsAndBlocks", Members::AsWritten),
    ("NewCard", Members::AsWritten),
    ("NewCategory", Members::AsWritten),
    ("NewMember", Members::AsWritten),
    ("NewSubscription", Members::AsWritten),
    ("NewUpload", Members::Only(&["name", "size"])),
    (
        "PatchBoardsAndBlocks",
        Members::Only(&["boardIds", "boardPatches"]),
    ),
    (
        "RegisterRequest",
        Members::Only(&["username", "email", "password"]),
    ),
    ("SettingsChange", Members::AsWritten),
    ("SharingRequest", Members::AsWritten),
    ("UserPreferencesPatch", Members::Only(&[])),
    // Closed sets of words.
    ("BoardType", Members::AsWritten),
    ("CardOpen", Members::AsWritten),
    ("CategoryType", Members::AsWritten),
    ("Density", Members::AsWritten),
    ("HourCycle", Members::AsWritten),
    ("MinimumRole", Members::AsWritten),
    ("SubscriberType", Members::AsWritten),
    ("Theme", Members::AsWritten),
    ("WeekStart", Members::AsWritten),
];

/// Paths whose parameter names the router cannot have: a position of the router has one
/// name, and these two routes read the first position after `/subscriptions` as
/// different things. The document names each as what it is.
const RENAMED_PATHS: [(&str, &str); 2] = [
    (
        "/api/v1/subscriptions/{blockOrSubscriberId}/{subscriberId}",
        "/api/v1/subscriptions/{blockId}/{subscriberId}",
    ),
    (
        "/api/v1/subscriptions/{blockOrSubscriberId}",
        "/api/v1/subscriptions/{subscriberId}",
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
        .merge(routes.uploads);
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

/// Brings the schemas to what the contract says: `ErrorBody` is among them, none
/// carries the defaults of the decoding, and each has the members [`SCHEMAS`] says are
/// always there.
fn finish_schemas(document: &mut Value) -> Result<(), ContractError> {
    let schemas = object_at(document, "/components/schemas")?;
    let error_body = serde_json::to_value(ErrorBody::schema())
        .map_err(|error| ContractError(format!("the error body has no schema: {error}")))?;
    schemas.insert("ErrorBody".to_owned(), error_body);

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
/// documentation they are also: ``[`Board`]`` reads `` `Board` `` in the contract.
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
            "application/json": { "schema": { "$ref": "#/components/schemas/ErrorBody" } },
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
            if !responses.keys().any(|status| status.starts_with('2')) {
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
mod tests {
    use serde_json::{Value, json};

    use super::{
        Members, document, finish_inline_schemas, finish_operations, open_free_objects,
        path_parameters, rename_paths, require_members, sorted, strip_defaults,
    };

    #[test]
    fn the_document_can_be_made() {
        let text = document().expect("the routes and schemas are fit for the contract");
        let document: Value = serde_json::from_str(&text).expect("the document is JSON");
        assert_eq!(
            document["openapi"], "3.1.0",
            "the version of OpenAPI the document is written in"
        );
        assert!(text.ends_with("}\n"), "the text ends with one line break");
    }

    #[test]
    fn every_operation_answers_a_refusal_with_the_error_body() {
        let text = document().expect("the document");
        let document: Value = serde_json::from_str(&text).expect("JSON");
        let paths = document["paths"].as_object().expect("paths");
        let mut operations = 0;
        for (path, item) in paths {
            for (method, operation) in item.as_object().expect("a path item") {
                operations += 1;
                assert_eq!(
                    operation["responses"]["default"]["content"]["application/json"]["schema"]["$ref"],
                    "#/components/schemas/ErrorBody",
                    "the refusal of {method} {path}"
                );
            }
        }
        assert!(operations > 70, "the document has {operations} operations");
    }

    /// The operations that need no session at all.
    const NO_SESSION: [&str; 5] = ["getClientConfig", "hello", "login", "ping", "register"];
    /// The operations that are read with a session or with the read token of a board.
    const READ_TOKEN: [&str; 4] = ["getBlocks", "getBoard", "getFile", "getFileInfo"];

    #[test]
    fn what_needs_no_session_says_so() {
        let text = document().expect("the document");
        let document: Value = serde_json::from_str(&text).expect("JSON");
        let mut no_session = Vec::new();
        let mut read_token = Vec::new();
        for item in document["paths"].as_object().expect("paths").values() {
            for operation in item.as_object().expect("a path item").values() {
                let name = operation["operationId"].as_str().expect("a name");
                let security = &operation["security"];
                if *security == json!([{}]) {
                    no_session.push(name);
                } else if *security == json!([{}, { "sessionCookie": [] }, { "bearerToken": [] }]) {
                    read_token.push(name);
                } else {
                    assert_eq!(
                        *security,
                        Value::Null,
                        "{name}: what says nothing needs what the document says for all"
                    );
                }
            }
        }
        no_session.sort_unstable();
        read_token.sort_unstable();
        assert_eq!(
            no_session, NO_SESSION,
            "these need no session, and no others"
        );
        assert_eq!(
            read_token, READ_TOKEN,
            "these are read with a session or a read token, and no others"
        );
        assert_eq!(
            document["security"],
            json!([{ "sessionCookie": [] }, { "bearerToken": [] }]),
            "what every operation needs unless it says otherwise"
        );
    }

    #[test]
    fn an_answer_has_every_member_and_a_patch_needs_none() {
        let text = document().expect("the document");
        let document: Value = serde_json::from_str(&text).expect("JSON");
        let schemas = &document["components"]["schemas"];
        let board = &schemas["Board"];
        assert_eq!(
            board["required"].as_array().map(Vec::len),
            board["properties"].as_object().map(serde_json::Map::len),
            "every member of a board is always there"
        );
        assert_eq!(
            schemas["BoardPatch"]["required"],
            Value::Null,
            "a patch needs no member"
        );
        assert_eq!(
            schemas["Sharing"]["required"],
            json!(["enabled"]),
            "the token and the time are left out while a board is not shared"
        );
        assert_eq!(
            schemas["CategoryBoards"]["allOf"][1]["required"],
            json!(["boardMetadata"]),
            "the members of a category with its boards, beside those of the category"
        );
        assert!(board["required"].is_array() && board["properties"].is_object());
        assert_eq!(
            defaults_in(schemas, "components.schemas"),
            Vec::<String>::new(),
            "no schema carries the defaults of the decoding"
        );
        assert_eq!(
            defaults_in(&document["paths"], "paths"),
            Vec::<String>::new(),
            "nor does a schema that is written where it is used"
        );
    }

    /// Where below `value` something has a `default` that is no member of a schema's
    /// `properties`, and no `default` answer of an operation.
    fn defaults_in(value: &Value, place: &str) -> Vec<String> {
        match value {
            Value::Object(members) => members
                .iter()
                .flat_map(|(name, member)| {
                    let inner = format!("{place}.{name}");
                    let is_schema_default = name == "default"
                        && !place.ends_with(".properties")
                        && !place.ends_with(".responses");
                    if is_schema_default {
                        vec![inner]
                    } else {
                        defaults_in(member, &inner)
                    }
                })
                .collect(),
            Value::Array(items) => items
                .iter()
                .flat_map(|item| defaults_in(item, place))
                .collect(),
            _ => Vec::new(),
        }
    }

    /// The members a value is written with.
    fn sent<T: serde::Serialize>(value: &T) -> Vec<String> {
        serde_json::to_value(value)
            .expect("JSON")
            .as_object()
            .expect("an object")
            .keys()
            .cloned()
            .collect()
    }

    /// The members a schema says are always there, with those of the schemas it has
    /// inline.
    fn required_of(schemas: &Value, schema: &Value) -> Vec<String> {
        let mut required: Vec<String> = schema["required"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|name| name.as_str().map(str::to_owned))
            .collect();
        for part in schema["allOf"].as_array().into_iter().flatten() {
            let part = match part["$ref"].as_str() {
                Some(reference) => &schemas[reference.rsplit('/').next().expect("a name")],
                None => part,
            };
            required.extend(required_of(schemas, part));
        }
        required.sort_unstable();
        required
    }

    /// The table of schemas says by hand which members are always there. What the
    /// server writes says it too: a value with nothing in it is written with exactly
    /// the members that are never left out. A member that is given a
    /// `skip_serializing_if` and forgotten in the table fails here.
    #[test]
    fn what_is_always_there_is_what_the_server_writes() {
        use fvoci_model::{
            Block, Board, BoardMember, BoardsAndBlocks, Card, Category, CategoryBoardMetadata,
            CategoryBoards, ClientConfig, FileInfo, Sharing, Team, User,
        };

        let text = document().expect("the document");
        let document: Value = serde_json::from_str(&text).expect("JSON");
        let schemas = &document["components"]["schemas"];
        let written = [
            ("Block", sent(&Block::default())),
            ("Board", sent(&Board::default())),
            ("BoardMember", sent(&BoardMember::default())),
            ("BoardsAndBlocks", sent(&BoardsAndBlocks::default())),
            ("Card", sent(&Card::default())),
            ("Category", sent(&Category::default())),
            (
                "CategoryBoardMetadata",
                sent(&CategoryBoardMetadata::default()),
            ),
            ("CategoryBoards", sent(&CategoryBoards::default())),
            ("ClientConfig", sent(&ClientConfig::default())),
            ("FileInfo", sent(&FileInfo::default())),
            ("Sharing", sent(&Sharing::default())),
            ("Team", sent(&Team::default())),
            ("User", sent(&User::default())),
        ];
        for (name, mut members) in written {
            members.sort_unstable();
            assert_eq!(
                required_of(schemas, &schemas[name]),
                members,
                "the members of {name} that are always there"
            );
        }
    }

    /// The words of a closed set are those the server writes, letter for letter.
    #[test]
    fn a_closed_set_of_words_lists_the_words_as_they_are_written() {
        use fvoci_model::{CardOpen, Density, HourCycle, Theme, UserSettings, WeekStart};

        let text = document().expect("the document");
        let document: Value = serde_json::from_str(&text).expect("JSON");
        let schemas = &document["components"]["schemas"];
        for (name, words) in [
            ("CardOpen", CardOpen::WORDS),
            ("Density", Density::WORDS),
            ("HourCycle", HourCycle::WORDS),
            ("Theme", Theme::WORDS),
            ("WeekStart", WeekStart::WORDS),
        ] {
            assert_eq!(schemas[name]["enum"], json!(words), "{name}");
            assert!(schemas[name]["description"].is_string(), "{name}");
        }
        // And what an answer carries is among them.
        let settings = serde_json::to_value(UserSettings::default()).expect("JSON");
        for (member, set) in [
            ("cardOpen", "CardOpen"),
            ("density", "Density"),
            ("hourCycle", "HourCycle"),
            ("theme", "Theme"),
            ("weekStart", "WeekStart"),
        ] {
            let words = schemas[set]["enum"].as_array().expect("words");
            assert!(
                words.contains(&settings[member]),
                "{member} is {}, which is none of {words:?}",
                settings[member]
            );
        }
        for (set, value) in [
            ("BoardType", json!(fvoci_model::BoardType::Open)),
            ("BoardType", json!(fvoci_model::BoardType::Private)),
            ("MinimumRole", json!(fvoci_model::MinimumRole::default())),
            ("CategoryType", json!(fvoci_model::CategoryType::default())),
        ] {
            let words = schemas[set]["enum"].as_array().expect("words");
            assert!(words.contains(&value), "{value} is none of {words:?}");
        }
    }

    /// A document with one operation, for what [`finish_operations`] refuses.
    fn with_operation(path: &str, operation: &Value) -> Value {
        json!({
            "paths": { path: { "get": operation } },
            "components": { "securitySchemes": { "sessionCookie": {} } },
        })
    }

    #[test]
    fn an_operation_that_is_not_fit_for_the_contract_is_refused_with_the_reason() {
        let good = json!({
            "operationId": "getThing",
            "summary": "A thing",
            "tags": ["things"],
            "parameters": [{ "in": "path", "name": "thingId" }],
            "responses": { "200": {} },
        });
        let path = "/things/{thingId}";
        finish_operations(&mut with_operation(path, &good)).expect("this one is fit");

        let without = |member: &str| {
            let mut operation = good.clone();
            operation.as_object_mut().expect("an object").remove(member);
            operation
        };
        let with = |member: &str, value: Value| {
            let mut operation = good.clone();
            operation[member] = value;
            operation
        };
        for (operation, reason) in [
            (without("operationId"), "has no operationId"),
            (without("summary"), "has no summary"),
            (without("tags"), "has no tags"),
            (
                without("parameters"),
                "does not describe its path parameter thingId",
            ),
            (
                with(
                    "parameters",
                    json!([{ "in": "path", "name": "thingId" }, { "in": "path", "name": "other" }]),
                ),
                "describes a path parameter other that its path does not have",
            ),
            (
                with("responses", json!({ "404": {} })),
                "has no answer of success",
            ),
            (
                with("security", json!([{ "sessionCooky": [] }])),
                "asks for sessionCooky, which is no security scheme",
            ),
        ] {
            let error = finish_operations(&mut with_operation(path, &operation))
                .expect_err(reason)
                .to_string();
            assert!(error.contains(reason), "{error}");
        }

        let mut twice = json!({
            "paths": { "/things": { "get": without("parameters"), "post": without("parameters") } },
        });
        let error = finish_operations(&mut twice)
            .expect_err("one name for two")
            .to_string();
        assert!(
            error.contains("two operations have the operationId getThing"),
            "{error}"
        );
    }

    #[test]
    fn a_path_that_is_not_there_cannot_be_renamed() {
        let error = rename_paths(&mut json!({ "paths": {} }))
            .expect_err("nothing to rename")
            .to_string();
        assert!(error.contains("there is no path"), "{error}");
    }

    #[test]
    fn a_schema_that_is_written_where_it_is_used_is_finished_too() {
        let mut paths = json!({
            "/things": { "post": {
                "parameters": [{ "name": "limit", "schema": { "type": "integer", "default": 100 } }],
                "requestBody": { "content": { "application/json": {
                    "schema": { "type": "object", "default": {} },
                } } },
                "responses": { "200": { "content": { "application/json": {
                    "schema": { "type": "array", "items": { "type": "object" } },
                } } } },
            } },
        });
        finish_inline_schemas(paths.as_object_mut().expect("an object"));
        let operation = &paths["/things"]["post"];
        assert_eq!(
            operation["parameters"][0]["schema"],
            json!({ "type": "integer" })
        );
        assert_eq!(
            operation["requestBody"]["content"]["application/json"]["schema"],
            json!({ "type": "object", "additionalProperties": true })
        );
        assert_eq!(
            operation["responses"]["200"]["content"]["application/json"]["schema"]["items"],
            json!({ "type": "object", "additionalProperties": true })
        );
    }

    /// What each operation that answers a list says about the order of the list: words
    /// of its description, or of the description of its answer.
    const ORDERS: &[(&str, &str)] = &[
        ("createBlocks", "in the order of the request"),
        (
            "createBoardsAndBlocks",
            "The boards in the order of the request",
        ),
        ("duplicateBlock", "the copy of the block first"),
        (
            "duplicateBoard",
            "in the order of the original's, oldest first",
        ),
        (
            "getBlocks",
            "Oldest first; the blocks that one request made come in the order that request had them in",
        ),
        ("getBoards", "Oldest first"),
        ("getCards", "Oldest first; the pages cut that order"),
        ("getCategories", "by `sortOrder`, lowest first"),
        ("getCommentVersions", "The first one first"),
        (
            "getDeletedBoards",
            "The last deleted first; the pages cut that order",
        ),
        (
            "getDeletedCards",
            "The last deleted first; the pages cut that order",
        ),
        ("getMembers", "By the identifier of the user"),
        ("getMyMemberships", "By the identifier of the board"),
        (
            "getSubscriptions",
            "Oldest first, and by the identifier of the block",
        ),
        ("getTeamUsers", "Oldest account first"),
        (
            "getTeamUsersById",
            "Oldest account first, not in the order of the request",
        ),
        ("getTeams", "oldest first"),
        ("getTemplates", "Each of the two oldest first"),
        (
            "getUsers",
            "Oldest account first, not in the order of the request",
        ),
        ("patchBoardsAndBlocks", "each in the order of the request"),
        ("reorderCategories", "in their new order"),
        ("reorderCategoryBoards", "in their new order"),
        ("searchBoards", "Oldest first"),
        ("searchTeamBoards", "Oldest first"),
    ];

    /// Whether what an operation answers is a list, or boards with their blocks.
    fn answers_a_list(responses: &Value) -> bool {
        responses
            .as_object()
            .expect("responses")
            .iter()
            .filter(|(status, _)| status.starts_with('2'))
            .any(|(_, response)| {
                let schema = &response["content"]["application/json"]["schema"];
                schema["type"] == "array"
                    || schema["$ref"] == "#/components/schemas/BoardsAndBlocks"
            })
    }

    #[test]
    fn every_list_says_its_order() {
        let text = document().expect("the document");
        let document: Value = serde_json::from_str(&text).expect("JSON");
        let mut lists = Vec::new();
        for item in document["paths"].as_object().expect("paths").values() {
            for operation in item.as_object().expect("a path item").values() {
                if !answers_a_list(&operation["responses"]) {
                    continue;
                }
                let id = operation["operationId"].as_str().expect("an operation id");
                let said: String = [&operation["description"]]
                    .into_iter()
                    .chain(
                        operation["responses"]
                            .as_object()
                            .expect("responses")
                            .values()
                            .map(|response| &response["description"]),
                    )
                    .filter_map(Value::as_str)
                    .flat_map(str::split_whitespace)
                    .collect::<Vec<_>>()
                    .join(" ");
                let order = ORDERS
                    .iter()
                    .find(|(operation, _)| *operation == id)
                    .map(|(_, order)| *order);
                let Some(order) = order else {
                    panic!("{id} answers a list and ORDERS does not say in which order");
                };
                assert!(
                    said.contains(order),
                    "{id} is to say its list comes {order:?}; it says: {said}"
                );
                lists.push(id.to_owned());
            }
        }
        lists.sort_unstable();
        let named: Vec<&str> = ORDERS.iter().map(|(operation, _)| *operation).collect();
        assert_eq!(lists, named, "the operations that answer a list");
    }

    #[test]
    fn the_routes_that_share_a_position_are_named_apart() {
        let text = document().expect("the document");
        let document: Value = serde_json::from_str(&text).expect("JSON");
        let paths = document["paths"].as_object().expect("paths");
        assert!(
            paths.contains_key("/api/v1/subscriptions/{blockId}/{subscriberId}"),
            "the path that ends a subscription"
        );
        assert!(
            paths.contains_key("/api/v1/subscriptions/{subscriberId}"),
            "the path that lists subscriptions"
        );
        assert!(
            !text.contains("blockOrSubscriberId"),
            "the router's name for the shared position is not in the document"
        );
    }

    #[test]
    fn path_parameters_are_the_names_in_braces() {
        let names: Vec<&str> = path_parameters("/boards/{boardId}/blocks/{blockId}").collect();
        assert_eq!(names, ["boardId", "blockId"], "the names in the path");
        assert_eq!(
            path_parameters("/boards").count(),
            0,
            "a path without parameters"
        );
    }

    #[test]
    fn defaults_are_removed_at_every_depth_but_a_member_named_default_stays() {
        let mut schema = json!({
            "default": {},
            "properties": {
                "default": { "type": "string", "default": "" },
                "list": { "type": "array", "default": [], "items": { "default": 0 } },
            },
            "allOf": [{ "default": 1 }],
        });
        strip_defaults(&mut schema);
        assert_eq!(
            schema,
            json!({
                "properties": {
                    "default": { "type": "string" },
                    "list": { "type": "array", "items": {} },
                },
                "allOf": [{}],
            }),
            "the schema without the defaults"
        );
    }

    #[test]
    fn an_object_without_members_may_have_any() {
        let mut schema = json!({
            "type": "object",
            "properties": {
                "fields": { "type": "object" },
                "maybe": { "type": ["object", "null"] },
                "list": { "type": "array", "items": { "type": "object" } },
                "counts": { "type": "object", "additionalProperties": { "type": "integer" } },
                "title": { "type": "string" },
            },
        });
        open_free_objects(&mut schema);
        assert_eq!(
            schema,
            json!({
                "type": "object",
                "properties": {
                    "fields": { "type": "object", "additionalProperties": true },
                    "maybe": { "type": ["object", "null"], "additionalProperties": true },
                    "list": {
                        "type": "array",
                        "items": { "type": "object", "additionalProperties": true },
                    },
                    "counts": { "type": "object", "additionalProperties": { "type": "integer" } },
                    "title": { "type": "string" },
                },
            }),
            "only the objects that say nothing about their members are opened"
        );
    }

    #[test]
    fn a_member_that_is_not_there_cannot_be_required() {
        let mut schema = json!({ "properties": { "a": {}, "b": {} } });
        let error =
            require_members("Thing", &mut schema, Members::Only(&["c"])).expect_err("no member c");
        assert_eq!(
            error.to_string(),
            "the API contract cannot be made: the schema Thing has no member c",
            "what is wrong"
        );
        require_members("Thing", &mut schema, Members::AllBut(&["b"])).expect("a member b");
        assert_eq!(schema["required"], json!(["a"]), "all members but b");
    }

    #[test]
    fn members_are_written_in_the_order_of_their_names() {
        let value = sorted(json!({ "b": [{ "d": 1, "c": 2 }], "a": 0 }));
        assert_eq!(
            serde_json::to_string(&value).expect("JSON"),
            r#"{"a":0,"b":[{"c":2,"d":1}]}"#,
            "the members in order"
        );
    }
}
