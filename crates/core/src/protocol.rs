//! Wire protocol between server and phone. Both REST (for `Command`-shaped
//! POST bodies) and the `/ws` WebSocket channel (for `Event`s).

use serde::{Deserialize, Serialize};

use crate::figure::FigureId;
use crate::portal::{SLOT_COUNT, SlotIndex, SlotState};

/// Client → server. Delivered as REST bodies in MVP; a WebSocket command
/// channel may land later.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Command {
    LoadFigure {
        slot: SlotIndex,
        figure_id: FigureId,
    },
    ClearSlot {
        slot: SlotIndex,
    },
    /// Ask the server to re-read the portal from RPCS3 and broadcast a fresh
    /// `PortalSnapshot`.
    RefreshPortal,
}

/// Server → client. Delivered on `/ws`.
///
/// Several variants carry a `session_id` so a shared broadcast channel can
/// fan messages out to all two connected clients, and each client filters
/// for events addressed to it. The id is the opaque u64 minted by the
/// server's [`SessionRegistry`] — the phone receives its own id in the
/// initial [`Event::Welcome`] and stores it for filtering + REST routing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    /// First event on every WS connection. Tells the phone the session id
    /// it should attach as `X-Session-Id` on subsequent REST calls, and
    /// which id to watch for in targeted events like `ProfileChanged` and
    /// `TakenOver`.
    ///
    /// `boot_id` is a u64 generated once at server startup. The phone
    /// remembers it across WS reconnects: a Welcome whose `boot_id`
    /// differs from the last-seen one means the server restarted, so
    /// the phone should reset its in-memory UI state (which profile
    /// is unlocked, current screen, etc.) since the server has no
    /// record of any of it.
    Welcome { session_id: u64, boot_id: u64 },
    /// Full snapshot of all 8 slots. Sent on connect and after `RefreshPortal`.
    PortalSnapshot { slots: [SlotState; SLOT_COUNT] },
    /// One slot changed state.
    SlotChanged { slot: SlotIndex, state: SlotState },
    /// Non-fatal error surfaced as a toast on the phone.
    Error { message: String },
    /// Game state changed. `None` means "no game running".
    GameChanged { current: Option<GameLaunched> },
    /// Profile unlock/lock transition, scoped to one session. `None` means
    /// the session is locked. Broadcast; clients ignore events not for
    /// their `session_id`.
    ProfileChanged {
        session_id: u64,
        profile: Option<UnlockedProfile>,
    },
    /// The target session has been forcibly evicted by a 3rd connection
    /// (FIFO — oldest out). The evicted phone shows the "Kaos took over"
    /// screen. Broadcast; clients ignore events not for their `session_id`.
    ///
    /// `cooldown_remaining_secs` is the wall-clock seconds the evicted
    /// phone must wait before the server will accept its KICK BACK IN
    /// reload — the FIFO forced-evict cooldown (PLAN 3.10/8.2a). Phones
    /// drive a local 1Hz countdown from this value and grey-out the
    /// kickback button until it reaches zero.
    TakenOver {
        session_id: u64,
        by_kaos: String,
        cooldown_remaining_secs: u32,
    },
    /// Offered to a session right after its profile unlocks, when that
    /// profile has a prior portal layout the user can resume. Phone shows
    /// a "Resume last setup?" modal; on confirm it issues per-slot
    /// `/load` calls against `slots`. Broadcast; clients filter by own id.
    /// PLAN 3.12.
    ResumePrompt {
        session_id: u64,
        slots: [SlotState; SLOT_COUNT],
    },
    /// RPCS3 exited unexpectedly while a game was running. Phones render a
    /// full-screen "GAME CRASHED" overlay (not a toast — session-breaking
    /// event). Auto-dismisses on the next `GameChanged { current: Some(_) }`.
    /// Broadcast to all sessions — the portal is dead for everyone. PLAN
    /// 4.15.14; see `docs/aesthetic/navigation.md` §3.8.
    GameCrashed { message: String },
    /// Kaos mid-game swap (PLAN 8.2b.4). The server timer picked a
    /// compatible replacement for one of `profile_id`'s currently-placed
    /// figures and just executed a clear+load pair on `slot`. Phones
    /// render the `kaos_swap` variant of `KaosOverlay` for ~5s showing
    /// the taunt, then dismiss. Broadcast to every session so all
    /// connected phones see the notification; the body of `profile_id`
    /// lets them tint the overlay with the target player's colour if
    /// they want to emphasise it's Alice's figure that changed.
    ///
    /// Replay-buffered for ghosted sessions: if the target phone was
    /// backgrounded when the timer fired, the taunt lands when they
    /// reconnect (PLAN 8.1.2). The `slot` + `old_figure_id` +
    /// `new_figure_id` fields let the phone reconcile against the
    /// `PortalSnapshot` it receives on reconnect — if the figure is
    /// still there, the swap hasn't actually happened yet (rare: driver
    /// job queued but not executed); normally the snapshot will already
    /// reflect the new state.
    KaosTaunt {
        profile_id: String,
        slot: SlotIndex,
        old_figure_id: crate::figure::FigureId,
        new_figure_id: crate::figure::FigureId,
        taunt: String,
    },
    /// A Skylanders figure was scanned on the attached NFC reader. Broadcast
    /// to all sessions so the phone can refresh its toy-box library view and
    /// surface a "new figure imported" toast. The raw 1024-byte tag dump
    /// stays on the server (written to the scanned-figures dir) — phones
    /// never see it. PLAN 6.5.1.
    ///
    /// - `uid`: 8-char uppercase hex of the Mifare NUID (doubles as filename stem).
    /// - `figure_id`: tag's 24-bit toy type; 0 if parse failed.
    /// - `variant`: tag's 16-bit variant word; 0 if parse failed.
    /// - `display_name`: parser-derived nickname for the UI ("Snap Shot",
    ///   "Eruptor", etc.). Best-effort — empty for parse failures, and for
    ///   Creation Crystals / CYOS tags the default-nickname offset decodes
    ///   as mojibake until 6.2.9 pins the CYOS layout (PLAN 6.5.0 notes).
    /// - `is_duplicate`: true if we'd already seen this uid (the scanned
    ///   `<uid>.sky` file existed before we re-wrote it). Phone branches
    ///   the confirmation copy on this — "Added to your collection." vs
    ///   "Already in your collection."
    FigureScanned {
        uid: String,
        figure_id: u32,
        variant: u16,
        display_name: String,
        is_duplicate: bool,
    },
}

/// Public profile description included in [`Event::ProfileChanged`] and the
/// initial WS snapshot. Never carries the PIN hash.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnlockedProfile {
    pub id: String,
    pub display_name: String,
    pub color: String,
    /// PLAN 8.2b.1 — whether the user has opted the profile into
    /// Kaos mid-game swaps. Phone's settings screen reads this to
    /// show the correct toggle state. `#[serde(default)]` so older
    /// servers without the column still parse into UnlockedProfile
    /// on the phone (default = off, matches the DB default).
    #[serde(default)]
    pub kaos_enabled: bool,
}

/// Announcement payload included in `Event::GameChanged`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameLaunched {
    pub serial: crate::figure::GameSerial,
    pub display_name: String,
}
