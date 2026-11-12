//! Integration tests of the store. Many began as ports of the original's shared store
//! test suite (`server/services/store/storetests`) and keep its test names; they test
//! this server's schema, not the original's.
//!
//! They run against SQLite by default. Set `FVOCI_STORE_TEST_DB_TYPE=postgres` and
//! `FVOCI_STORE_TEST_CONN_STRING` to a PostgreSQL URL to run them against PostgreSQL; each
//! test then works in a schema of its own. The role of the URL and the third variable
//! one test needs are described in `fvoci_store::testing`.

mod block_reads_and_patches;
mod blocks;
mod boards;
mod boards_and_blocks;
mod categories;
mod category_boards;
mod common;
mod file_info;
mod image_files;
mod invitations;
mod loose_objects;
mod observe;
mod oidc;
mod order;
mod restored_boards;
mod sessions;
mod setup;
mod sharing;
mod subscriptions;
mod tenancy;
mod trash;
mod users;
mod users_at_once;
mod workspace_membership;
mod workspace_rename;
mod workspaces;
