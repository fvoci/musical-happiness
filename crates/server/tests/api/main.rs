//! API tests: a server in this process on a port of its own, driven over HTTP.
//!
//! Ports of the original's `server/integrationtests` (the cases for native
//! authentication and local permissions) and of the `server/app` tests
//! that need a store. One module per original test file.

mod common;

mod admin;
mod admins;
mod blocks;
mod boards;
mod boards_and_blocks;
mod cards;
mod content_blocks;
mod export;
mod files;
mod limits;
mod onboarding;
mod paths;
mod properties;
mod search;
mod sharing;
mod sidebar;
mod subscriptions;
mod system;
mod templates;
mod users;
mod visibility;
mod websocket;
