# RPCS3 Portal Control (Phase 1 spike 1a)

## Decision

**Windows UI Automation (UIA) is fully viable.** Every widget we need to drive the emulated Skylanders portal is addressable via UIA and supports `InvokePattern` / `ValuePattern`. We proceed with UIA as the primary mechanism; OCR / coordinate-based fallbacks are off the table unless a future RPCS3 release breaks accessibility.

Tested against RPCS3 `0.0.40-19203-0b9c53e2 Alpha | master` on Windows 11.

## Source code confirmed

RPCS3's Skylanders emulation lives in:
- `rpcs3/Emu/Io/Skylander.{h,cpp}` — the USB device emulation and `sky_portal` state (`g_skyportal.load_skylander`, `remove_skylander`).
- `rpcs3/rpcs3qt/skylander_dialog.{h,cpp}` — the "Skylanders Manager" dialog, `UI_SKY_NUM = 8` slots.

Key facts from the source:
- 8 fixed slots regardless of game (matches user expectation).
- Each slot has `Clear`, `Create`, `Load` buttons. `Load` opens a `QFileDialog` with filter `"Skylander (*.sky *.bin *.dmp *.dump);;All Files (*)"`.
- Dialog is a **singleton** (`skylander_dialog::inst`) with `WA_DeleteOnClose` — closing resets the instance. Loading a figure that's "already in use on the portal" fails with a warning dialog, so we must clear a slot before reloading into it.
- The dialog is menu-triggered (`actionManage_Skylanders_Portal` → `skylander_dialog::get_dlg(this)->show()`). **No active emulation required** to open it — great for test setup.
- File-load code path: `load_skylander` opens a `QFileDialog::getOpenFileName`, then `load_skylander_path` locks the file (`fs::lock`) and passes it to `g_skyportal.load_skylander`.

## UIA tree snapshot

Rooted at the RPCS3 main window (the dialog is a **nested window**, not a desktop-level top-level):

```
Window "Skylanders Manager" <skylander_dialog> #gui_application.skylanders_manager
  TitleBar ""
    Button "Close"
  Group "Active Portal Skylanders:" <QGroupBox>
    Text   "Skylander 1"      <QLabel>
    Edit   ""                 <QLineEdit>   [Value]          # shows "None" or figure name
    Button "Clear"            <QPushButton> [Invoke,Value]
    Button "Create"           <QPushButton> [Invoke,Value]
    Button "Load"             <QPushButton> [Invoke,Value]
    Custom "..."              <QFrame>                        # separator after row 1
    Text   "Skylander 2"      <QLabel>
    Edit   ""                 <QLineEdit>   [Value]
    Button "Clear"            <QPushButton> [Invoke,Value]
    Button "Create"           <QPushButton> [Invoke,Value]
    Button "Load"             <QPushButton> [Invoke,Value]
    Custom "..."              <QFrame>
    ... (× 8 rows total)
```

## Addressing strategy

- **Find the dialog**: walk children of the RPCS3 `main_window`, match either class `skylander_dialog` or AutomationId ending in `skylanders_manager`. Fallback to name match on the window's localized title `"Skylanders Manager"`.
- **Find the group box**: first (and only) `QGroupBox` child, or match by name `"Active Portal Skylanders:"`.
- **Resolve a slot row**: find the `QLabel` whose text is exactly `"Skylander N"`; the next four siblings are the `QLineEdit`, `Clear`, `Create`, `Load` buttons in layout order. A `QFrame` separator follows rows 2–8.
- **Read current slot state**: `ValuePattern::get_value()` on the `QLineEdit`. Source code shows the edit holds either `"None"` or the figure's canonical name (from `list_skylanders[(id,var)]`) or `"Unknown (Id:… Var:…)"` for unmapped IDs.
- **Load a figure**: `InvokePattern::invoke()` on the row's `Load` button → file dialog appears (see below).
- **Clear a slot**: `InvokePattern::invoke()` on the row's `Clear` button. No further interaction needed.
- **AutomationIds are NOT unique per row**: every button shares `gui_application.skylanders_manager.QGroupBox.QPushButton`. Disambiguation must be by tree position (use the row's label as the anchor) or by bounding-rect `top` coordinate. Tree position is more robust to window resizes.

## Driving the file dialog (to be confirmed)

Not yet probed in this spike. Plan:
1. Invoke the row's `Load` button.
2. Wait (with a timeout) for a new top-level window whose name matches `"Select Skylander File"` or is a standard Windows common file dialog.
3. The Windows common dialog exposes UIA `Edit` (file-name box) with a well-known AutomationId. Write the absolute `.sky` path via `ValuePattern::set_value()`.
4. Invoke the `Open` button.
5. Wait for the parent dialog's row `QLineEdit` to change away from `"None"` — this is our confirmation signal for the spinner.

Risks captured for the follow-up probe:
- Windows 11 may intermittently use the new-style (XAML) file picker; different UIA tree. Mitigation: support both layouts.
- RPCS3 rejects a file already in use (see source). A retry that first invokes `Clear` avoids this.

## Confirmation / spinner signal

- Success: the slot's `QLineEdit` value transitions from `"None"` to a figure name. Watch the `ValuePattern::get_value()` on a short poll, or subscribe to UIA `PropertyChangedEvent` for the value property.
- Failure (file malformed, lock, already on portal): a modal `QMessageBox` top-level window appears. Catch any new child Window and treat its text as the error; click OK and surface failure to the phone.

## Off-screen strategy (open question)

The spec's "don't interrupt game flow" goal wants the dialog hidden. Two options surfaced by the tree:
1. **Minimize** the RPCS3 main window, or
2. **Move** the dialog via `WindowPattern` / `TransformPattern` to coordinates outside the visible desktop.

Not tested yet. Both are feasible on paper (UIA `TransformPattern.Move` works on Qt windows). Concern: the game viewport is a child of the main window too, so minimizing the main window will also black out the game. The correct target is the dialog itself, not the main window. **To verify in 1a-follow-up:** can we `Move` the Qt dialog to, say, `-2000, -2000` while UIA still finds it and its children?

## Incidental discoveries

- **Game serial detection works from the main window's game-list table** — columns "Name", "Serial" are UIA `DataItem`s. The user's library includes `BLUS30968 Skylanders Giants`, `BLUS31076 Swap Force`, `BLUS31442 Trap Team`, `BLUS31545 SuperChargers`. Auto-detection per the spec's Q62 answer is plausible without touching config files, though reading RPCS3's `games.yml` is cleaner.
- **RPCS3 version** is visible in the window title string; a regex lift answers the Q72 version-check requirement without shell-executing the binary.

## Drive-test results (end-to-end via `tools/uia-drive/`)

Loaded `Eruptor.sky` into slot 1 end-to-end:

| Step                                           | Elapsed    |
|------------------------------------------------|-----------:|
| Invoke Load → file dialog appears              |    554 ms  |
| `ValuePattern::set_value` on file-name edit    |      2 ms  |
| Invoke Open → slot edit reflects figure name   |     71 ms  |
| **Total (including tree walks)**               | **861 ms** |

Comfortably under the 1.5s target. The `ValuePattern::get_value()` polling loop on the slot's `QLineEdit` is a clean spinner-completion signal — 30ms poll interval was enough.

File dialog confirmed:
- Class `#32770` (standard Win32 common dialog).
- Edit AutomationId `1148` (inside a ComboBox of the same id) — Value pattern for setting the path.
- Open button AutomationId `1` (Win32 IDOK) — Invoke pattern.

Off-screen move: **UIA's `TransformPattern.move_to(-4000,-4000)` reported success and UIA still found/read children, but the OS window did not move visually.** Qt's QDialog appears to intercept the UIA transform, or the TransformPattern operates in UIA-tree-local coordinates. **Phase 2 must move the OS window via raw Win32 `SetWindowPos`** using the `NativeWindowHandle` property exposed by the UIA element. Example:

```rust
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{SetWindowPos, SWP_NOSIZE, SWP_NOZORDER};

let hwnd = HWND(dialog.get_native_window_handle()? as _);
SetWindowPos(hwnd, None, -4000, -4000, 0, 0, SWP_NOSIZE | SWP_NOZORDER)?;
```

UIA accessibility continues to work across SetWindowPos on other Qt apps; we'll confirm on RPCS3 specifically in Phase 2 but it's a very well-trodden path.

## Game catalogue + launching (incidental to 1a, closes 1b)

RPCS3 maintains `<install>/config/games.yml` as a flat mapping of serial → game root path:

```yaml
BLUS30968: C:/games/ps3/Skylanders Giants/
BLUS31076: C:/games/ps3/Skylanders Swap Force/
BLUS31442: C:/games/ps3/Skylanders Trap Team/
BLUS31545: C:/games/ps3/Skylanders Superchargers/
BLUS31600: C:/games/ps3/Skylanders Imaginators/
```

Read this at first launch (plus on refresh) to auto-detect installed Skylanders titles. Each game root has `PS3_GAME/USRDIR/EBOOT.BIN` as the launch target.

CLI launch: `rpcs3.exe "<game_root>/PS3_GAME/USRDIR/EBOOT.BIN"` — spawns the game with GUI (Manage menu stays accessible; the portal dialog works during play). Do **not** pass `--no-gui`: per user comment and our spike, the Skylanders Manager dialog is only accessible via the GUI menu bar.

Useful flags:
- `--fullscreen` — only takes effect when `--no-gui` is also set. For us, fullscreen is driven by the game/RPCS3 config, not our CLI.
- `--config <path>` — per-game config override. Probably useful later for standardising graphics settings.
- `--game-screen <index>` — force specific monitor. Potentially useful given the HTPC's single large TV.

## RPCS3 version detection

The main window title string is `RPCS3 0.0.40-19203-0b9c53e2 Alpha | master` — parseable via regex. No need to shell out; UIA already gives us the title.

## Remaining 1a/1b items (all Phase 2, not blockers)

- Actually use Win32 `SetWindowPos` to hide the dialog off-screen; confirm accessibility still works on RPCS3 specifically.
- Handle the error-modal path (`QMessageBox` if load fails) — dismiss and propagate the message to the phone.
- Handle the main-window-minimized case — does UIA still resolve children? Probably yes (UIA is independent of visibility), confirm in Phase 2.
- Make the driver tolerant of dialog-not-open (auto-trigger via Manage menu). For now the Phase 2 `UiaPortalDriver::open_dialog()` does that.

## Phase 2 plan for the RPCS3 control module

Crate `crates/rpcs3-control/` exposes:

```rust
pub trait PortalDriver {
    fn open_dialog(&self) -> Result<()>;
    fn read_slots(&self) -> Result<[SlotState; 8]>;
    fn load(&self, slot: u8, path: &Path) -> Result<()>;
    fn clear(&self, slot: u8) -> Result<()>;
}

pub struct UiaPortalDriver { /* UIA automation + cached element handles */ }
```

- Re-resolve widgets on every call (cheap; tolerates dialog-recreation on `WA_DeleteOnClose`).
- Serialize actions via a tokio mutex — Qt dialogs aren't re-entrant from external driving.
- Emit structured `tracing` events at every step (open, find-row, invoke, value-poll, success/failure) — the e2e test harness will tail this.
- Feature flag a `MockPortalDriver` for integration tests that don't need RPCS3.

## Probe tool

`tools/uia-probe/` (this spike's builder). Usage:

```
cargo run --manifest-path tools/uia-probe/Cargo.toml --release -- "RPCS3"
cargo run --manifest-path tools/uia-probe/Cargo.toml --release -- "Skylanders Manager"
```

Keep the tool around; we'll re-run it against future RPCS3 versions as a regression check.
