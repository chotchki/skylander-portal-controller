---
layout: page
title: Setup and requirements
permalink: /setup/
---

This project intentionally ships as a thin wrapper. It does not bundle the emulator, the games, or the Skylanders firmware. Users bring their own.

## What you need

### Hardware

- A Windows 11 PC. The target deployment is an HTPC hooked to a TV, with Steam in Big Picture mode acting as the shell.
- An 86-inch TV is the form factor the launcher UI is tuned for. Any large display works; the launcher just targets that reading distance.
- A phone or tablet on the same Wi-Fi network as the HTPC. iOS Safari and Android Chrome are the tested targets.

### Software you install yourself

- **[RPCS3](https://rpcs3.net/)**, the PS3 emulator, installed and working. You should be able to boot a Skylanders game in it before installing this project. RPCS3 also needs the PS3 system firmware, which you supply per [RPCS3's own setup guide](https://rpcs3.net/quickstart).
- **Your own backup of Skylanders figure firmware.** This app reads `.sky` files dumped from physical toys you own. Dumping your own figures is out of scope for this project. The [Portal Authority](https://portal-authority.fandom.com/) and [dumping wiki pages](https://rpcs3.net/wiki/Help%3A_Skylanders) cover the tools and hardware.
- **The Skylanders games themselves**, installed into RPCS3. You dump those from physical discs you own, per RPCS3's normal disc-dump flow. Supported serials: BLUS30906 (SSA), BLUS30968 (Giants), BLUS31076 (Swap Force), BLUS31442 (Trap Team), BLUS31545 (SuperChargers), BLUS31600 (Imaginators).

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

Releases ship as a zip on [GitHub Releases](https://github.com/chotchki/skylander-portal-controller/releases). Current plan is a single `skylander-portal.exe` with all assets (phone SPA, fonts, figure metadata, element icons) embedded into the binary. See the [roadmap]({{ '/roadmap/' | relative_url }}) for where that stands.

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
