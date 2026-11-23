//! The path and query parameters of the API, as the contract describes them.
//!
//! The handlers read their parameters themselves; these types only say, in a handler's
//! `params(...)`, which ones it reads.

use utoipa::openapi::path::{Parameter, ParameterBuilder, ParameterIn};
use utoipa::openapi::schema::{KnownFormat, ObjectBuilder, Schema, SchemaFormat, Type};
use utoipa::openapi::{RefOr, Required};
use utoipa::{IntoParams, PartialSchema};

use crate::app::MAX_PER_PAGE;

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

/// The schema of `perPage`: a whole number from 1 to [`MAX_PER_PAGE`].
struct PerPageCount;

impl PartialSchema for PerPageCount {
    fn schema() -> RefOr<Schema> {
        ObjectBuilder::new()
            .schema_type(Type::Integer)
            .format(Some(SchemaFormat::KnownFormat(KnownFormat::Int32)))
            .minimum(Some(1))
            .maximum(Some(MAX_PER_PAGE))
            .into()
    }
}

// -- in the path -----------------------------------------------------------------------

parameter!(WorkspaceId: Path "workspaceId", String, True, "The identifier of the workspace. 404 unless the user of the session works in it, a member who is no guest.");
parameter!(WorkerWorkspaceId: Path "workspaceId", String, True, "The identifier of the workspace. 404 unless the user of the session works in it, a member who is no guest.");
parameter!(PeopleWorkspaceId: Path "workspaceId", String, True, "The identifier of the workspace. 404 unless the user of the session works in it, a member who is no guest; and 404 for the workspace of the user's own space, which has no people to invite, to group, to give another role or to take out.");
parameter!(MemberWorkspaceId: Path "workspaceId", String, True, "The identifier of the workspace. 404 unless the user of the session is a member of it, a guest too.");
parameter!(BoardId: Path "boardId", String, True, "The identifier of the board.");
parameter!(ContainerId: Path "containerId", String, True, "The identifier of the board, the wiki or the diary.");
parameter!(PeopleContainerId: Path "containerId", String, True, "The identifier of the board or the wiki. A diary is 404, as an identifier of nothing is: it is its owner's alone, with nobody to put on it, no link, and no copy.");
parameter!(OwnWorkspaceId: Path "workspaceId", String, True, "The identifier of the workspace of the user's own space. 404 for any other workspace, which has no diaries, as for one the user of the session is no member of.");
parameter!(DiaryId: Path "diaryId", String, True, "The identifier of the diary: a board of the `kind` `diary`. A board of another kind is 404, as an identifier of nothing is.");
parameter!(EntryId: Path "entryId", String, True, "The identifier of the entry: an entry of the diary of the path. 404 for an entry of another diary.");
parameter!(MonthDayOfYear: Path "monthDay", String, True, "A month and a day without a year, as `11-21`: two digits each. `02-29` is a day of its own, which only leap years have. 400 for anything else.");
parameter!(EntryVersionNumber: Path "version", i64, True, "The number of the version of the entry's body, as `getDiaryEntryBodyVersions` lists it.");
parameter!(BlockId: Path "blockId", String, True, "The identifier of the block.");
parameter!(CardId: Path "cardId", String, True, "The identifier of the card.");
parameter!(WikiId: Path "wikiId", String, True, "The identifier of the wiki: a board of the `kind` `wiki`. A board of another kind is 404, as an identifier of nothing is.");
parameter!(PageId: Path "pageId", String, True, "The identifier of the page: a page of the wiki of the path. 404 for a page of another wiki.");
parameter!(CommentId: Path "commentId", String, True, "The identifier of the comment: a comment on the page of the path. 404 for a comment on another page, and for a block that is no comment.");
parameter!(CardNumber: Path "number", i64, True, "The number of the card on the board, as `Card.number` gives it. One that no card of the board has, and what is no number, is 404.");
parameter!(BodyVersionNumber: Path "version", i64, True, "The number of the version of the card's body, as `getCardBodyVersions` lists it.");
parameter!(PageVersionNumber: Path "version", i64, True, "The number of the version of the page's body, as `getPageBodyVersions` lists it.");
parameter!(FileVersionNumber: Path "version", i64, True, "The number of the version of the file, as `getFileVersions` lists it.");
parameter!(LeaseId: Path "leaseId", String, True, "The lease of the file, as `holdFileLease` gave it.");
parameter!(CategoryId: Path "categoryId", String, True, "The identifier of the category, one of the user's own.");
parameter!(TeamId: Path "teamId", String, True, "The identifier of a team within this workspace.");
parameter!(UserId: Path "userId", String, True, "The identifier of the user.");
parameter!(InvitationId: Path "invitationId", String, True, "The identifier of the invitation, as making it and the list answer it. It is not the link's token.");
parameter!(FileId: Path "fileId", String, True, "The identifier of the file, as the upload answered it. It is no UUID: an unguessable name that ends in the file's extension.");
parameter!(NotificationId: Path "notificationId", String, True, "The identifier of the notification: one of the user of the session.");
parameter!(SubscriberId: Path "subscriberId", String, True, "The identifier of the subscriber: the user of the session.");
parameter!(UploadId: Path "uploadId", String, True, "The identifier of the upload, as opening it answered. Only the user who opened it may use it.");
parameter!(PartNumber: Path "partNumber", i64, True, "The number of the part, from 1 to the `partCount` of the upload.");
parameter!(OrgId: Path "orgId", String, True, "The identifier of the organization. 404 unless the user of the session administers it; and 404 for the user's own space, which nobody administers.");
parameter!(MemberOrgId: Path "orgId", String, True, "The identifier of an organization that people work in together. 404 unless the user of the session is a member of it (a guest of its workspaces is none); and 404 for the user's own space, which nobody is a member of.");
parameter!(OrgWorkspaceId: Path "workspaceId", String, True, "The identifier of a workspace of the organization. 404 for one of no other organization.");

// -- in the headers --------------------------------------------------------------------

parameter!(AcceptLanguage: Header "Accept-Language", String, False, "The languages the reader takes, as RFC 9110 writes them (`ko-KR, ko;q=0.9, en;q=0.8`). Read where the user's settings name no language the server writes in: the range of the highest `q` that names English or Korean decides, the first of them for equal `q`, and `*` is English.");
parameter!(ContentDigest: Header "Content-Digest", String, True, "The SHA-256 digest of the part, as RFC 9530 writes it: `sha-256=:<base64 of the 32 bytes>:`. Other algorithms in the header are not looked at.");

// -- in the query ----------------------------------------------------------------------

parameter!(LoginOrg: Query "org", String, False, "The identifier of the organization the login page is for. A user's own space has no login page: its identifier is that of no organization here.");
parameter!(StartOrg: Query "org", String, True, "The identifier of the organization whose provider the person signs in through. A user's own space has no provider.");
parameter!(CallbackOrg: Path "orgId", String, True, "The organization the sign-in was started for: each has an address of its own to come back to (its provider's `redirectUri`).");
parameter!(OidcIssuer: Query "iss", String, False, "The issuer that answered (RFC 9207), if the provider says: it must be the provider's.");
parameter!(OidcState: Query "state", String, False, "The `state` the sign-in was started with, as the provider sends it back.");
parameter!(OidcCode: Query "code", String, False, "The authorization code the provider gives.");
parameter!(OidcError: Query "error", String, False, "The provider's code of why it did not let the person sign in.");
parameter!(SharedReadToken: Query "readToken", String, True, "The read token of the board, as `getSharing` answers it while the board is shared.");
parameter!(DisableNotify: Query "disableNotify", bool, False, "`true` keeps the change from being told to anybody: the people it newly mentions, or gives a card to, get no notification of it.");
parameter!(Preview: Query "preview", bool, False, "`true` gives each card its `preview`. Any other value does not, and the cards have none.");
parameter!(AsTemplate: Query "asTemplate", bool, False, "`true` makes the copy a template. Any other value does not.");
parameter!(ToWorkspace: Query "toWorkspace", String, False, "The workspace the copy goes to: one the user works in. Left out, the workspace of the path.");
parameter!(SourceBoardId: Query "sourceBoardId", String, False, "The board the blocks were copied from. The files the blocks refer to are copied from that board to this one, and the blocks point at the copies. Such blocks are copies: the people they name are told nothing of them.");
parameter!(AllBlocks: Query "all", String, False, "Any value that is not empty asks for every block of the board, and the other parameters are not looked at.");
parameter!(OneBlockId: Query "blockId", String, False, "Asks for the one block with this identifier, in a list of one. 404 if the board has no such block.");
parameter!(ParentId: Query "parentId", String, False, "Narrows the list to the blocks below the block of this identifier.");
parameter!(BlockTypeFilter: Query "type", String, False, "Narrows the list to the blocks of this type, written as the blocks have it.");
parameter!(ReadThrough: Query "through", i64, False, "The time, in milliseconds, up to which the notifications are read: those whose latest happened later (`updateAt`) stay unread. Left out, now.");
parameter!(Page: Query "page", u32, False, "The page, counted from 0. Left out, the first one. Below 0 or above 2147483647 is 400.");
parameter!(PerPage: Query "perPage", PerPageCount, False, "How many a page has, from 1 to 1000. Left out, 50. Anything else is 400: a longer list is read page after page.");
parameter!(SearchTerm: Query "q", String, False, "What to look for. Without it nothing is found.");
parameter!(TitleTerm: Query "q", String, False, "The words to look for in titles, at most 200 characters. Without it, what was changed last is found.");
parameter!(SearchField: Query "field", String, False, "What the term is matched against: `title` (also when left out), or `property_name`, for the boards whose own `properties` object has a member named exactly as the term. Anything else is 400.");
parameter!(ContentText: Query "q", String, False, "The words to find, at most 200 characters. Without it nothing is found.");
parameter!(ContentKind: Query "kind", String, False, "The kind of thing to find: `card`, `page` or `entry`. Left out, every kind. Anything else is 400.");
parameter!(DaysFrom: Query "from", String, True, "The first day to list, as `2026-11-01`. 400 for what is no day, and where it is left out.");
parameter!(DaysTo: Query "to", String, True, "The last day to list, as `2026-11-30`, at most 365 days after the first. 400 for what is no day, and where it is left out.");
parameter!(ContentLimit: Query "limit", i64, False, "How many hits to answer with at most, from 1 to 50; 20 if left out.");
parameter!(UserSearch: Query "search", String, False, "Narrows the list to users whose name has this text in it. In the text `%` stands for any text and `_` for any one character. Whether the case of letters counts depends on the database the server runs on.");
