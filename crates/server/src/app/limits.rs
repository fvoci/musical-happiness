//! The limits of an organization, and of an account: how much of the server each may
//! use, read in one place.
//!
//! Every organization has the limits of the configuration for now. What comes to differ
//! from one organization to another (a plan, a quota) is read here, of the organization
//! the handle works in, and whoever checks such a limit asks [`App::limits`]: nothing
//! else reads one from the configuration. What the server as a whole can take (the
//! size of a request, of one part of an upload, the bytes all open uploads hold back)
//! is no organization's, and stays the configuration's.

use std::time::Duration;

use fvoci_model::PageLimits;

use super::App;

/// How much an organization may use of the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrgLimits {
    /// The largest file an upload takes, in bytes: `max_file_bytes`, which the
    /// configuration keeps positive and within what `fvoci_model::MAX_UPLOAD_PARTS`
    /// parts hold.
    pub largest_file: i64,
    /// The largest archive an import takes, in bytes: `max_import_bytes`, kept
    /// positive and within what the parts hold as well.
    pub largest_import: i64,
    /// The most invitation messages the organization's workspaces queue in an hour,
    /// all of them together: `invitation_mail_per_org_hour`, which the configuration
    /// keeps at least one request's recipients.
    pub invitation_mail_per_hour: u32,
    /// How long the provider an organization moves away from is kept, for its people
    /// to carry their accounts over: `oidc_former_provider_days`. No time keeps none.
    pub former_provider_kept: Duration,
    /// How deep the pages of a wiki go and how many a wiki has: `wiki_page_depth` and
    /// `wiki_pages`, each at least one.
    pub pages: PageLimits,
}

impl OrgLimits {
    /// The most bytes the open uploads of one user may hold back: twice the largest
    /// thing a user sends, a file or an archive.
    #[must_use]
    pub fn open_upload_bytes_of_a_user(self) -> i64 {
        self.largest_file.max(self.largest_import).saturating_mul(2)
    }
}

/// How much one account may use of the server, whatever organization it works in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccountLimits {
    /// How many organizations in use an account may have made: by registering with
    /// the name of one, and as an account that is there. The ones it was let into are
    /// not counted.
    pub organizations_made: u32,
}

/// How many organizations an account makes, until accounts have plans of their own.
const ORGANIZATIONS_AN_ACCOUNT_MAKES: u32 = 3;

impl App {
    /// The limits of an account. The same for every account for now; what comes to
    /// differ from one to another (a plan) is read here, and whoever checks such a
    /// limit asks this.
    #[must_use]
    pub fn account_limits(&self) -> AccountLimits {
        AccountLimits {
            organizations_made: ORGANIZATIONS_AN_ACCOUNT_MAKES,
        }
    }

    /// The limits of the organization this handle works in. They are the
    /// configuration's, the same for every organization, until organizations have
    /// their own: from then on whoever asks holds the handle of the organization whose
    /// limits they mean, and what the client is told before anybody signs in
    /// (`App::client_config`) is the configuration's still.
    #[must_use]
    pub fn limits(&self) -> OrgLimits {
        let config = self.config();
        OrgLimits {
            largest_file: config.max_file_bytes,
            largest_import: i64::try_from(config.max_import_bytes).unwrap_or(i64::MAX),
            invitation_mail_per_hour: config.smtp.invitation_mail_per_org_hour,
            former_provider_kept: config.oidc_former_provider_kept(),
            pages: config.wiki.page_limits(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use fvoci_model::PageLimits;

    use super::OrgLimits;

    #[test]
    fn a_user_holds_back_twice_the_largest_thing_they_send() {
        let limits = |largest_file, largest_import| OrgLimits {
            largest_file,
            largest_import,
            invitation_mail_per_hour: 1,
            former_provider_kept: Duration::ZERO,
            pages: PageLimits::default(),
        };
        assert_eq!(limits(10, 3).open_upload_bytes_of_a_user(), 20);
        assert_eq!(limits(3, 10).open_upload_bytes_of_a_user(), 20);
        assert_eq!(limits(i64::MAX, 1).open_upload_bytes_of_a_user(), i64::MAX);
    }
}
