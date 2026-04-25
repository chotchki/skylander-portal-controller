-- Per-game preferred display mode (PLAN 4.20.x).
--
-- Captured after each successful game boot once the mode stabilises;
-- read on the next launch of the same serial so we can pre-set
-- Windows' primary display BEFORE spawning RPCS3. With the mode
-- already correct, RPCS3's startup doesn't trigger a display-mode
-- flicker — which has been masking egui-side animations on Chris's
-- HTPC.
--
-- Refresh rate is stored as an integer (Hz) — `EnumDisplaySettings`
-- returns the dmDisplayFrequency field which is already integer Hz.

CREATE TABLE IF NOT EXISTS game_display_modes (
    serial      TEXT PRIMARY KEY,
    width       INTEGER NOT NULL,
    height      INTEGER NOT NULL,
    refresh_hz  INTEGER NOT NULL,
    captured_at TEXT NOT NULL
);
