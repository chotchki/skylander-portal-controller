---
layout: page
title: Features
permalink: /features/
---

# Features

What the app does today, grouped by what you actually interact with.

## Portal control

- Eight-slot portal interface matching the emulated RPCS3 portal, independent of which Skylanders game is running. Any figure works in any slot; the game itself handles compatibility (and will complain if you overload it).
- A portal view and a collection view, treated as separate surfaces. The collection is a "toy box" you open as an overlay on top of the portal.
- Browse the full owned collection filtered by element (fire, water, air, earth, life, undead, magic, tech, light, dark), by game of origin (SSA, Giants, Swap Force, Trap Team, SuperChargers, Imaginators), and by category (figures, traps, vehicles, sidekicks, items, creation crystals).
- Full-text search by canonical figure name for kids who actually know what they are looking for.
- Tap a figure to add it to a slot. Tap a figure already on the portal to remove it. No drag-and-drop.
- Reposes collapsed in the browse view behind a "N variants" badge with a per-card cycle button, so a stack of Spyro variants does not drown out the rest of the collection.

## Profiles and PINs

- Up to four profiles per household. Each has a 4-digit PIN. PINs are anti-sibling protection, not real security.
- Every profile gets its own working copies of figure firmware files, branched on first-ever use from a clean reference pack. That means one kid's Eruptor levelling up does not overwrite the other kid's Eruptor.
- Per-profile last portal layout. On session unlock the app offers "resume last setup?" and if you accept, it drives the RPCS3 dialog automatically to restore the slots.
- Reset-a-figure-to-fresh is an explicit user action. Imaginators creation crystals are never auto-reset without a confirm.
- A working copy is shared across games per-figure per-profile — one Spyro save state follows that profile through every game, matching physical portal behaviour.

## Multi-phone play

- Up to two phones connected at once. Matches co-op player count in the games.
- Each phone unlocks its own profile with its own PIN. Unlocking on one phone does not propagate.
- The portal itself is shared free-for-all. Either phone can touch any slot. A single driver serialises the actual hardware dialog, so there is no race.
- Ownership indicator on each occupied slot (profile colour and initial) so players can tell whose figure is whose.
- Any connected phone can surface the join QR code in-app so existing players can hand it to a new joiner.

## Takeover

- A third connection evicts the oldest session, FIFO. The evicted phone gets a Kaos-themed "taken over" screen with a "kick back" button that re-joins.
- One-minute cooldown applies only to forced evictions, to stop ping-pong. Joining into a free seat has no cooldown.
- Kick-back re-locks the profile. PIN re-entry is required before the figure collection comes back.

## Under the hood

- RPCS3's Qt dialog is hidden off-screen while the app drives it via Windows UI Automation. Users never see the dialog flicker during a figure swap. The game keeps focus.
- Figure metadata (name, element, game, type, hero image, element icon) is committed into the repo, sourced from the Skylanders Fandom wiki under Creative Commons and attributed accordingly.
- The phone never sees filesystem paths or filenames. Only stable figure IDs leave the server.
- Strict input validation on every command. Local-network HTTP today. HMAC-signed commands with the key embedded in the QR are next after the protocol stabilises.

## Kaos surprise (planned, held for last)

- After a 20-minute warmup, Kaos can interrupt play at random intervals within every subsequent hour.
- Text-only overlay with catchphrases; no audio (to avoid copyright issues).
- A random figure on the portal gets swapped for a different compatible figure from the owned collection. The app drives this through the same portal automation as a manual swap — so it looks and plays identically from the game's point of view.
- Dark purple / pink "mind magic" skin for the phone UI during a takeover.
- Parent kill-switch. Explicitly off by default; intentionally hidden in config, not the phone UI, so curious kids cannot re-enable it.

{% comment %}
TODO: screenshots of the portal view, toy-box overlay, profile picker,
takeover screen. Pending the locked aesthetic pass.
{% endcomment %}
