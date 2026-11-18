//! What the model refuses of a block, a relation or a card a client sent, as the
//! problems the API answers with.

use fvoci_model::{BlockError, CardError, RELATION_LIMIT, RelationError, RelationProblem};

use super::{AppError, Reason};

impl From<BlockError> for AppError {
    fn from(error: BlockError) -> Self {
        match error {
            BlockError::TitleSizeLimitExceeded
            | BlockError::FieldsSizeLimitExceeded
            | BlockError::InvalidCardProperties(_) => Self::bad_request(error),
            BlockError::BodyNotAllowed
            | BlockError::BodyFileNotOfContainer
            | BlockError::FileBlockInComment => Self::bad_request(error).with_field("body"),
            BlockError::NulInTitle => Self::bad_request(error).with_field("title"),
            BlockError::InvalidCardField { .. } | BlockError::InvalidPageField { .. } => {
                Self::bad_request(error).with_field("fields")
            }
            BlockError::AfterNotSibling => Self::bad_request(error).with_field("after"),
            BlockError::PageTooDeep { limit } => Self::bad_request(error)
                .with_reason(Reason::PageTooDeep)
                .with_limit(u64::from(limit))
                .with_field("parentId"),
            BlockError::TooManyPages { limit } => Self::bad_request(error)
                .with_reason(Reason::TooManyPages)
                .with_limit(u64::from(limit)),
            BlockError::InvalidRelation(error) => Self::from(error),
            BlockError::TypeNotCurrent
            | BlockError::TypeStrandsComments
            | BlockError::PageByBlocks
            | BlockError::PageOffWiki
            | BlockError::NotPageOnWiki => Self::bad_request(error).with_field("type"),
            BlockError::ParentNotOnContainer
            | BlockError::ParentBelowBlock
            | BlockError::CommentNotOnCardOrPage
            | BlockError::ParentIsComment
            | BlockError::PageNotBelowPage => Self::bad_request(error).with_field("parentId"),
            BlockError::EmptyContainerId => Self::from(anyhow::Error::from(error)),
        }
    }
}

impl From<RelationError> for AppError {
    /// A bad request on `fields` (where a card's property values are), with the reason
    /// of the problem: the words name the property, and no card.
    fn from(error: RelationError) -> Self {
        let reason = match error.problem {
            RelationProblem::NotAList => Reason::RelationNotAList,
            RelationProblem::ToItself => Reason::RelationToItself,
            RelationProblem::Repeated => Reason::RelationRepeated,
            RelationProblem::TooMany => Reason::RelationTooMany,
            RelationProblem::NotACard => Reason::RelationNotACard,
            RelationProblem::CardNotFound => Reason::RelationCardNotFound,
        };
        let refused = Self::bad_request(&error).with_reason(reason);
        let refused = if error.problem == RelationProblem::TooMany {
            refused.with_limit(u64::try_from(RELATION_LIMIT).unwrap_or(u64::MAX))
        } else {
            refused
        };
        refused.with_field("fields")
    }
}

impl From<CardError> for AppError {
    fn from(error: CardError) -> Self {
        match error {
            // What a client sent.
            CardError::BoardIdMismatch | CardError::Invalid(_) => Self::bad_request(error),
            CardError::ReadAtMissing => Self::bad_request(error).with_field("readAt"),
            // What is stored cannot be read as a card.
            CardError::NotCardBlock | CardError::InvalidFieldType { .. } => {
                Self::from(anyhow::Error::from(error))
            }
        }
    }
}
