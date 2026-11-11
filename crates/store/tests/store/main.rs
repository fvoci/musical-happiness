//! Integration tests of the store. Many began as ports of the original's shared store
//! test suite (`server/services/store/storetests`) and keep its test names; they test
//! this server's schema, not the original's.
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
mod order;
mod sessions;
mod setup;
mod sharing;
mod subscriptions;
mod teams;
mod tenancy;
mod users;
