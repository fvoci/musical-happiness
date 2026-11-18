//! The path and query parameters of the API, as the contract describes them.
//!
//! The handlers read their parameters themselves; these types only say, in a handler's
//! `params(...)`, which ones it reads.

use utoipa::openapi::Required;
use utoipa::openapi::path::{Parameter, ParameterBuilder, ParameterIn};
use utoipa::{IntoParams, PartialSchema};

/// Defines a type that stands for one parameter.
macro_rules! parameter {
    ($type:ident: $place:ident $name:literal, $schema:ty, $required:ident, $description:literal) => {
        #[doc = $description]
        #[derive(Debug)]
        pub(crate) struct $type;

        impl IntoParams for $type {
            fn into_params(_: impl Fn() -> Option<ParameterIn>) -> Vec<Parameter> {
                vec![
                    ParameterBuilder::new()
                        .name($name)
                        .parameter_in(ParameterIn::$place)
                        .required(Required::$required)
                        .description(Some($description))
                        .schema(Some(<$schema as PartialSchema>::schema()))
                        .build(),
                ]
            }
        }
    };
}

// -- in the path -----------------------------------------------------------------------

parameter!(WorkspaceId: Path "workspaceId", String, True, "The identifier of the workspace. 404 unless the user of the session works in it, a member who is no guest.");
parameter!(WorkerWorkspaceId: Path "workspaceId", String, True, "The identifier of the workspace. 404 unless the user of the session works in it, a member who is no guest.");
parameter!(MemberWorkspaceId: Path "workspaceId", String, True, "The identifier of the workspace. 404 unless the user of the session is a member of it, a guest too.");
parameter!(BoardId: Path "boardId", String, True, "The identifier of the board.");
parameter!(ContainerId: Path "containerId", String, True, "The identifier of the board or the wiki.");
parameter!(BlockId: Path "blockId", String, True, "The identifier of the block.");
parameter!(CardId: Path "cardId", String, True, "The identifier of the card.");
parameter!(WikiId: Path "wikiId", String, True, "The identifier of the wiki: a board of the `kind` `wiki`. A board of another kind is 404, as an identifier of nothing is.");
parameter!(PageId: Path "pageId", String, True, "The identifier of the page: a page of the wiki of the path. 404 for a page of another wiki.");
parameter!(CommentId: Path "commentId", String, True, "The identifier of the comment: a comment on the page of the path. 404 for a comment on another page, and for a block that is no comment.");
parameter!(BodyVersionNumber: Path "version", i64, True, "The number of the version of the card's body, as `getCardBodyVersions` lists it.");
parameter!(PageVersionNumber: Path "version", i64, True, "The number of the version of the page's body, as `getPageBodyVersions` lists it.");
parameter!(CategoryId: Path "categoryId", String, True, "The identifier of the category, one of the user's own.");
parameter!(TeamId: Path "teamId", String, True, "The identifier of a team within this workspace.");
parameter!(UserId: Path "userId", String, True, "The identifier of the user.");
parameter!(InvitationId: Path "invitationId", String, True, "The identifier of the invitation, as making it and the list answer it. It is not the link's token.");
parameter!(FileId: Path "fileId", String, True, "The identifier of the file, as the upload answered it. It is no UUID: an unguessable name that ends in the file's extension.");
parameter!(SubscriberId: Path "subscriberId", String, True, "The identifier of the subscriber: the user of the session.");
parameter!(UploadId: Path "uploadId", String, True, "The identifier of the upload, as opening it answered. Only the user who opened it may use it.");
parameter!(PartNumber: Path "partNumber", i64, True, "The number of the part, from 1 to the `partCount` of the upload.");
parameter!(OrgId: Path "orgId", String, True, "The identifier of the organization. 404 unless the user of the session administers it.");
parameter!(OrgWorkspaceId: Path "workspaceId", String, True, "The identifier of a workspace of the organization. 404 for one of no other organization.");

// -- in the headers --------------------------------------------------------------------

parameter!(ContentDigest: Header "Content-Digest", String, True, "The SHA-256 digest of the part, as RFC 9530 writes it: `sha-256=:<base64 of the 32 bytes>:`. Other algorithms in the header are not looked at.");

// -- in the query ----------------------------------------------------------------------

parameter!(LoginOrg: Query "org", String, False, "The identifier of the organization the login page is for.");
parameter!(StartOrg: Query "org", String, True, "The identifier of the organization whose provider the person signs in through.");
parameter!(CallbackOrg: Path "orgId", String, True, "The organization the sign-in was started for: each has an address of its own to come back to (its provider's `redirectUri`).");
parameter!(OidcIssuer: Query "iss", String, False, "The issuer that answered (RFC 9207), if the provider says: it must be the provider's.");
parameter!(OidcState: Query "state", String, False, "The `state` the sign-in was started with, as the provider sends it back.");
parameter!(OidcCode: Query "code", String, False, "The authorization code the provider gives.");
parameter!(OidcError: Query "error", String, False, "The provider's code of why it did not let the person sign in.");
parameter!(SharedReadToken: Query "readToken", String, True, "The read token of the board, as `getSharing` answers it while the board is shared.");
parameter!(DisableNotify: Query "disableNotify", bool, False, "`true` keeps the change from being told to the subscribers of what changed. The server has nothing yet that tells subscribers, so today this changes nothing.");
parameter!(Preview: Query "preview", bool, False, "`true` gives each card its `preview`. Any other value does not, and the cards have none.");
parameter!(AsTemplate: Query "asTemplate", bool, False, "`true` makes the copy a template. Any other value does not.");
parameter!(ToWorkspace: Query "toWorkspace", String, False, "The workspace the copy goes to: one the user works in. Left out, the workspace of the path.");
parameter!(SourceBoardId: Query "sourceBoardId", String, False, "The board the blocks were copied from. The files the blocks refer to are copied from that board to this one, and the blocks point at the copies.");
parameter!(AllBlocks: Query "all", String, False, "Any value that is not empty asks for every block of the board, and the other parameters are not looked at.");
parameter!(OneBlockId: Query "blockId", String, False, "Asks for the one block with this identifier, in a list of one. 404 if the board has no such block.");
parameter!(ParentId: Query "parentId", String, False, "Narrows the list to the blocks below the block of this identifier.");
parameter!(BlockTypeFilter: Query "type", String, False, "Narrows the list to the blocks of this type, written as the blocks have it.");
parameter!(Page: Query "page", u32, False, "The page, counted from 0. Left out, the first one. Below 0 or above 2147483647 is 400.");
parameter!(PerPage: Query "perPage", u32, False, "How many a page has. Left out, 100; 0 asks for all of them. Below 0 or above 2147483647 is 400.");
parameter!(CappedPerPage: Query "perPage", u32, False, "How many a page has, at most 200. Left out or 0, 50. Below 0 or above 2147483647 is 400.");
parameter!(SearchTerm: Query "q", String, False, "What to look for. Without it nothing is found.");
parameter!(TitleTerm: Query "q", String, False, "The words to look for in titles, at most 200 characters. Without it, what was changed last is found.");
parameter!(SearchField: Query "field", String, False, "What the term is matched against: `title` (also when left out), or `property_name`, for the boards whose own `properties` object has a member named exactly as the term. Anything else is 400.");
parameter!(ContentText: Query "q", String, False, "The words to find, at most 200 characters. Without it nothing is found.");
parameter!(ContentKind: Query "kind", String, False, "The kind of thing to find: `card` or `page`. Left out, both. Anything else is 400.");
parameter!(ContentLimit: Query "limit", i64, False, "How many hits to answer with at most, from 1 to 50; 20 if left out.");
parameter!(UserSearch: Query "search", String, False, "Narrows the list to users whose name has this text in it. In the text `%` stands for any text and `_` for any one character. Whether the case of letters counts depends on the database the server runs on.");
