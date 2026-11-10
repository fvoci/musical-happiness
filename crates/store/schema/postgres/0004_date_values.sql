-- Changes of the data that SQL alone does not make well, made by the server once after
-- the migrations (crates/store/src/data_steps.rs). A step's row goes when it is done,
-- in the transaction that does it.
CREATE TABLE data_steps (
    name TEXT NOT NULL PRIMARY KEY
);
-- The values of date properties and the values of date filters, to the shape of
-- docs/s6-design.md 5.7 (B): live and in the trash.
INSERT INTO data_steps (name) VALUES ('date_values');
