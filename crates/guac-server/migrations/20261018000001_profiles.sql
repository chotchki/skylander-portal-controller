-- guacamole's profile tables (AA design decision 6): the product's three
-- migrations folded into one, same shape, so one profile store reads both
-- (`profiles::tests` compares them).

CREATE TABLE IF NOT EXISTS profiles (
    id           TEXT PRIMARY KEY,
    display_name TEXT NOT NULL,
    pin_hash     TEXT NOT NULL,
    color        TEXT NOT NULL,
    created_at   TEXT NOT NULL,
    kaos_enabled INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS sessions (
    profile_id               TEXT PRIMARY KEY
        REFERENCES profiles(id) ON DELETE CASCADE,
    last_portal_layout_json  TEXT,
    updated_at               TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS figure_usage (
    profile_id   TEXT NOT NULL
        REFERENCES profiles(id) ON DELETE CASCADE,
    figure_id    TEXT NOT NULL,
    last_used_at TEXT NOT NULL,
    PRIMARY KEY (profile_id, figure_id)
);

-- Unused by guacamole (no display-mode hook), kept so every store query has
-- its table.
CREATE TABLE IF NOT EXISTS game_display_modes (
    serial      TEXT PRIMARY KEY,
    width       INTEGER NOT NULL,
    height      INTEGER NOT NULL,
    refresh_hz  INTEGER NOT NULL,
    captured_at TEXT NOT NULL
);
