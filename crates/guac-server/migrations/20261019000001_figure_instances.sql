-- A figure's progress per profile (openspec AH.1, design decision 8): one row
-- per profile per figure, made fresh at the profile's first placement of it.
-- `figure_id` is the id `figure_usage` keys by; `xp` is the total, never the
-- figure's own 33,000/63,500 split. No CHECK on the caps: they're the game's,
-- read off the disc, and the store clamps at the caps its caller passes. The
-- product's migration set has no such table, so only guacamole's arm queries it.

CREATE TABLE IF NOT EXISTS figure_instances (
    profile_id    TEXT NOT NULL
        REFERENCES profiles(id) ON DELETE CASCADE,
    figure_id     TEXT NOT NULL,
    xp            INTEGER NOT NULL DEFAULT 0,
    gold          INTEGER NOT NULL DEFAULT 0,
    playtime_secs INTEGER NOT NULL DEFAULT 0,
    generation    INTEGER NOT NULL,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL,
    PRIMARY KEY (profile_id, figure_id)
);

-- Every generation comes from this one counter, so a row made fresh after a
-- reset deleted the last one never reuses a generation the TV may still be
-- sending values under.
CREATE TABLE IF NOT EXISTS figure_generation (
    id   INTEGER PRIMARY KEY CHECK (id = 1),
    last INTEGER NOT NULL
);
INSERT OR IGNORE INTO figure_generation (id, last) VALUES (1, 0);
