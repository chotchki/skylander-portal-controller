---
layout: home
title: Skylander Portal Controller
tagline: Phone-driven portal control for an emulated Skylanders setup, built for kids who outlived their PS3 hardware.
---

A Windows app that lets kids manage an emulated Skylanders portal from a phone or tablet, while a PS3 Skylanders game runs on the TV through [RPCS3](https://rpcs3.net/).

## Who this is for

Parents who still have bins of Skylanders figures in the basement and kids who want to keep playing the games — but whose original PS3 hardware died years ago. If you already run RPCS3 on an HTPC hooked to your TV, this project replaces the alt-tab-to-a-Windows-file-picker dance with a phone interface your kids can actually use.

It is not a Skylanders emulator, a ROM dump, or a way to play games you do not own. You bring your own RPCS3 install and your own backups of the Skylanders figure firmware. See the [setup page]({{ '/setup/' | relative_url }}) for what you need.

## Why it exists

Physical Skylanders portals carry a surprising amount of hidden state. Every figure stores level, XP, gold, hats, and quest progress directly on the toy. When a figure gets lost under the couch — or quietly snatched by a sibling during an argument — that save data goes with it.

RPCS3 has a "Skylanders Manager" dialog that emulates the portal, loads figure firmware files from disk, and lets you clear or swap slots while a game runs. The problem is driving that dialog from across the living room. A tiny Qt file picker with filenames like `C:\...\Giants\Fire\Series 2 Hot Dog.sky` is not a thing a six-year-old can operate with a controller from the couch.

This app takes over that role. It runs on the HTPC, shows a QR code on the TV at startup, and serves a touch-friendly SPA to any phone on the same Wi-Fi. The kids browse figures by element and game, pick one, tap a slot, and the app drives the RPCS3 dialog for them — while the game keeps running.

## Gameplay flow

1. Launch the app from Steam Big Picture. A fullscreen launcher appears on the TV with a QR code.
2. Phone or iPad scans the QR and connects over LAN.
3. Pick a profile (one per kid, PIN-gated so siblings cannot nuke each other's save data). Pick a game.
4. App launches RPCS3 with that game. Launcher fades. Game boots on the TV.
5. Phone switches to portal-control view: eight slots plus a browsable collection of every figure you own. Tap a slot, tap a figure, figure appears on the in-game portal.
6. Reset a figure to fresh, swap in a different one, hand the phone to the next kid. Same flow until someone wants to quit.

## Project status

**v1.1.0 shipped.** Core portal control works end-to-end. Profiles, PINs, multi-phone takeover with FIFO eviction, game launching with display-mode persistence, and the Skylanders-themed aesthetic are all stable. v1.1.0 added sticky disconnects (your figures stay on the portal across a PWA backgrounding or short network blip), the Kaos surprise feature (mid-game figure swaps with text-only catchphrase overlay, opt-in per profile), and a kid-friendly empty-portal UI.

Releases: [github.com/chotchki/skylander-portal-controller/releases](https://github.com/chotchki/skylander-portal-controller/releases) — Windows zip with the phone bundle embedded. See the [roadmap]({{ '/roadmap/' | relative_url }}) for what's next.

Source: [github.com/chotchki/skylander-portal-controller](https://github.com/chotchki/skylander-portal-controller)

{% comment %}
TODO: add launcher + phone screenshots once the aesthetic pass is locked.
Target slots:
- Hero: eframe launcher window on the TV showing QR + figure count.
- Phone: profile picker, game picker, portal view with 8 slots.
- Phone: toy-box overlay (browse view).
{% endcomment %}
