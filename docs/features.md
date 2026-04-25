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
- Empty portal slots stay hidden — the toy-box arrow hint is the only call-to-action when nothing is placed, so a kid tapping around doesn't get the false-affordance impression that empty bezels do something. Slots reappear as figures land.
- Browse the full owned collection filtered by element (fire, water, air, earth, life, undead, magic, tech, light, dark), by game of origin (SSA, Giants, Swap Force, Trap Team, SuperChargers, Imaginators), and by category (figures, traps, vehicles, sidekicks, items, creation crystals). The library is auto-filtered to figures compatible with whatever game is currently booted.
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
- One-minute cooldown applies only to forced evictions, to stop ping-pong. The kickback button greys out and counts down the seconds remaining so the player knows when they can come back, instead of tapping a button that would just bounce off the cooldown.
- Joining into a free seat has no cooldown.
- Kick-back re-locks the profile. PIN re-entry is required before the figure collection comes back.

## Sticky disconnects (v1.1.0)

- A phone going to sleep, switching apps, or briefly losing Wi-Fi no longer clears that profile's figures off the portal. The session is *ghosted* — the figures stay placed and the slot ownership stays attributed.
- When the phone reconnects (within an hour), the server adopts the ghost back into a live session and replays anything the phone missed during the gap (Kaos taunts, slot changes triggered by the other phone, error toasts).
- A third phone joining still evicts the oldest seat — ghost or live. If the seat was a ghost, its figures clear with the eviction. The 2-phone seat cap stays honest under co-op pressure.
- The TV launcher's player-orbit pip dims for a ghosted session, so co-op players can tell at a glance which phone is responsive right now.

## Kaos surprise (v1.1.0)

- Per-profile opt-in toggle in the kebab menu. Off by default while we tune the cadence and the compatibility heuristic in real play.
- 20-minute warmup from session unlock, then a swap fires at a uniformly random offset between 1 minute and 1 hour later — and re-arms with another random offset after each fire. Disruption, not spam.
- A random figure on the portal gets swapped for a different compatible figure from the owned collection. The app drives this through the same portal automation as a manual swap, so it looks and plays identically from the game's point of view. Vehicles only swap in for SuperChargers; figures only swap into games at or after their game-of-origin.
- Text-only overlay with Kaos-voice catchphrases; no audio (to avoid copyright issues). Auto-dismisses after about five seconds, or tap to dismiss early.
- The taunt routes through the ghost session's replay buffer too — if the targeted phone happened to be backgrounded when the swap fired, the catchphrase still lands when they come back.

## Under the hood

- RPCS3's Qt dialog is hidden off-screen while the app drives it via Windows UI Automation. Users never see the dialog flicker during a figure swap. The game keeps focus.
- Per-game preferred display mode is persisted after each successful boot, so the next launch can pre-set the display before RPCS3 starts and avoid resolution-flash flicker on the TV.
- Figure metadata (name, element, game, type, hero image, element icon) is committed into the repo, sourced from the Skylanders Fandom wiki under Creative Commons and attributed accordingly.
- The phone never sees filesystem paths or filenames. Only stable figure IDs leave the server.
- HMAC-signed commands with a 32-byte key embedded in the join QR. Strict input validation on every command. The phone bundle is embedded into the server exe, so a single Windows binary ships everything.

{% comment %}
TODO: screenshots of the portal view, toy-box overlay, profile picker,
takeover screen. Pending the locked aesthetic pass.
{% endcomment %}
