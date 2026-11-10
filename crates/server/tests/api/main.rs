//! API tests: a server in this process on a port of its own, driven over HTTP.
//!
//! Ports of the original's `server/integrationtests` (the cases for native
//! authentication, local permissions and single-user mode) and of the `server/app` tests
//! that need a store. One module per original test file.

mod common;

mod admin;
mod blocks;
mod boards;
mod boards_and_blocks;
mod cards;
mod content_blocks;
mod export;
mod files;
mod onboarding;
mod search;
mod sharing;
mod sidebar;
mod subscriptions;
mod system;
mod templates;
mod users;
mod websocket;
