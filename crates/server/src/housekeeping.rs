//! The reach of the server's housekeeping: the one place of the server that asks for
//! the rows of every organization.

use fvoci_store::Tenant;

use crate::app::App;

/// The server as its housekeeping uses it: reaching the rows of every organization.
/// What the server throws away on nobody's behalf (sessions, uploads and temporary
/// files that ended, what was in the trash too long) is looked for in all of them; as
/// one organization, a clean-up would take what it does not see of the others for gone.
///
/// Nothing that answers a request works through this.
pub(crate) fn reach(app: &App) -> App {
    app.scoped(Tenant::System)
}
