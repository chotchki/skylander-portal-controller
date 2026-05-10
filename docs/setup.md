---
layout: page
title: Setup and requirements
permalink: /setup/
---

This project intentionally ships as a thin wrapper. It does not bundle the emulator, the games, or the Skylanders firmware. Users bring their own.

## What you need

### Hardware

- A **Windows 11 PC** for the full HTPC experience (drives a real RPCS3), or an **Apple Silicon Mac** running macOS 14+ for a mock-driver build (no live emulator — useful for demo, family-member play, or development).
- A large TV is the target form factor on Windows. The launcher UI is tuned for lean-back viewing from across the room.
- A phone or tablet on the same Wi-Fi network. iOS Safari and Android Chrome are the tested targets.

### Software you install yourself

- **[RPCS3](https://rpcs3.net/)**, the PS3 emulator, installed and working. You should be able to boot a Skylanders game in it before installing this project. RPCS3 also needs the PS3 system firmware, which you supply per [RPCS3's own setup guide](https://rpcs3.net/quickstart). *Mac users can skip RPCS3* — the macOS build only ships the mock driver, so a fake `rpcs3` path is enough.
- **Your own backup of Skylanders figure firmware.** This app reads `.sky` files dumped from physical toys you own. Dumping your own figures is out of scope for this project. The [Portal Authority](https://portal-authority.fandom.com/) and [dumping wiki pages](https://rpcs3.net/wiki/Help%3A_Skylanders) cover the tools and hardware.
- **The Skylanders games themselves**, installed into RPCS3. You dump those from physical discs you own, per RPCS3's normal disc-dump flow. Supported serials: BLUS30779 (SSA), BLUS30968 (Giants), BLUS31076 (Swap Force), BLUS31442 (Trap Team), BLUS31545 (SuperChargers), BLUS31600 (Imaginators).

### Why we do not bundle any of that

RPCS3 is freely distributable but changes fast and users already install it for other reasons. Re-distributing it would pin you to whatever version happened to be current when we cut a release. More importantly, game ISOs and figure firmware dumps are copyrighted material. Bundling or distributing either would make this a piracy tool rather than a legitimate accessibility wrapper. Not a road we are going down.

## First-launch configuration

On first run the app does a tiny one-time config flow from the HTPC keyboard:

1. Point it at your `rpcs3.exe`.
2. Point it at the root folder of your firmware-pack backup. Expected layout is `{Game}/{Element}/[Alternate types/]{Name}.sky`, which matches the common community pack layouts.
3. The app scans RPCS3's installed-games list and surfaces whichever of the six supported Skylanders titles you have.

After that, every subsequent launch goes straight to the QR code. First-launch state is kept on disk, so you do this once per HTPC.

## Steam Big Picture integration

The app is designed to be added to Steam as a non-Steam game and launched from Big Picture as a "third game" (alongside RPCS3 itself). It starts up, shows the QR on the TV, and launches RPCS3 only once a phone has picked a game. Steam-shell edge cases are tracked as a compatibility-pass concern rather than a day-one constraint — if you run without Steam, the app still works.

## Distribution

Releases ship on [GitHub Releases](https://github.com/chotchki/skylander-portal-controller/releases). Each release attaches two artifacts:

- **Windows:** `skylander-portal-controller-vX.Y.Z-windows-x86_64.zip` — single `skylander-portal.exe` with all assets (phone SPA, fonts, figure metadata, element icons) embedded into the binary. This is the production target with the UIA driver against a real RPCS3.
- **macOS:** `skylander-portal-controller-vX.Y.Z-macos-arm64.tar.gz` — single `skylander-portal-controller` binary, same embedded assets. Apple Silicon only. The macOS build ships the mock driver only — there's no AXUIElement-based equivalent of the Windows UI Automation driver, so a Mac binary can't talk to a real RPCS3. It's still useful for demo, family-member play, dev iteration, and the iOS-Simulator e2e harness.

### Mac install notes

`tar.gz` extracts to a single binary. The first time you run it, macOS Gatekeeper will refuse to launch the binary because it isn't code-signed. **Right-click the binary in Finder, choose "Open", then click "Open" in the dialog.** That records an exception so subsequent launches work normally. (Code signing + notarization is post-MVP — see the [roadmap]({{ '/roadmap/' | relative_url }}).)

The Mac binary skips the first-launch wizard entirely. On first run it writes a sensible `config.json` to `~/Library/Application Support/skylander-portal-controller/` (mock driver, no RPCS3 path, no firmware pack required) and goes straight to serving the QR. If you want to point it at a `.sky` firmware pack later, edit the `firmware_pack_root` field in that file and restart. SQLite db, logs, and working-copy `.sky` files all live under the same per-user directory.

## Running from source

If you want to hack on this or run it before a formal release exists:

```
git clone https://github.com/chotchki/skylander-portal-controller.git
cd skylander-portal-controller
cp .env.dev.example .env.dev
# edit .env.dev to point at your RPCS3 + firmware pack
cd phone && trunk build && cd ..
cargo run
```

Needs: Rust toolchain with the `wasm32-unknown-unknown` target, [`trunk`](https://trunkrs.dev/), Windows 11 for the UI-Automation driver. See the repo [README](https://github.com/chotchki/skylander-portal-controller#running-in-dev) for the full dev setup.

{% comment %}
TODO: screenshots of the first-launch config flow and the Steam
Big Picture launch target.
{% endcomment %}
