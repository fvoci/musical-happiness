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
mod comments;
mod compression;
mod content_blocks;
mod cookies;
mod deleted_cards;
mod export;
mod fake_oidc;
mod files;
mod guests;
mod imports;
mod invitations;
mod limits;
mod login_limits;
mod loose_objects;
mod metrics;
mod metrics_accounts;
mod metrics_listener;
mod metrics_outcomes;
mod oidc;
mod oidc_races;
mod onboarding;
mod organizations;
mod paths;
mod people;
mod properties;
mod readiness;
mod reconcile;
mod refusals;
mod relation_copies;
mod relations;
mod search;
mod server_lock;
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
mod workspace_admin;
mod workspace_members;
mod workspace_removal;
mod workspaces;
