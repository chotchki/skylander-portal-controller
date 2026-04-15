-- Profile system (PLAN 3.9).
--
-- profiles:     up-to-4 named profiles with PIN hashes (argon2) and a hex colour.
-- sessions:     per-profile last-known portal layout (JSON blob) for resume prompt.
-- figure_usage: per-profile last-used timestamp for a figure (seeds future
--               sort-by-recency; also the hook working copies will grow off of).

CREATE TABLE IF NOT EXISTS profiles (
    id           TEXT PRIMARY KEY,
    display_name TEXT NOT NULL,
    pin_hash     TEXT NOT NULL,
    color        TEXT NOT NULL,
    created_at   TEXT NOT NULL
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
