//! API tests: a server in this process on a port of its own, driven over HTTP.
//!
//! Ports of the original's `server/integrationtests` (the cases for native
//! authentication and local permissions) and of the `server/app` tests
//! that need a store. One module per original test file.

mod common;

mod admin;
mod admins;
mod blocks;
mod board_broadcasts;
mod boards;
mod boards_and_blocks;
mod bodies;
mod card_previews;
mod cards;
mod collab;
mod comments;
mod compression;
mod content_blocks;
mod cookies;
mod deleted_cards;
mod export;
mod files;
mod imports;
mod limits;
mod login_limits;
mod loose_objects;
mod metrics;
mod metrics_accounts;
mod metrics_outcomes;
mod onboarding;
mod organizations;
mod paths;
mod properties;
mod readiness;
mod reconcile;
mod refusals;
mod search;
mod sharing;
mod sidebar;
mod sidebar_messages;
mod subscriptions;
mod system;
mod templates;
mod uploads;
mod users;
mod visibility;
mod websocket;
mod workspaces;
