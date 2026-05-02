# ios-inspect

Mac-only dev tool for iterating on iOS-specific phone UI bugs against the
iOS Simulator without a human in the loop. Drives the simulator via
`xcrun simctl` and Safari via `ios-webkit-debug-proxy` + the WebKit Web
Inspector protocol.

See [PLAN.md §4.21](../../PLAN.md) for the original motivation;
multi-device (iPad + iPhone simultaneously) is [PLAN 10.2](../../PLAN.md).

## Prereqs (macOS only)

- Xcode + at least one iOS runtime (Xcode → Settings → Platforms).
  For multi-device work you'll want both an iPhone runtime and an iPad
  runtime installed.
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

### Single-device session (legacy default)

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

### Multi-device session (iPad + iPhone)

`boot` accepts `--device` repeatedly. Each booted device gets its own
proxy on a unique port pair (`9221+0/+1`, `+2/+3`, …) so commands can
target a specific device or fan out to all of them.

```sh
# Boot both:
ios-inspect boot --device "iPhone 17 Pro" --device "iPad Pro 13-inch (M5)"
# (cold boot of an iPad runtime can take 30–90 s — give it room.)

# Open the same URL on both:
ios-inspect open http://christophers-macbook-pro.local:8765/

# Fan-out probe — output is prefixed with the device label:
ios-inspect eval '[window.innerWidth, window.innerHeight]'
# [iphone-17-pro] [402,714]
# [ipad-pro-13-inch-m5] [1032,1290]

# Target one device by name (substring, case-insensitive) or UDID:
ios-inspect eval --device ipad 'document.title'
ios-inspect computed-style --device iphone .pwa-hint --filter display

# Paired screenshots — pass --output as a directory; each device writes
# <label>.png into it:
mkdir -p /tmp/ios-shots
ios-inspect screenshot --output /tmp/ios-shots --web-only
# → /tmp/ios-shots/iphone-17-pro.png  (402×714)
# → /tmp/ios-shots/ipad-pro-13-inch-m5.png  (1032×1290)

# Single-device screenshot still takes a file path:
ios-inspect screenshot --device iphone --output /tmp/iphone.png --web-only

# Shutdown tears down every booted device + its proxy:
ios-inspect shutdown
```

`ios-inspect boot --device <new>` against an existing session adds the
new device to the running session rather than replacing it — useful for
booting the iPad after you've already started iterating on the iPhone.

State (per-device UDID, socket, proxy PID, port pair) lives in
`/tmp/ios-inspect-state.json`. Pre-10.2 single-device state files are
auto-migrated on first read.

## Self-healing proxy

The `webinspectord_sim` socket path is ephemeral — when the proxy dies,
the OS restarts the daemon under a fresh `launchd.*` path (sometimes
under `/private/tmp/`, sometimes `/private/var/tmp/`). On every
proxy-touching command the tool `lsof`s for the live sockets per device
and silently restarts the proxy if the cached path has drifted. With
multiple devices we can't always re-attribute a rotated socket back to
the right UDID — if that happens you'll get a "lost its
webinspectord_sim socket" error and need to re-run
`ios-inspect boot --device "<name>"` for that one device. Single-device
sessions don't hit this in practice.

## Known limitations

- **Simulator fidelity gap.** `safe-area-inset-bottom` reports `0` in
  sim Safari (non-standalone), versus ~34 px on real Dynamic Island
  hardware. Bugs that depend on non-zero safe-area insets need either
  PWA-standalone mode (Add to Home Screen inside the sim) or a
  real-device fallback. Same gap on iPad sim, where the bottom inset
  is always 0 anyway since iPad doesn't have a Dynamic Island.
- **WebKit vs Chrome CDP.** This is not Chrome's DevTools Protocol.
  WebKit's dialect:
  - Wraps per-page commands in `Target.sendMessageToTarget`.
  - Has no `DOM.enable` / `CSS.enable` (always-on; calling them errors).
  - Uses `DOM.getOuterHTML` instead of `DOM.describeNode`.
  - `Page.snapshotRect` returns a `data:image/png;base64,…` dataURL,
    not a binary blob; payload can exceed 1 MB so the WS
    `max_message_size` is set to 32 MB.
- **Single tab at a time per device.** If multiple Safari tabs are open
  the tool picks the most recently-registered one for each device. No
  `--tab` override yet.
- **Same-name devices across runtimes.** "iPhone 17 Pro" exists under
  iOS 26.0, 26.2, 26.4, etc. Substring matching picks the first hit by
  HashMap iteration order — non-deterministic across runs. To pin a
  specific runtime, pass the UDID instead of the name (read from
  `xcrun simctl list devices available --json`).
