//! Workspaces: the workspaces of an organization.
//!
//! A statement here names the organization itself: the handle's
//! ([`Tenant::Org`](crate::Tenant::Org)). SQLite has no policy that would keep the
//! workspaces of the system organization out of an answer.

use fvoci_model::{Board, BoardType, GuestInvitationsChange, Workspace};

use crate::db::{Bound, Id, OptId, Param, Row, Scope, Sql, now_millis};
use crate::error::StoreError;
use crate::{Store, boards, members};

const WORKSPACE_COLUMNS: &str = "id, name, settings, modified_by, update_at, guest_invitations";

impl Store {
    /// Stores a new workspace of the handle's organization, with the current time as its
    /// creation and modification time.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NoOrganization`] if the handle names no organization,
    /// [`StoreError::InvalidId`] if the workspace's identifier is not one, and
    /// [`StoreError::Database`] if the statement fails, as it does when a workspace with the
    /// identifier exists.
    pub async fn create_workspace(&self, workspace: &Workspace) -> Result<(), StoreError> {
        self.with_conn(async |scope| {
            let org = scope.org()?;
            insert_workspace(scope, workspace, org).await
        })
        .await
    }

    /// Turns the invitations of guests to the boards of a workspace on or off, records
    /// who did it, and returns the workspace as it is then.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if the organization has no such workspace in
    /// use.
    pub async fn set_guest_invitations(
        &self,
        workspace_id: &str,
        change: GuestInvitationsChange,
        modified_by: &str,
    ) -> Result<Workspace, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql("UPDATE workspaces SET guest_invitations = ");
            sql.bind(change.enabled)
                .push_bind(", modified_by = ", OptId(modified_by))
                .push_bind(", update_at = ", now_millis())
                .push_bind(" WHERE id = ", Id(workspace_id))
                .push_bind(" AND org_id = ", scope.org()?)
                .push_bind(" AND delete_at = ", 0_i64)
                .push(&format!(" RETURNING {WORKSPACE_COLUMNS}"));
            let row = scope
                .fetch_optional(sql)
                .await?
                .ok_or_else(workspace_not_found)?;
            workspace_from_row(&row)
        })
        .await
    }

    /// Returns the workspace with the given identifier.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if the organization has no such workspace.
    pub async fn get_workspace(&self, id: &str) -> Result<Workspace, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = select_workspaces(scope)?;
            sql.push_bind(" AND id = ", Id(id));
            let row = scope
                .fetch_optional(sql)
                .await?
                .ok_or_else(workspace_not_found)?;
            workspace_from_row(&row)
        })
        .await
    }

    /// Returns every workspace of the organization, oldest first. The first is the root
    /// workspace.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn get_workspaces(&self) -> Result<Vec<Workspace>, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = select_workspaces(scope)?;
            sql.push(" ORDER BY create_at, id");
            scope
                .fetch_all(sql)
                .await?
                .iter()
                .map(workspace_from_row)
                .collect()
        })
        .await
    }
}

/// Stores a new workspace of the organization `org`.
pub(crate) async fn insert_workspace(
    scope: &mut Scope<'_>,
    workspace: &Workspace,
    org: impl Into<Bound>,
) -> Result<(), StoreError> {
    let now = now_millis();
    let mut sql = scope.sql(
        "INSERT INTO workspaces (id, org_id, name, settings, modified_by, guest_invitations, \
         create_at, update_at) VALUES (",
    );
    sql.bind(Id(&workspace.id))
        .push_bind(", ", org)
        .push_bind(", ", &workspace.title)
        .push_bind(", ", Param::json(&workspace.settings)?)
        .push_bind(", ", OptId(&workspace.modified_by))
        .push_bind(", ", workspace.guest_invitations)
        .push_bind(", ", now)
        .push_bind(", ", now)
        .push(")");
    scope.execute(sql).await?;
    Ok(())
}

/// Stores a new organization under the identifier `org` and the given name, with
/// `workspace` as its first workspace, which is its default one. Nobody is a member of
/// either yet.
pub(crate) async fn insert_organization(
    scope: &mut Scope<'_>,
    org: impl Into<Bound>,
    name: &str,
    workspace: &Workspace,
) -> Result<(), StoreError> {
    let org: Bound = org.into();
    let now = now_millis();
    let mut sql =
        scope.sql("INSERT INTO organizations (id, name, settings, create_at, update_at) VALUES (");
    sql.bind(org.clone())
        .push_bind(", ", name)
        .push(", '{}'")
        .push_bind(", ", now)
        .push_bind(", ", now)
        .push(")");
    scope.execute(sql).await?;
    insert_workspace(scope, workspace, org.clone()).await?;
    let mut sql = scope.sql("UPDATE organizations SET default_workspace_id = ");
    sql.bind(Id(&workspace.id)).push_bind(" WHERE id = ", org);
    scope.execute(sql).await?;
    Ok(())
}

fn workspace_not_found() -> StoreError {
    StoreError::not_found("workspace")
}

/// The workspaces of the handle's organization that are in use.
fn select_workspaces(scope: &Scope<'_>) -> Result<Sql, StoreError> {
    let mut sql = scope.sql(&format!(
        "SELECT {WORKSPACE_COLUMNS} FROM workspaces WHERE org_id = "
    ));
    sql.bind(scope.org()?).push_bind(" AND delete_at = ", 0_i64);
    Ok(sql)
}

fn workspace_from_row(row: &Row) -> Result<Workspace, StoreError> {
    Ok(Workspace {
        id: row.id(0)?,
        title: row.text(1)?,
        settings: row.json_map(2, "settings")?,
        modified_by: row.id(3)?,
        update_at: row.int(4)?,
        guest_invitations: row.bool(5)?,
    })
}

impl Store {
    /// Returns the boards of a workspace whose title matches a search term and that are open
    /// or private with the user as a member with a role, oldest first. Templates are left out.
    ///
    /// An empty `term` matches every board; otherwise the title must contain every
    /// space-separated word of the term, ignoring case.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn search_boards_for_user_in_workspace(
        &self,
        workspace_id: &str,
        term: &str,
        user_id: &str,
    ) -> Result<Vec<Board>, StoreError> {
        self.with_conn(async |scope| {
            search_boards_for_user_in_workspace(scope, workspace_id, term, user_id).await
        })
        .await
    }

    /// Returns every board of a workspace that is no template, whoever is a member of
    /// it, oldest first: what the workspace's administrators list.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn get_boards_of_workspace(
        &self,
        workspace_id: &str,
    ) -> Result<Vec<Board>, StoreError> {
        self.with_conn(async |scope| every_board(scope, workspace_id, Kind::Boards).await)
            .await
    }

    /// Returns every template of a workspace, whoever is a member of it, oldest first:
    /// what the workspace's administrators list of its templates. The built-in templates
    /// are not among them.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn get_templates_of_workspace(
        &self,
        workspace_id: &str,
    ) -> Result<Vec<Board>, StoreError> {
        self.with_conn(async |scope| every_board(scope, workspace_id, Kind::Templates).await)
            .await
    }
}

/// Which boards of a workspace [`every_board`] reads.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// The boards that are no template.
    Boards,
    /// The templates.
    Templates,
}

/// Every board of the workspace of the kind, oldest first.
async fn every_board(
    scope: &mut Scope<'_>,
    workspace_id: &str,
    kind: Kind,
) -> Result<Vec<Board>, StoreError> {
    let mut sql = scope.sql(&format!(
        "SELECT {} FROM boards AS b WHERE b.is_template = ",
        boards::board_select_list("b.")
    ));
    sql.bind(kind == Kind::Templates)
        .push_bind(" AND b.workspace_id = ", Id(workspace_id))
        .push(boards::OLDEST_BOARD_FIRST);
    boards::fetch_boards(scope, sql).await
}

async fn search_boards_for_user_in_workspace(
    scope: &mut Scope<'_>,
    workspace_id: &str,
    term: &str,
    user_id: &str,
) -> Result<Vec<Board>, StoreError> {
    let mut sql = scope.sql(&format!(
        "SELECT DISTINCT {} FROM boards AS b LEFT JOIN board_members AS bm ON b.id = bm.board_id \
         WHERE b.is_template = ",
        boards::board_select_list("b.")
    ));
    sql.bind(false)
        .push_bind(" AND b.workspace_id = ", Id(workspace_id))
        .push_bind(" AND (b.type = ", BoardType::Open.as_str())
        .push_bind(" OR (b.type = ", BoardType::Private.as_str())
        .push_bind(" AND bm.user_id = ", Id(user_id))
        .push(members::BM_HAS_A_ROLE)
        .push("))");
    sql.push(boards::OLDEST_BOARD_FIRST);
    Ok(boards::with_title_words(
        boards::fetch_boards(scope, sql).await?,
        term,
    ))
}
