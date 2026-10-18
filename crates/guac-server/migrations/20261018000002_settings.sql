-- guacamole's settings (openspec AA, design decision 6): the dumps folders, the
-- port and the pairing key, one row each, read by `setup.rs`.

CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
