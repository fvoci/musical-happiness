//! Ordered SQLite and PostgreSQL schema migration steps.

/// One migration: its version, its name, and its SQL for each database kind.
pub(crate) struct Step {
    pub(super) version: i64,
    pub(super) description: &'static str,
    pub(super) sqlite: &'static str,
    pub(super) postgres: &'static str,
}

/// Every migration, oldest first. Applied ones are never edited: add a new one.
pub(crate) const STEPS: [Step; 3] = [
    Step {
        version: 1,
        description: "baseline",
        sqlite: include_str!("../../schema/sqlite/0001_baseline.sql"),
        postgres: include_str!("../../schema/postgres/0001_baseline.sql"),
    },
    Step {
        version: 4,
        description: "favorites",
        sqlite: include_str!("../../schema/sqlite/0004_favorites.sql"),
        postgres: include_str!("../../schema/postgres/0004_favorites.sql"),
    },
    Step {
        version: 5,
        description: "notification archive",
        sqlite: include_str!("../../schema/sqlite/0005_notification_archive.sql"),
        postgres: include_str!("../../schema/postgres/0005_notification_archive.sql"),
    },
];
