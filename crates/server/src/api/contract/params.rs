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

parameter!(TeamId: Path "teamId", String, True, "The identifier of the team.");
parameter!(BoardId: Path "boardId", String, True, "The identifier of the board.");
parameter!(BlockId: Path "blockId", String, True, "The identifier of the block.");
parameter!(CardId: Path "cardId", String, True, "The identifier of the card.");
parameter!(CategoryId: Path "categoryId", String, True, "The identifier of the category, one of the user's own.");
parameter!(UserId: Path "userId", String, True, "The identifier of the user.");
parameter!(FileId: Path "fileId", String, True, "The identifier of the file, as the upload answered it.");
parameter!(SubscriberId: Path "subscriberId", String, True, "The identifier of the subscriber: the user of the session.");
parameter!(MoveWhere: Path "where", String, True, "Where the block goes: `before` or `after` the other one.");
parameter!(DestinationBlockId: Path "dstBlockId", String, True, "The identifier of the block the moved one is put next to.");

// -- in the query ----------------------------------------------------------------------

parameter!(ReadToken: Query "readToken", String, False, "The read token of the board. With the token of a board that is shared, the request needs no session.");
parameter!(DisableNotify: Query "disableNotify", bool, False, "`true` keeps the change from being told to the subscribers of what changed. The server has nothing yet that tells subscribers, so today this changes nothing.");
parameter!(AsTemplate: Query "asTemplate", bool, False, "`true` makes the copy a template. Any other value does not.");
parameter!(ToTeam: Query "toTeam", String, False, "The team the copy goes to. Left out, it stays in the team of the board.");
parameter!(SourceBoardId: Query "sourceBoardId", String, False, "The board the blocks were copied from. The files the blocks refer to are copied from that board to this one, and the blocks point at the copies.");
parameter!(AllBlocks: Query "all", String, False, "Any value asks for every block of the board, and the other parameters are not looked at.");
parameter!(OneBlockId: Query "blockId", String, False, "Asks for the one block with this identifier.");
parameter!(ParentId: Query "parentId", String, False, "Narrows the list to the blocks with this parent.");
parameter!(BlockTypeFilter: Query "type", String, False, "Narrows the list to the blocks of this type.");
parameter!(Page: Query "page", i64, False, "The page, counted from 0. Left out, the first one.");
parameter!(PerPage: Query "perPage", i64, False, "How many cards a page has. Left out, 100. Zero or less asks for all cards.");
parameter!(SearchTerm: Query "q", String, False, "What to look for. Without it nothing is found.");
parameter!(SearchField: Query "field", String, False, "What the term is matched against: `title` (also when left out) or `property_name`, the names of card properties.");
parameter!(UserSearch: Query "search", String, False, "Narrows the list to users whose name has this text in it.");
