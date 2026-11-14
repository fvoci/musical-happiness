//! Registrations whose email address is not confirmed yet: no account, nobody's, in no
//! organization, until the link of their mail confirms them; removed seven days after
//! they were made if it never does.

use fvoci_model::User;

use crate::accounts::{self, Founding};
use crate::db::{Dialect, Id, OptId, Row, Scope, Sql};
use crate::error::{StoreError, Taken};
use crate::{Store, users};

/// A registration waits seven days for its address to be confirmed, in milliseconds.
pub const SIGNUP_LIFETIME_MILLIS: i64 = 7 * 24 * 60 * 60 * 1_000;

/// The link of a registration's mail confirms it for a day, in milliseconds.
pub const SIGNUP_LINK_LIFETIME_MILLIS: i64 = 24 * 60 * 60 * 1_000;

/// The key of the lock that account names are claimed under on PostgreSQL: an account
/// and a registration are two tables, which no unique index holds together.
const ACCOUNT_NAMES_LOCK: i64 = 0x6676_6f63_6e61_6d65;

/// A registration whose email address is not confirmed yet.
#[derive(Clone, PartialEq, Eq)]
pub struct Signup {
    /// The account's identifier once it is confirmed.
    pub id: String,
    /// The login name the account will have.
    pub username: String,
    /// The address to be confirmed.
    pub email: String,
    /// The name to be shown by; empty for none.
    pub display_name: String,
    /// An Argon2id PHC string.
    pub password_hash: String,
    /// The language tag of the screen it was made from, for its mail.
    pub language: String,
    /// What it starts once it is confirmed.
    pub start: SignupStart,
    /// The digest of the secret kept by the cookie of the browser it was made from;
    /// empty for one made before browsers were told apart.
    pub browser_hash: String,
    /// When it was made.
    pub create_at: i64,
    /// When it ends unconfirmed, and is removed.
    pub expires_at: i64,
}

impl std::fmt::Debug for Signup {
    /// Leaves out the address and the password hash.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Signup")
            .field("id", &self.id)
            .field("start", &self.start)
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

impl Signup {
    /// The account it becomes, not stored yet.
    #[must_use]
    pub fn to_account(&self) -> User {
        User {
            id: self.id.clone(),
            username: self.username.clone(),
            email: self.email.clone(),
            display_name: self.display_name.clone(),
            password_hash: self.password_hash.clone(),
            ..User::default()
        }
    }
}

/// What a registration starts once it is confirmed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignupStart {
    /// An organization of this name, with its first workspace of the same name.
    Organization(String),
    /// The link of an invitation it was made by, which counts it as a use: in what it
    /// lets in once the registration is confirmed, and given back if it is not.
    Invitation {
        /// The link.
        id: String,
        /// Its organization.
        org_id: String,
    },
}

/// How a registration is found to be confirmed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Proof<'a> {
    /// By the digest of the link of its mail, while the link is live.
    Link(&'a str),
    /// By the server's operator, by its user name, in any letter case.
    Operator(&'a str),
}

/// What a login names a registration by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignupLookup<'a> {
    /// Its user name, in any letter case.
    Username(&'a str),
    /// Its address, in any letter case.
    Email(&'a str),
}

/// A confirmed registration: the account it became, and where it was let in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirmed {
    /// The account, as it was stored.
    pub user: User,
    /// The organization it started or joined.
    pub org_id: String,
    /// The workspace the account first sees.
    pub workspace_id: String,
    /// For a guest's link: the board it was let onto.
    pub board_id: Option<String>,
    /// Whether it started its organization, or joined one by a link.
    pub started_organization: bool,
}

/// The columns of a registration, in the order [`signup_from_row`] reads them.
const COLUMNS: &str = "id, username, email, display_name, password_hash, language, \
    organization_name, create_at, expires_at, invitation_id, invitation_org_id, browser_hash";

impl Store {
    /// Stores a registration whose address is to be confirmed, holding its name and
    /// address. The registrations that ended by `now` go first, and so does the one
    /// made before from the same browser, whose marker has the digest `replacing`: its
    /// name and address are free for this one, and the use of a link it was made by is
    /// given back. All of it happens or none: a registration that is refused replaces
    /// nothing.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Conflict`] if an account, deleted or not, or another
    /// registration that has not ended has the name or the address, in any letter
    /// case; and [`StoreError::Database`] if a statement fails.
    pub async fn create_signup(
        &self,
        signup: &Signup,
        replacing: Option<&str>,
        now: i64,
    ) -> Result<(), StoreError> {
        self.system()
            .transaction(async |scope| {
                remove_ended(scope, now).await?;
                if let Some(browser) = replacing {
                    discard_of_browser(scope, browser).await?;
                }
                hold_names(scope, signup).await
            })
            .await
    }

    /// The registration `proof` names, if it has not ended by `now`, nor its link.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn signup_of(
        &self,
        proof: Proof<'_>,
        now: i64,
    ) -> Result<Option<Signup>, StoreError> {
        self.with_conn(async |scope| find(scope, proof, now).await)
            .await
    }

    /// The registration a login names, if it has not ended by `now`.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn live_signup(
        &self,
        lookup: SignupLookup<'_>,
        now: i64,
    ) -> Result<Option<Signup>, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql(&format!("SELECT {COLUMNS} FROM signups WHERE "));
            match lookup {
                SignupLookup::Username(name) => sql.push_bind("lower(username) = lower(", name),
                SignupLookup::Email(email) => sql.push_bind("lower(email) = lower(", email),
            };
            sql.push_bind(") AND expires_at > ", now);
            scope
                .fetch_optional(sql)
                .await?
                .as_ref()
                .map(signup_from_row)
                .transpose()
        })
        .await
    }

    /// Gives the registration `signup_id` a new link that confirms it for a day from
    /// `now`, under the digest `digest`: the link it had before confirms nothing any
    /// more. Says whether it did; not for a registration that is gone or ended.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the statement fails.
    pub async fn renew_signup_link(
        &self,
        signup_id: &str,
        digest: &str,
        now: i64,
    ) -> Result<bool, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql("UPDATE signups SET token_hash = ");
            sql.bind(digest)
                .push_bind(
                    ", token_expires_at = ",
                    now.saturating_add(SIGNUP_LINK_LIFETIME_MILLIS),
                )
                .push_bind(" WHERE id = ", Id(signup_id))
                .push_bind(" AND expires_at > ", now);
            Ok(scope.execute(sql).await? == 1)
        })
        .await
    }

    /// Confirms the registration `proof` names at `now`: makes its account, under its
    /// identifier, and starts its organization as `founding` says, the account its
    /// administrator and of its first workspace, as [`Store::sign_up`] does; and
    /// removes the registration. All of it happens or none. Of two confirmations at
    /// once, one confirms and the other finds nothing.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFirstUser`] if [`Founding::First`] is asked and the
    /// instance has had its first user, and the registration stays; otherwise as
    /// [`Store::sign_up`].
    pub async fn confirm_signup(
        &self,
        signup: &Signup,
        proof: Proof<'_>,
        founding: Founding,
        now: i64,
    ) -> Result<Option<Confirmed>, StoreError> {
        let SignupStart::Organization(name) = &signup.start else {
            return Err(StoreError::not_found("registration of its own"));
        };
        let existing = match founding {
            Founding::First => self.instance_organization().await?,
            Founding::New => None,
        };
        let place = accounts::OrgPlace::of(existing.as_deref());
        let org_id = existing.unwrap_or_else(fvoci_model::new_id);
        self.for_org(&org_id)?
            .transaction(async |scope| {
                if !take(scope, &signup.id, proof, now).await? {
                    return Ok(None);
                }
                let mut user = signup.to_account();
                let workspace_id = accounts::found(scope, &mut user, name, founding, place).await?;
                Ok(Some(Confirmed {
                    user,
                    org_id: org_id.clone(),
                    workspace_id,
                    board_id: None,
                    started_organization: true,
                }))
            })
            .await
    }

    /// Removes the registration `proof` names at `now`, unconfirmed: its name and
    /// address are free again, and the use of a link it was made by is given back.
    /// Says whether there was one.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a statement fails.
    pub async fn discard_signup(&self, proof: Proof<'_>, now: i64) -> Result<bool, StoreError> {
        self.system()
            .transaction(async |scope| {
                let Some(signup) = find(scope, proof, now).await? else {
                    return Ok(false);
                };
                // The link before the registration, as a registration by it is made;
                // the use goes back only with the registration that took it.
                hold_link_of(scope, &signup).await?;
                if !take(scope, &signup.id, proof, now).await? {
                    return Ok(false);
                }
                give_back_use(scope, &signup).await?;
                Ok(true)
            })
            .await
    }

    /// Whether the link of the registration `signup_id` is still the one of `digest`,
    /// and confirms it at `now`: a newer mail, or a confirmation, ends it.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn signup_link_stands(
        &self,
        signup_id: &str,
        digest: &str,
        now: i64,
    ) -> Result<bool, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql("SELECT 1 FROM signups WHERE id = ");
            sql.bind(Id(signup_id)).push(" AND ");
            push_proof(&mut sql, Proof::Link(digest), now);
            sql.push_bind(" AND expires_at > ", now);
            Ok(scope.fetch_optional(sql).await?.is_some())
        })
        .await
    }

    /// Removes every registration that would start an organization: on a server of one
    /// organization once it has its first user, none of them can, and their names and
    /// addresses are free again. Returns how many went.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the statement fails.
    pub async fn remove_founding_signups(&self) -> Result<u64, StoreError> {
        self.with_conn(async |scope| {
            let sql = scope.sql("DELETE FROM signups WHERE organization_name IS NOT NULL");
            scope.execute(sql).await
        })
        .await
    }

    /// Removes the registrations that ended unconfirmed by `now`, and the links of
    /// the others that ended: their names and addresses are free again. Returns how
    /// many registrations went.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a statement fails; nothing is removed then.
    pub async fn remove_ended_signups(&self, now: i64) -> Result<u64, StoreError> {
        self.system()
            .transaction(async |scope| {
                let removed = remove_ended(scope, now).await?;
                let mut links =
                    scope.sql("UPDATE signups SET token_hash = NULL, token_expires_at = NULL");
                links.push_bind(" WHERE token_expires_at <= ", now);
                scope.execute(links).await?;
                Ok(removed)
            })
            .await
    }
}

/// Holds the names of accounts until the transaction ends, against every other
/// transaction that makes an account or a registration: whether a name is taken is
/// asked of two tables, and one of them is written. SQLite's writers wait for each
/// other already.
pub(crate) async fn claim_account_names(scope: &mut Scope<'_>) -> Result<(), StoreError> {
    if scope.dialect() == Dialect::Postgres {
        let mut sql = scope.sql("SELECT pg_advisory_xact_lock(");
        sql.bind(ACCOUNT_NAMES_LOCK).push(")");
        scope.fetch_optional(sql).await?;
    }
    Ok(())
}

/// Which of the name and the address a registration that has not ended by `now`
/// holds, in any letter case: for an account that is to be made.
pub(crate) async fn signup_has_names(
    scope: &mut Scope<'_>,
    username: &str,
    email: &str,
    now: i64,
) -> Result<Option<Taken>, StoreError> {
    holder_of_names(scope, Holders::Signups { live_at: now }, username, email).await
}

/// Who holds account names besides the table a new row goes into.
#[derive(Debug, Clone, Copy)]
enum Holders {
    /// The accounts, deleted or not.
    Accounts,
    /// The registrations that have not ended at `live_at`.
    Signups { live_at: i64 },
}

/// Which of the name and the address `holders` hold, in any letter case; the name
/// first where one holds each.
async fn holder_of_names(
    scope: &mut Scope<'_>,
    holders: Holders,
    username: &str,
    email: &str,
) -> Result<Option<Taken>, StoreError> {
    let table = match holders {
        Holders::Accounts => "users",
        Holders::Signups { .. } => "signups",
    };
    let mut sql = scope.sql("SELECT lower(username) = lower(");
    sql.bind(username)
        .push(&format!(") FROM {table} WHERE (lower(username) = lower("))
        .bind(username)
        .push(") OR lower(email) = lower(")
        .bind(email)
        .push("))");
    if let Holders::Signups { live_at } = holders {
        sql.push_bind(" AND expires_at > ", live_at);
    }
    sql.push(" ORDER BY 1 DESC");
    Ok(match scope.fetch_optional(sql).await? {
        Some(row) if row.bool(0)? => Some(Taken::Username),
        Some(_) => Some(Taken::Email),
        None => None,
    })
}

/// Stores `signup`, holding its name and address against every account and registration.
///
/// # Errors
///
/// Returns [`StoreError::Conflict`] if an account, deleted or not, or a registration
/// that has not ended has the name or the address.
pub(crate) async fn hold_names(scope: &mut Scope<'_>, signup: &Signup) -> Result<(), StoreError> {
    claim_account_names(scope).await?;
    if let Some(taken) =
        holder_of_names(scope, Holders::Accounts, &signup.username, &signup.email).await?
    {
        return Err(StoreError::Conflict(taken));
    }
    insert(scope, signup).await
}

async fn insert(scope: &mut Scope<'_>, signup: &Signup) -> Result<(), StoreError> {
    let (organization_name, invitation_id, invitation_org_id) = match &signup.start {
        SignupStart::Organization(name) => (name.as_str(), "", ""),
        // A name is never empty: none, for a registration by a link.
        SignupStart::Invitation { id, org_id } => ("", id.as_str(), org_id.as_str()),
    };
    let mut sql = scope.sql(&format!("INSERT INTO signups ({COLUMNS}) VALUES ("));
    sql.bind(Id(&signup.id))
        .push_bind(", ", &signup.username)
        .push_bind(", ", &signup.email)
        .push_bind(", ", &signup.display_name)
        .push_bind(", ", &signup.password_hash)
        .push_bind(", ", &signup.language)
        .push_bind(", NULLIF(", organization_name)
        .push(", '')")
        .push_bind(", ", signup.create_at)
        .push_bind(", ", signup.expires_at)
        .push_bind(", ", OptId(invitation_id))
        .push_bind(", ", OptId(invitation_org_id))
        .push_bind(", NULLIF(", &signup.browser_hash)
        .push(", ''))");
    match scope.execute(sql).await {
        Ok(_) => Ok(()),
        Err(StoreError::Database(error)) => Err(users::unique_violation(error)),
        Err(error) => Err(error),
    }
}

/// The registration `proof` names, if it and its link have not ended by `now`.
async fn find(
    scope: &mut Scope<'_>,
    proof: Proof<'_>,
    now: i64,
) -> Result<Option<Signup>, StoreError> {
    let mut sql = scope.sql(&format!("SELECT {COLUMNS} FROM signups WHERE "));
    push_proof(&mut sql, proof, now);
    sql.push_bind(" AND expires_at > ", now);
    scope
        .fetch_optional(sql)
        .await?
        .as_ref()
        .map(signup_from_row)
        .transpose()
}

/// Removes the registration `signup_id` if `proof` still names it at `now`; says
/// whether it did. Of two transactions that take one, one does.
pub(crate) async fn take(
    scope: &mut Scope<'_>,
    signup_id: &str,
    proof: Proof<'_>,
    now: i64,
) -> Result<bool, StoreError> {
    let mut sql = scope.sql("DELETE FROM signups WHERE ");
    push_proof(&mut sql, proof, now);
    sql.push_bind(" AND expires_at > ", now)
        .push_bind(" AND id = ", Id(signup_id));
    Ok(scope.execute(sql).await? == 1)
}

fn push_proof(sql: &mut Sql, proof: Proof<'_>, now: i64) {
    match proof {
        Proof::Link(digest) => {
            sql.push_bind("token_hash = ", digest)
                .push_bind(" AND token_expires_at > ", now);
        }
        Proof::Operator(username) => {
            sql.push_bind("lower(username) = lower(", username)
                .push(")");
        }
    }
}

/// Removes the registration made from the browser whose marker has the digest
/// `browser`, if there is one, ended or not: its name and address are free again, and
/// the use of a link it was made by is given back, in whatever organization the link
/// is. For a transaction that stores what replaces it. Says whether there was one.
pub(crate) async fn discard_of_browser(
    scope: &mut Scope<'_>,
    browser: &str,
) -> Result<bool, StoreError> {
    let mut sql = scope.sql(&format!(
        "SELECT {COLUMNS} FROM signups WHERE browser_hash = "
    ));
    sql.bind(browser);
    let Some(row) = scope.fetch_optional(sql).await? else {
        return Ok(false);
    };
    discard(scope, &signup_from_row(&row)?).await
}

/// Removes `signup`, giving back the use of the link it was made by; says whether it
/// was there still. The link is held first, as a confirmation holds it: the use goes
/// back only with the registration that took it, one confirmed meanwhile keeping it.
async fn discard(scope: &mut Scope<'_>, signup: &Signup) -> Result<bool, StoreError> {
    hold_link_of(scope, signup).await?;
    let mut sql = scope.sql("DELETE FROM signups WHERE id = ");
    sql.bind(Id(&signup.id));
    if scope.execute(sql).await? == 0 {
        return Ok(false);
    }
    give_back_use(scope, signup).await?;
    Ok(true)
}

/// What a transaction's wall was before it was opened for one link.
struct Wall(Option<String>);

/// Opens the transaction's wall for a link of the organization `org_id`, to be closed
/// again with [`close_wall`]: a registration is no organization's, and the link one
/// was made by may be of another organization than the one a transaction that removes
/// it works in. Nothing but the statement on that one link, which names it and its
/// organization, runs while it is open. Only inside a transaction, whose setting it
/// is; nothing to do where the transaction is the link's organization's, or on SQLite,
/// which has no wall.
async fn open_wall(scope: &mut Scope<'_>, org_id: &str) -> Result<Wall, StoreError> {
    if scope.dialect() != Dialect::Postgres || scope.org_id().as_deref() == Some(org_id) {
        return Ok(Wall(None));
    }
    let sql = scope.sql("SELECT COALESCE(current_setting('fvoci.system', true), 'off')");
    let before = match scope.fetch_optional(sql).await? {
        Some(row) => row.text(0)?,
        None => "off".to_owned(),
    };
    set_wall(scope, "on").await?;
    Ok(Wall(Some(before)))
}

/// Puts the transaction's wall back as it was before [`open_wall`].
async fn close_wall(scope: &mut Scope<'_>, wall: Wall) -> Result<(), StoreError> {
    match wall.0 {
        Some(before) => set_wall(scope, &before).await,
        None => Ok(()),
    }
}

async fn set_wall(scope: &mut Scope<'_>, reach: &str) -> Result<(), StoreError> {
    let mut sql = scope.sql("SELECT set_config('fvoci.system', ");
    sql.bind(reach).push(", true)");
    scope.fetch_optional(sql).await?;
    Ok(())
}

/// Removes the registrations that ended by `now`, giving back the uses of the links
/// they were made by first; returns how many. Needs the system's reach: the links are
/// of every organization, and a statement the wall hides them from changes nothing.
async fn remove_ended(scope: &mut Scope<'_>, now: i64) -> Result<u64, StoreError> {
    // Those made by a link one by one, each link held before its registration goes, as
    // a registration by it is made and confirmed, and in the order of the links, so that
    // two clean-ups at once hold them alike: a use goes back only with the registration
    // that took it, one confirmed meanwhile keeping it.
    let mut sql = scope.sql(&format!(
        "SELECT {COLUMNS} FROM signups WHERE expires_at <= "
    ));
    sql.bind(now)
        .push(" AND invitation_id IS NOT NULL ORDER BY invitation_id, id");
    let held = scope
        .fetch_all(sql)
        .await?
        .iter()
        .map(signup_from_row)
        .collect::<Result<Vec<_>, _>>()?;
    let mut removed = 0;
    for signup in &held {
        hold_link_of(scope, signup).await?;
        let mut sql = scope.sql("DELETE FROM signups WHERE id = ");
        sql.bind(Id(&signup.id))
            .push_bind(" AND expires_at <= ", now);
        if scope.execute(sql).await? == 1 {
            give_back_use(scope, signup).await?;
            removed += 1;
        }
    }
    let mut sql = scope.sql("DELETE FROM signups WHERE invitation_id IS NULL AND expires_at <= ");
    sql.bind(now);
    Ok(removed + scope.execute(sql).await?)
}

/// Holds the link a registration was made by, if it is there, against its acceptances
/// and confirmations, in whatever organization it is.
async fn hold_link_of(scope: &mut Scope<'_>, signup: &Signup) -> Result<(), StoreError> {
    if let (SignupStart::Invitation { id, org_id }, Dialect::Postgres) =
        (&signup.start, scope.dialect())
    {
        let wall = open_wall(scope, org_id).await?;
        let mut sql = scope.sql("SELECT id FROM invitations WHERE id = ");
        sql.bind(Id(id))
            .push_bind(" AND org_id = ", Id(org_id))
            .push(" FOR UPDATE");
        scope.fetch_optional(sql).await?;
        close_wall(scope, wall).await?;
    }
    Ok(())
}

/// Gives back the use a registration took of the link it was made by, if it was, in
/// whatever organization the link is.
async fn give_back_use(scope: &mut Scope<'_>, signup: &Signup) -> Result<(), StoreError> {
    if let SignupStart::Invitation { id, org_id } = &signup.start {
        let wall = open_wall(scope, org_id).await?;
        let mut sql = scope.sql("UPDATE invitations SET uses = uses - 1 WHERE id = ");
        sql.bind(Id(id))
            .push_bind(" AND org_id = ", Id(org_id))
            .push(" AND uses > 0");
        scope.execute(sql).await?;
        close_wall(scope, wall).await?;
    }
    Ok(())
}

fn signup_from_row(row: &Row) -> Result<Signup, StoreError> {
    Ok(Signup {
        id: row.id(0)?,
        username: row.text(1)?,
        email: row.text(2)?,
        display_name: row.text(3)?,
        password_hash: row.text(4)?,
        language: row.text(5)?,
        start: match row.id(9)? {
            invitation_id if invitation_id.is_empty() => SignupStart::Organization(row.text(6)?),
            invitation_id => SignupStart::Invitation {
                id: invitation_id,
                org_id: row.id(10)?,
            },
        },
        browser_hash: row.opt_text(11)?.unwrap_or_default(),
        create_at: row.int(7)?,
        expires_at: row.int(8)?,
    })
}
