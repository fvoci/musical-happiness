//! Integration tests of the store, ported from the original's shared store test suite
//! (`server/services/store/storetests`).
//!
//! They run against SQLite by default. Set `FVOCI_STORE_TEST_DB_TYPE=postgres` and
//! `FVOCI_STORE_TEST_CONN_STRING` to a PostgreSQL URL to run them against PostgreSQL; each
//! test then works in a schema of its own.

mod blocks;
mod boards;
mod boards_and_blocks;
mod categories;
mod category_boards;
mod common;
mod file_info;
mod sessions;
mod setup;
mod sharing;
mod subscriptions;
mod system_settings;
mod teams;
mod users;
