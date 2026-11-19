//! Integration tests of the store. Many began as ports of the original's shared store
//! test suite (`server/services/store/storetests`) and keep its test names; they test
//! this server's schema, not the original's.
//!
//! They run against SQLite by default. Set `FVOCI_STORE_TEST_DB_TYPE=postgres` and
//! `FVOCI_STORE_TEST_CONN_STRING` to a PostgreSQL URL to run them against PostgreSQL; each
//! test then works in a schema of its own. The role of the URL and the third variable
//! one test needs are described in `fvoci_store::testing`.

mod account_deletion;
mod account_erasure;
mod block_reads_and_patches;
mod blocks;
mod blocks_and_deletion;
mod board_blocks;
mod board_kinds;
mod board_search;
mod boards;
mod boards_and_blocks;
mod body_docs;
mod body_links;
mod body_versions;
mod builtin_templates;
mod categories;
mod category_boards;
mod common;
mod container_rename;
mod file_info;
mod image_files;
mod instance_role_scope;
mod invitations;
mod link_lock_order;
mod lock_gate;
mod loose_objects;
mod observe;
mod oidc;
mod oidc_audit;
mod oidc_former;
mod oidc_held;
mod oidc_pending;
mod oidc_trial_binding;
mod operator_password;
mod order;
mod org_admin;
mod page_copies;
mod page_moves;
mod page_trash;
mod pages;
mod password_reset;
mod person_names;
mod registration;
mod restored_boards;
mod search_outbox;
mod session_origin;
mod sessions;
mod setup;
mod sharing;
mod signup_lock_order;
mod signups;
mod sqlite_wal;
mod subscriptions;
mod table_owner;
mod tenancy;
mod trash;
mod trash_file_race;
mod users;
mod users_at_once;
mod workspace_membership;
mod workspace_names;
mod workspace_rename;
mod workspaces;
