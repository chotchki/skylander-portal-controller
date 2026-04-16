# Game launch + window management (3.6b)

Research spike into opening the Skylanders Manager dialog reliably against
RPCS3 + a running game. Motivated by the original 3.7 live-lifecycle tests
failing at `open_dialog` during early boot.

## TL;DR

- UIA `Invoke` / `ExpandCollapse` on Qt 6 menus are silent no-ops. The Manage
  menu item is in the UIA tree but has zero children until a real user
  interaction opens the menu visually. Our previous driver used Invoke and
  never actually worked against a running game.
- Keyboard navigation works. The mechanism: single Alt tap → arrow-key nav →
  Enter. UIA exposes `HasKeyboardFocus` on each MenuItem, so we verify each
  step and fail fast if RPCS3 ever reorders its menu.
- Two-window architecture: while a game runs, RPCS3 has a main window
  (menu-bar, class `Qt6110QWindowIcon`, title `"RPCS3 <version>"`) and a
  separate game-viewport window (same class, title `"FPS: ..."`). Both are
  visible; the viewport usually covers the main window.
- Menu nav requires the main window to have foreground focus. We force it
  with `AttachThreadInput` + `SetForegroundWindow` and re-assert before each
  keystroke (focus thieves: game viewport, RPCS3 update-check popup).
- Dialog opens once per RPCS3 session. We immediately sling it off-screen and
  keep it there; portal ops run against the hidden dialog. Menu nav never
  runs again until RPCS3 restarts.
- The navigation runs with the game viewport minimised and the main window
  moved to `(-4000, -4000)`. The Skylanders Manager dialog + Qt menu popups
  still flicker briefly (Qt clamps popup windows to visible screen coords even
  when the parent is off-screen). This is a once-per-session flash during
  RPCS3 boot — acceptable for MVP. Documented as a post-Kaos enhancement in
  PLAN 5.1.

## What didn't work (and why each is a dead end)

| Approach | Outcome |
|---|---|
| UIA `Invoke` / `ExpandCollapse` on the Manage menu item | Returns success, menu doesn't open visually, submenu items never populate in UIA. Qt 6's menu rendering ignores UIA patterns. |
| `SendInput` Alt+M chord | Blocked — game viewport captures input. `Alt+M` is also a chord; Qt's menu accelerator only opens on the modal "menu focus mode" triggered by a single Alt tap. |
| `PostMessage(WM_SYSCOMMAND, SC_KEYMENU, 'M')` targeting main HWND | Delivered, no visible effect. Qt doesn't honour WM_SYSCOMMAND for its menu bar. |
| `SendInput` / `PostMessage` / `SendMessage` mouse clicks at UIA-reported coords | Click landed at correct screen coords, but the main window is behind the game viewport — click hits the viewport. `SetWindowPos(HWND_TOP)` didn't change Z-order visually (Qt reasserts). |
| Keyboard accelerators inside the menu (Alt-tap → 'M') | Qt only accepts arrow keys + Enter once the menu bar is in focus mode. Accelerator letters aren't honoured. |

## The winning sequence

1. Enumerate top-level windows owned by rpcs3.exe; identify the main window
   (class `Qt6110QWindowIcon`, title prefix `"RPCS3 "`) and the optional
   viewport (same class, title prefix `"FPS:"`).
2. Save the main window's current rect (for later restore).
3. If the viewport exists, `ShowWindow(SW_MINIMIZE)` it. This stops the
   viewport from stealing keyboard focus during the nav.
4. `SetWindowPos` the main window to `(-4000, -4000)` so its menu-bar
   highlight isn't visible. The menu popups Qt spawns during navigation will
   still render at visible screen coords — Qt clamps them to the screen even
   when the parent is off-screen.
5. `AttachThreadInput(our_thread, fg_thread, true)` + `AttachThreadInput(
   our_thread, target_thread, true)` + `SetForegroundWindow(main_hwnd)`.
   Detach after. This is the classic Win32 trick to steal foreground despite
   the foreground-lock rules.
6. Re-focus (no-op if already foreground) then send a single `VK_MENU` tap.
   Assert via UIA that a MenuItem named `"File"` now has keyboard focus.
7. `VK_RIGHT` × 3 → expect `"Manage"` focused.
8. `VK_DOWN` → submenu opens, `"Virtual File System"` focused.
9. `VK_DOWN` × 3 → `"Portals and Gates"` focused.
10. `VK_RIGHT` → sub-submenu expands, `"Skylanders Portal"` focused.
11. `VK_RETURN` → dialog opens.
12. Poll UIA for a top-level Window named `"Skylanders Manager"`. As soon as
    it appears, `SetWindowPos(-4000, -4000)` via `hide::find_dialog_hwnd`.
13. Restore the main window's original rect. Restore the viewport via
    `ShowWindow(SW_RESTORE)`.

Verification via `has_keyboard_focus()` between each step means we don't
blindly count keystrokes — we know exactly where we are and fail fast with
the expected-vs-actual item name if RPCS3 ever reorders its menu.

Total nav time: ~2 seconds (9 keystrokes × 200ms settle).

## Focus-theft gotchas

- **Game viewport**: minimised during nav, restored after. User sees a ~2s
  taskbar-blip.
- **RPCS3 update-check popup**: can spawn during boot and steal foreground.
  We re-assert `SetForegroundWindow` before every keystroke; if the popup
  grabs focus mid-nav a step will fail the expected-focus check and we get a
  clean error. Users should disable update checks in RPCS3's config for the
  quietest experience. (Setting → Advanced → "Automatically check for
  updates at startup" → off.)
- **`SetForegroundWindow` foreground-lock**: flaky on first call if another
  process owns foreground and our process hasn't recently received input.
  `AttachThreadInput` resolves it. Transient failures are acceptable —
  `open_dialog` just returns an error and the caller retries.

## Lockfile cleanup

RPCS3 writes a singleton lockfile (`RPCS3.buf`) next to `rpcs3.exe` on
startup, removes it on clean exit. A forced kill (e.g. `child.kill()` because
`shutdown_graceful` hit its timeout) leaves it orphaned; the next launch
bails with "Another instance of RPCS3 is running." `shutdown_graceful` now
deletes `<install_dir>/RPCS3.buf` after the Forced path.

`install_dir` is remembered on `RpcsProcess::launch`. `RpcsProcess::attach`
has no path context, so lockfile cleanup is skipped for attached processes —
fine for tests, and attached shutdown normally goes the polite WM_CLOSE route
anyway.

## Launching a game programmatically

The EBOOT-argument launch path (`rpcs3.exe <path-to-EBOOT.BIN>`) puts RPCS3
into a direct-boot mode where the menu bar does **not** respond to
synthesised keystrokes — Alt/Right work to navigate the top-level menus, but
the Down press that should open a submenu silently fails. Unsuitable for
automation.

The reliable launch recipe is to launch with no arguments (library view) and
UIA-boot the target game by serial:

1. `Command::new(exe).spawn()` — library view opens.
2. Wait for the main window (`"RPCS3 "` title prefix) to be visible.
3. Find the `DataItem` under the `game_list_table` whose name equals the
   target serial (e.g. `"BLUS30968"`).
4. `SelectionItemPattern.select()` + `UIElement.set_focus()` + synthesised
   `Enter`. UIA `Invoke` alone doesn't boot; the selection-plus-focus-plus-
   keystroke trio is what works.
5. Poll for the viewport window (`"FPS:"` title prefix) to appear.

Then the Manage menu nav (above) works normally — confirmed by
`examples/boot_game.rs` followed by `examples/open_skylanders_dialog.rs`.

## Lockfile + job object cleanup

RPCS3 can re-exec itself (elevation shim, worker spawn). A plain
`Child::kill` only terminates the Rust-tracked PID, leaking the real
emulator as a detached orphan that holds `RPCS3.buf` and the log file. Fix:
spawn → wrap in a Win32 Job Object (`CreateJobObjectW` +
`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`) → `AssignProcessToJobObject(child)`.
Closing the job handle (or explicit `TerminateJobObject`) kills the whole
process tree. `RpcsProcess::shutdown_graceful`'s Forced branch now does
both (job terminate + child kill) and sweeps `RPCS3.buf` for good measure.

## Session isolation (SSH caveat)

Win32 window handles and input queues are session-scoped. An SSH-spawned
process lives in session 0 (services); the interactive desktop is session
2+. From session 0 you can see PIDs but `EnumWindows`, UIA tree walk, and
`SendInput` all come up empty for cross-session windows. Tests that drive
real RPCS3 must run on the physical/interactive desktop, not through SSH.
RDP works but has its own focus-stealing issues — the cleanest validation
is the user sitting at the machine.

## Artefacts

- `crates/rpcs3-control/src/uia.rs::trigger_dialog_via_menu` — the
  production implementation.
- `crates/rpcs3-control/src/process.rs` — `RpcsProcess::launch` with Job
  Object wrapping; `shutdown_graceful` with Forced-path lockfile + job
  teardown.
- `crates/rpcs3-control/examples/rpcs3_windows.rs` — dumps all rpcs3.exe
  top-level windows (title, class, HWND, rect). First diagnostic to reach for
  if a future RPCS3 version breaks window identification.
- `crates/rpcs3-control/examples/dump_menu_tree.rs` — dumps every MenuItem
  under the main window + desktop-wide. Verified the "Manage menu has zero
  children until opened visually" finding.
- `crates/rpcs3-control/examples/library_probe.rs` — enumerates the
  `game_list_table` DataItems with serials. Diagnostic for future RPCS3
  versions if column layout changes.
- `crates/rpcs3-control/examples/boot_game.rs` — booting a game by serial.
  Tries UIA Invoke → Select+Focus+Enter → mouse double-click, reports which
  worked.
- `crates/rpcs3-control/examples/show_main.rs` — force-show the main window
  (tries `ShowWindow(SW_SHOWNORMAL)`). Only useful if you caught RPCS3 in
  an invisible state, which in practice means it was launched cross-session.
- `crates/rpcs3-control/examples/open_skylanders_dialog.rs` — standalone
  repro of the full winning sequence with per-step focus readouts. Useful for
  iterating if RPCS3 menu labels change; supports `--no-minimise` and
  `--hide-main` flags for testing variants.

## Open questions

1. Does Qt menu navigation timing change on slower machines? The 200ms
   inter-key pause is empirical. If a future user reports "focus drifted
   mid-nav", widen it or poll-until-expected rather than fixed-sleep.
2. Does RPCS3's update-check popup fire every launch or only periodically?
   Worth documenting the disable toggle in the user-facing README.
