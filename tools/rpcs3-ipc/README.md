# rpcs3-ipc — manual smoke-test tools for the patched-RPCS3 IPC channel

Throwaway Python clients used to validate the Phase-16 patch series (`rpcs3-patches/`)
against a running patched RPCS3, independent of the Rust controller. The production
client is the Rust `IpcPortalDriver` (PLAN 16.5); these stay around for quick
by-hand checks of a fresh patched build.

The patched RPCS3 listens on an **AF_UNIX** socket (path via `$SKYLANDER_IPC_PATH`,
default `%TEMP%\rpcs3-skylander.sock` on Windows, `/tmp/rpcs3-skylander.sock`
elsewhere). Windows CPython has no `socket.AF_UNIX`, so these call `ws2_32` directly
via `ctypes` (AF_UNIX is an OS feature since Win10 1803).

## Protocol (newline-terminated ASCII)

```
PING                  -> PONG
STATE                 -> OK status=<s> frames=<n> progr=<a>/<b> seg=<c>/<d>
STATUS                -> OK 0:<serial|empty> ... 7:<serial|empty>
WINDOW                -> OK handle=<hex>     (native game-window HWND; 0 until created)
LOAD <abs .sky path>  -> OK slot=<n> | ERR <reason>
CLEAR <slot 0-7>      -> OK | ERR <reason>
```
The server also pushes `HB ...` heartbeat lines ~1/s (same fields as `STATE`).

## Usage

```bash
python tools/rpcs3-ipc/skylander_ipc_test.py PING
python tools/rpcs3-ipc/skylander_ipc_test.py STATE
python tools/rpcs3-ipc/skylander_ipc_test.py LOAD "C:\path\to\working\figure.sky"
python tools/rpcs3-ipc/skylander_ipc_test.py CLEAR 0
python tools/rpcs3-ipc/skylander_ipc_test.py --watch 6        # print 6 lines incl. heartbeats
python tools/rpcs3-ipc/skylander_ipc_test.py --path C:\custom\x.sock STATE

# P2: read the window handle over IPC, then reposition the window (no focus-steal)
python tools/rpcs3-ipc/p2_window_demo.py --path D:\workspace\rpcs3\rpcs3-skylander.sock 80 80 1280 720
```

Bring up a patched RPCS3 first — see `docs/dev/rpcs3-fork-htpc-bringup.md` (build)
and the build/run batch files in the dev clone (`D:\workspace\rpcs3`).
