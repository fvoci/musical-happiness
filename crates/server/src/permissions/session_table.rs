//! The table of which session opens what is an organization's: the rule of an
//! organization that takes no password, row by row.

use super::session_opens;

/// One case of which session opens what is an organization's.
struct SessionRow {
    case: &'static str,
    /// The provider the organization takes sessions of and no other.
    required: Option<&'static str>,
    /// The provider the session was made through.
    made_through: Option<&'static str>,
    opens: bool,
}

const SESSION_ROWS: &[SessionRow] = &[
    SessionRow {
        case: "a password opens an organization that takes one",
        required: None,
        made_through: None,
        opens: true,
    },
    SessionRow {
        case: "a provider's session opens an organization that takes a password too",
        required: None,
        made_through: Some("ours"),
        opens: true,
    },
    SessionRow {
        case: "a password opens nothing of an organization that takes none",
        required: Some("ours"),
        made_through: None,
        opens: false,
    },
    SessionRow {
        case: "the session of the organization's provider opens it",
        required: Some("ours"),
        made_through: Some("ours"),
        opens: true,
    },
    SessionRow {
        case: "the session of another organization's provider opens nothing",
        required: Some("ours"),
        made_through: Some("theirs"),
        opens: false,
    },
    SessionRow {
        case: "the session of the provider the organization had before opens nothing",
        required: Some("ours-now"),
        made_through: Some("ours-before"),
        opens: false,
    },
];

#[test]
fn which_session_opens_an_organization() {
    for row in SESSION_ROWS {
        assert_eq!(
            session_opens(row.required, row.made_through),
            row.opens,
            "{}",
            row.case
        );
    }
}
