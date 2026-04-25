-- PLAN 8.2b.1: per-profile Kaos feature toggle.
--
-- Default 0 (disabled) so existing profiles don't start getting
-- mid-game swaps without explicit opt-in while we tune cadence
-- + compatibility in 8.2b. Stored as INTEGER 0/1 since SQLite
-- lacks a bool primitive and `sqlx` maps it cleanly.

ALTER TABLE profiles
    ADD COLUMN kaos_enabled INTEGER NOT NULL DEFAULT 0;
