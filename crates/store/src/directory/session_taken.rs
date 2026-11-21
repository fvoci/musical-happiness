//! Which sessions of a user open a workspace: the one rule of it, and how it is read
//! of a workspace's organization and its user.

use fvoci_model::OrgKind;

/// Which sessions of a user open what is a workspace's or its organization's: the one
/// rule of it, for a request, a websocket connection and a room alike.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionTaken {
    /// Any session of the user.
    Any,
    /// Only a session made through this provider: the organization takes no password
    /// (`sso_required`).
    Through(String),
    /// Only a session made with the account's password: the user's own space, once
    /// the account has a password. A provider is an organization's, and what a
    /// session made through one opens is what that organization's people may reach;
    /// the space is the person's alone.
    WithPassword,
}

impl SessionTaken {
    /// What an organization that people work in takes: sessions made through
    /// `required` and no other, or any if it takes a password too (`None`).
    #[must_use]
    pub fn of_organization(required: Option<String>) -> Self {
        required.map_or(Self::Any, Self::Through)
    }

    /// What the space of an account takes: a session made with its password once it
    /// has one, and any until then, as an account without a password has no other.
    /// `password` is the account's stored password hash, or any text that is empty
    /// where that is (`HAS_PASSWORD`): nothing is read of it but that.
    #[must_use]
    pub fn of_space(password: &str) -> Self {
        if password.is_empty() {
            Self::Any
        } else {
            Self::WithPassword
        }
    }

    /// Whether what is refused of a session here is for want of the account's
    /// password, and not of a provider: what the session is told, so that its person
    /// knows which way in to take.
    #[must_use]
    pub fn asks_for_password(&self) -> bool {
        matches!(self, Self::WithPassword)
    }

    /// Whether a session opens it: `made_through` is the provider the session was
    /// made through, `None` for one made with a password. A session made through
    /// another provider opens nothing of an organization that takes one: not one of
    /// another organization, and not the one the organization had before.
    #[must_use]
    pub fn opened_by(&self, made_through: Option<&str>) -> bool {
        match self {
            Self::Any => true,
            Self::Through(required) => made_through == Some(required.as_str()),
            Self::WithPassword => made_through.is_none(),
        }
    }
}

/// What a workspace takes, from what was read of it: its organization's kind, the
/// provider that organization takes sessions of and no other (empty for none), and
/// whether the user, who is a member of it, has a password ([`HAS_PASSWORD`]).
pub(super) fn taken(kind: OrgKind, required_provider: String, password: &str) -> SessionTaken {
    match kind {
        OrgKind::Personal => SessionTaken::of_space(password),
        OrgKind::Team => {
            SessionTaken::of_organization(Some(required_provider).filter(|id| !id.is_empty()))
        }
    }
}

/// The column that says whether the user `u` has a password, and nothing of it: empty
/// for none.
pub(super) const HAS_PASSWORD: &str = "CASE WHEN u.password_hash = '' THEN '' ELSE 'set' END";

#[cfg(test)]
mod tests {
    use super::*;

    /// One case of which session opens what.
    struct Row {
        case: &'static str,
        takes: SessionTaken,
        /// The provider the session was made through.
        made_through: Option<&'static str>,
        opens: bool,
    }

    fn through(provider: &str) -> SessionTaken {
        SessionTaken::of_organization(Some(provider.to_owned()))
    }

    /// The table of which session opens what: the rule of an organization that takes
    /// no password and of a space whose account has one, row by row.
    #[test]
    fn which_session_opens_what() {
        let rows = [
            Row {
                case: "a password opens an organization that takes one",
                takes: SessionTaken::of_organization(None),
                made_through: None,
                opens: true,
            },
            Row {
                case: "a provider's session opens an organization that takes a password too",
                takes: SessionTaken::of_organization(None),
                made_through: Some("ours"),
                opens: true,
            },
            Row {
                case: "a password opens nothing of an organization that takes none",
                takes: through("ours"),
                made_through: None,
                opens: false,
            },
            Row {
                case: "the session of the organization's provider opens it",
                takes: through("ours"),
                made_through: Some("ours"),
                opens: true,
            },
            Row {
                case: "the session of another organization's provider opens nothing",
                takes: through("ours"),
                made_through: Some("theirs"),
                opens: false,
            },
            Row {
                case: "the session of the provider the organization had before opens nothing",
                takes: through("ours-now"),
                made_through: Some("ours-before"),
                opens: false,
            },
            Row {
                case: "any session opens the space of an account without a password",
                takes: SessionTaken::of_space(""),
                made_through: Some("ours"),
                opens: true,
            },
            Row {
                case: "a password opens the space of an account that has one",
                takes: SessionTaken::of_space("a hash"),
                made_through: None,
                opens: true,
            },
            Row {
                case: "a provider's session opens nothing of the space of an account with a password",
                takes: SessionTaken::of_space("a hash"),
                made_through: Some("ours"),
                opens: false,
            },
        ];
        for row in rows {
            assert_eq!(
                row.takes.opened_by(row.made_through),
                row.opens,
                "{}",
                row.case
            );
        }
    }

    #[test]
    fn a_space_takes_no_provider_and_an_organization_no_password() {
        assert_eq!(
            taken(OrgKind::Personal, "a-provider".to_owned(), "set"),
            SessionTaken::WithPassword
        );
        assert_eq!(
            taken(OrgKind::Personal, String::new(), ""),
            SessionTaken::Any
        );
        assert_eq!(
            taken(OrgKind::Team, "a-provider".to_owned(), "set"),
            through("a-provider")
        );
        assert_eq!(
            taken(OrgKind::Team, String::new(), "set"),
            SessionTaken::Any
        );
    }
}
