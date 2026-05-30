#!/usr/bin/env python3
r"""P1 spike test client for the patched-RPCS3 Skylander AF_UNIX IPC listener.

Windows CPython has no socket.AF_UNIX, so we call ws2_32 directly via ctypes
(AF_UNIX is supported by the OS since Win10 1803). `import socket` triggers the
process WSAStartup that ws2_32 needs.

Protocol (newline-terminated ASCII):
  PING                 -> PONG
  STATE                -> OK status=<s> frames=<n> progr=<a>/<b> seg=<c>/<d>
  STATUS               -> OK 0:<serial|empty> ... 7:<...>
  LOAD <abs .sky path> -> OK slot=<n> | ERR <reason>
  CLEAR <slot 0-7>     -> OK | ERR <reason>
The server also pushes `HB ...` heartbeat lines ~1/s; this client skips them when
waiting for a command reply, or prints them in --watch mode.

Usage:
  python skylander_ipc_test.py PING
  python skylander_ipc_test.py STATE
  python skylander_ipc_test.py LOAD "C:\path\to\working\figure.sky"
  python skylander_ipc_test.py CLEAR 0
  python skylander_ipc_test.py --watch 6        # print 6 lines (watch heartbeats)
  python skylander_ipc_test.py --path C:\custom\x.sock STATE
"""
import argparse, ctypes, os, socket, sys  # noqa: F401  (socket import => WSAStartup)

AF_UNIX, SOCK_STREAM = 1, 1
INVALID = (1 << 64) - 1  # 64-bit SOCKET

ws2 = ctypes.WinDLL("ws2_32.dll")
ws2.socket.restype = ctypes.c_uint64
ws2.socket.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_int]
ws2.connect.restype = ctypes.c_int
ws2.connect.argtypes = [ctypes.c_uint64, ctypes.c_void_p, ctypes.c_int]
ws2.send.restype = ctypes.c_int
ws2.send.argtypes = [ctypes.c_uint64, ctypes.c_char_p, ctypes.c_int, ctypes.c_int]
ws2.recv.restype = ctypes.c_int
ws2.recv.argtypes = [ctypes.c_uint64, ctypes.c_char_p, ctypes.c_int, ctypes.c_int]
ws2.closesocket.argtypes = [ctypes.c_uint64]
ws2.WSAGetLastError.restype = ctypes.c_int


class sockaddr_un(ctypes.Structure):
    _fields_ = [("sun_family", ctypes.c_ushort), ("sun_path", ctypes.c_char * 108)]


def default_path():
    tmp = os.environ.get("TEMP", r"C:\Windows\Temp")
    return os.path.join(tmp, "rpcs3-skylander.sock")


def connect(path):
    s = ws2.socket(AF_UNIX, SOCK_STREAM, 0)
    if s == INVALID:
        sys.exit(f"socket() failed: WSA {ws2.WSAGetLastError()}")
    a = sockaddr_un(sun_family=AF_UNIX)
    a.sun_path = path.encode("utf-8")
    if ws2.connect(s, ctypes.byref(a), ctypes.sizeof(a)) != 0:
        sys.exit(f"connect('{path}') failed: WSA {ws2.WSAGetLastError()} "
                 f"(is a Skylanders game running in the patched RPCS3?)")
    return s


def read_lines(s, want):
    """Read until `want` complete lines are collected; return list of lines."""
    out, buf = [], b""
    chunk = ctypes.create_string_buffer(4096)
    while len(out) < want:
        n = ws2.recv(s, chunk, 4096, 0)
        if n <= 0:
            break
        buf += chunk.raw[:n]
        while b"\n" in buf:
            line, buf = buf.split(b"\n", 1)
            out.append(line.decode(errors="replace").rstrip("\r"))
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--path", default=default_path())
    ap.add_argument("--watch", type=int, default=0, help="print N lines (heartbeats) and exit")
    ap.add_argument("cmd", nargs=argparse.REMAINDER)
    a = ap.parse_args()

    s = connect(a.path)
    try:
        if a.watch:
            for ln in read_lines(s, a.watch):
                print(ln)
            return
        if not a.cmd:
            sys.exit("no command (or use --watch N)")
        ws2.send(s, (" ".join(a.cmd) + "\n").encode(), len(" ".join(a.cmd)) + 1, 0)
        # skip heartbeat lines; print the first command reply
        for ln in read_lines(s, 8):
            if not ln.startswith("HB "):
                print(ln)
                return
        print("(no non-heartbeat reply received)")
    finally:
        ws2.closesocket(s)


if __name__ == "__main__":
    main()
