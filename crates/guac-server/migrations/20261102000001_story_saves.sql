-- A profile's story save per game (openspec AU.3, design decision 4): what
-- the original's save scripts keep, by the names they give it, one row per
-- profile per game per save layout. A build reads and writes only the row of
-- its own layout; a row of another is kept as it was, never written over or
-- deleted, so a build that reads it can take it back. `layout` and `data`
-- are guacamole's (`saves::SaveValues` as JSON); the store reads neither. The
-- product's migration set has no such table, so only guacamole's arm
-- queries it.

CREATE TABLE IF NOT EXISTS story_saves (
    profile_id TEXT NOT NULL
        REFERENCES profiles(id) ON DELETE CASCADE,
    game       TEXT NOT NULL,
    layout     TEXT NOT NULL,
    data       TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (profile_id, game, layout)
);
