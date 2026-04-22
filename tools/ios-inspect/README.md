# ios-inspect

Mac-only dev tool for iterating on iOS-specific phone UI bugs against the
iOS Simulator without a human in the loop. Drives the simulator via
`xcrun simctl` and Safari via `ios-webkit-debug-proxy` + the WebKit Web
Inspector protocol.

See [PLAN.md §4.21](../../PLAN.md) for motivation.

## Prereqs (macOS only)

- Xcode + at least one iOS runtime (Xcode → Settings → Platforms).
- `brew install ios-webkit-debug-proxy`.
- `trunk` (for serving `phone/` — already installed if you work on the
  phone SPA).

## Usage

```sh
# One-time build:
cargo build --manifest-path tools/ios-inspect/Cargo.toml

# Alias the binary for brevity:
alias ios-inspect=$PWD/tools/ios-inspect/target/debug/ios-inspect
```

Typical session:

```sh
# Serve the phone SPA (background tab / other terminal):
cd phone && trunk serve --address 0.0.0.0 --port 8090

# Boot a sim + start the proxy.
ios-inspect boot
# (auto-picks the newest Dynamic-Island iPhone; override with
#  `--device "iPhone 17 Pro"` if needed)

# Load the SPA in the sim's Safari:
ios-inspect open http://192.168.1.155:8090/

# Probe:
ios-inspect eval 'window.innerHeight'
ios-inspect computed-style .app --filter "height,padding-bottom,background-color"
ios-inspect dump-dom --selector .conn-lost-overlay
ios-inspect screenshot -o /tmp/x.png           # full sim frame
ios-inspect screenshot --web-only -o /tmp/y.png # just the viewport
ios-inspect tabs                                 # list Safari tabs

# Tear down:
ios-inspect shutdown
```

## Self-healing proxy

The `webinspectord_sim` socket path is ephemeral — when the proxy dies,
the OS restarts the daemon under a fresh `launchd.*` path (sometimes
under `/private/tmp/`, sometimes `/private/var/tmp/`). On every
proxy-touching command the tool `lsof`s for the live socket and silently
restarts the proxy if the cached path has drifted. You should never need
to re-run `boot` mid-session.

## Known limitations

- **Simulator fidelity gap.** `safe-area-inset-bottom` reports `0` in
  sim Safari (non-standalone), versus ~34 px on real Dynamic Island
  hardware. Bugs that depend on non-zero safe-area insets need either
  PWA-standalone mode (Add to Home Screen inside the sim) or a
  real-device fallback.
- **WebKit vs Chrome CDP.** This is not Chrome's DevTools Protocol.
  WebKit's dialect:
  - Wraps per-page commands in `Target.sendMessageToTarget`.
  - Has no `DOM.enable` / `CSS.enable` (always-on; calling them errors).
  - Uses `DOM.getOuterHTML` instead of `DOM.describeNode`.
  - `Page.snapshotRect` returns a `data:image/png;base64,…` dataURL,
    not a binary blob; payload can exceed 1 MB so the WS
    `max_message_size` is set to 32 MB.
- **Single tab at a time.** If multiple Safari tabs are open the tool
  picks the most recently-registered one. No `--tab` override yet.
- **One simulator at a time.** `boot` auto-picks the newest available
  Dynamic-Island iPhone runtime; pass `--device` to override.
