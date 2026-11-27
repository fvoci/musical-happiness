//! Ordered SQLite and PostgreSQL schema migration steps.

/// One migration: its version, its name, and its SQL for each database kind.
pub(crate) struct Step {
    pub(super) version: i64,
    pub(super) description: &'static str,
    pub(super) sqlite: &'static str,
    pub(super) postgres: &'static str,
}

/// Every migration, oldest first. Applied ones are never edited: add a new one.
pub(crate) const STEPS: [Step; 2] = [
    Step {
        version: 1,
        description: "baseline",
        sqlite: include_str!("../../schema/sqlite/0001_baseline.sql"),
        postgres: include_str!("../../schema/postgres/0001_baseline.sql"),
    },
    Step {
        version: 2,
        description: "card events",
        sqlite: include_str!("../../schema/sqlite/0002_card_events.sql"),
        postgres: include_str!("../../schema/postgres/0002_card_events.sql"),
    },
];

/// What takes away, on either kind of database, what each migration after the baseline
/// made, by its version, the newest first: for the tests of a database that the last
/// server before the baseline left, whose schema is the baseline's. A test of this
/// module holds that every migration after the baseline is here.
#[cfg(any(test, feature = "test-util"))]
pub(crate) const UNDONE_AFTER_THE_BASELINE: [(i64, &str); 1] = [(2, "DROP TABLE card_events")];
