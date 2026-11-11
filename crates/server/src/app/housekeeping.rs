//! The reach of the server's housekeeping: the one place of the server that asks for
//! the rows of every organization.

use fvoci_store::Tenant;

use super::App;

impl App {
    /// The server as its housekeeping uses it: reaching the rows of every organization.
    /// What the server throws away on nobody's behalf (sessions, uploads and temporary
    /// files that ended, what was in the trash too long) is looked for in all of them;
    /// as one organization, a clean-up would take what it does not see of the others
    /// for gone.
    ///
    /// Each job of the housekeeping takes this reach itself, whatever handle it is
    /// started with. Nothing that answers a request works through this.
    #[must_use]
    pub fn housekeeping(&self) -> Self {
        self.scoped(Tenant::System)
    }
}
