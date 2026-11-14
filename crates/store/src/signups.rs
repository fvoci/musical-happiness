//! Registrations whose email address is not confirmed yet: no account, nobody's, in no
//! organization, until the link of their mail confirms them; removed seven days after
//! they were made if it never does.

use fvoci_model::User;

use crate::accounts::{self, Founding};
use crate::db::{Dialect, Id, Row, Scope};
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
    pub fn account(&self) -> User {
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
}

/// The columns of a registration, in the order [`signup_from_row`] reads them.
const COLUMNS: &str = "id, username, email, display_name, password_hash, language, \
    organization_name, create_at, expires_at";

impl Store {
    /// Stores a registration whose address is to be confirmed, holding its name and
    /// address. The registrations that ended by `now` go first.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Conflict`] if an account, deleted or not, or a registration
    /// that has not ended has the name or the address, in any letter case; and
    /// [`StoreError::Database`] if a statement fails.
    pub async fn create_signup(&self, signup: &Signup, now: i64) -> Result<(), StoreError> {
        self.transaction(async |scope| {
            remove_ended(scope, now).await?;
            claim_account_names(scope).await?;
            if let Some(taken) = account_has_names(scope, &signup.username, &signup.email).await? {
                return Err(StoreError::Conflict(taken));
            }
            insert(scope, signup).await
        })
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
        proof: Proof<'_>,
        founding: Founding,
        now: i64,
    ) -> Result<Option<Confirmed>, StoreError> {
        let Some(signup) = self
            .with_conn(async |scope| find(scope, proof, now).await)
            .await?
        else {
            return Ok(None);
        };
        let SignupStart::Organization(name) = &signup.start;
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
                let mut user = signup.account();
                let workspace_id = accounts::found(scope, &mut user, name, founding, place).await?;
                Ok(Some(Confirmed {
                    user,
                    org_id: org_id.clone(),
                    workspace_id,
                }))
            })
            .await
    }

    /// Removes the registration `proof` names at `now`, unconfirmed: its name and
    /// address are free again. Says whether there was one.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a statement fails.
    pub async fn discard_signup(&self, proof: Proof<'_>, now: i64) -> Result<bool, StoreError> {
        self.transaction(async |scope| {
            let Some(signup) = find(scope, proof, now).await? else {
                return Ok(false);
            };
            take(scope, &signup.id, proof, now).await
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
        self.transaction(async |scope| {
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

/// Which of the name and the address a registration that has not ended holds, in any
/// letter case: for an account that is to be made.
pub(crate) async fn signup_has_names(
    scope: &mut Scope<'_>,
    username: &str,
    email: &str,
) -> Result<Option<Taken>, StoreError> {
    let mut sql = scope.sql("SELECT lower(username) = lower(");
    sql.bind(username)
        .push(") FROM signups WHERE (lower(username) = lower(")
        .bind(username)
        .push(") OR lower(email) = lower(")
        .bind(email)
        .push_bind(")) AND expires_at > ", crate::db::now_millis())
        .push(" ORDER BY 1 DESC");
    Ok(match scope.fetch_optional(sql).await? {
        Some(row) if row.bool(0)? => Some(Taken::Username),
        Some(_) => Some(Taken::Email),
        None => None,
    })
}

/// Which of the name and the address an account, deleted or not, holds, in any
/// letter case: for a registration that is to be made.
async fn account_has_names(
    scope: &mut Scope<'_>,
    username: &str,
    email: &str,
) -> Result<Option<Taken>, StoreError> {
    let mut sql = scope.sql("SELECT lower(username) = lower(");
    sql.bind(username)
        .push(") FROM users WHERE lower(username) = lower(")
        .bind(username)
        .push(") OR lower(email) = lower(")
        .bind(email)
        .push(") ORDER BY 1 DESC");
    Ok(match scope.fetch_optional(sql).await? {
        Some(row) if row.bool(0)? => Some(Taken::Username),
        Some(_) => Some(Taken::Email),
        None => None,
    })
}

async fn insert(scope: &mut Scope<'_>, signup: &Signup) -> Result<(), StoreError> {
    let SignupStart::Organization(organization_name) = &signup.start;
    let mut sql = scope.sql(&format!("INSERT INTO signups ({COLUMNS}) VALUES ("));
    sql.bind(Id(&signup.id))
        .push_bind(", ", &signup.username)
        .push_bind(", ", &signup.email)
        .push_bind(", ", &signup.display_name)
        .push_bind(", ", &signup.password_hash)
        .push_bind(", ", &signup.language)
        .push_bind(", ", organization_name)
        .push_bind(", ", signup.create_at)
        .push_bind(", ", signup.expires_at)
        .push(")");
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
async fn take(
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

fn push_proof(sql: &mut crate::db::Sql, proof: Proof<'_>, now: i64) {
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

/// Removes the registrations that ended by `now`; returns how many.
async fn remove_ended(scope: &mut Scope<'_>, now: i64) -> Result<u64, StoreError> {
    let mut sql = scope.sql("DELETE FROM signups WHERE expires_at <= ");
    sql.bind(now);
    scope.execute(sql).await
}

fn signup_from_row(row: &Row) -> Result<Signup, StoreError> {
    Ok(Signup {
        id: row.id(0)?,
        username: row.text(1)?,
        email: row.text(2)?,
        display_name: row.text(3)?,
        password_hash: row.text(4)?,
        language: row.text(5)?,
        start: SignupStart::Organization(row.text(6)?),
        create_at: row.int(7)?,
        expires_at: row.int(8)?,
    })
}
