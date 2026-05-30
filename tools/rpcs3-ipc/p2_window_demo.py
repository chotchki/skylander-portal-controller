#!/usr/bin/env python3
r"""P2 spike demo: read the game window handle over the Skylander IPC channel, then
reposition the window via Win32 SetWindowPos (no focus-steal). Proves the controller
gets a real, positionable native handle.

Usage:
  python p2_window_demo.py --path D:\workspace\rpcs3\rpcs3-skylander.sock 80 80 1280 720
"""
import argparse, ctypes, os, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import skylander_ipc_test as ipc  # reuse AF_UNIX ws2_32 connect/read

user32 = ctypes.WinDLL("user32", use_last_error=True)


class RECT(ctypes.Structure):
    _fields_ = [("left", ctypes.c_long), ("top", ctypes.c_long),
                ("right", ctypes.c_long), ("bottom", ctypes.c_long)]


user32.GetWindowRect.argtypes = [ctypes.c_void_p, ctypes.POINTER(RECT)]
user32.SetWindowPos.argtypes = [ctypes.c_void_p, ctypes.c_void_p,
                                ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_uint]


def get_handle(path):
    s = ipc.connect(path)
    try:
        ws = ipc.ws2
        ws.send(s, b"WINDOW\n", 7, 0)
        for ln in ipc.read_lines(s, 8):
            if ln.startswith("OK handle="):
                return int(ln.split("=", 1)[1], 16)
    finally:
        ipc.ws2.closesocket(s)
    return 0


def rect(hwnd):
    r = RECT()
    user32.GetWindowRect(ctypes.c_void_p(hwnd), ctypes.byref(r))
    return (r.left, r.top, r.right - r.left, r.bottom - r.top)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--path", default=ipc.default_path())
    ap.add_argument("x", type=int)
    ap.add_argument("y", type=int)
    ap.add_argument("w", type=int)
    ap.add_argument("h", type=int)
    a = ap.parse_args()

    h = get_handle(a.path)
    if not h:
        sys.exit("no window handle from IPC (handle=0 — is the game window created yet?)")
    print(f"handle = 0x{h:X}  (from IPC WINDOW)")
    print("before:", rect(h))

    SWP_NOZORDER, SWP_NOACTIVATE, SWP_SHOWWINDOW = 0x0004, 0x0010, 0x0040
    ok = user32.SetWindowPos(ctypes.c_void_p(h), None, a.x, a.y, a.w, a.h,
                             SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW)
    if not ok:
        sys.exit(f"SetWindowPos failed: GetLastError={ctypes.get_last_error()}")
    print("after: ", rect(h), "<- moved/resized via the IPC-reported handle, no focus-steal")


if __name__ == "__main__":
    main()
