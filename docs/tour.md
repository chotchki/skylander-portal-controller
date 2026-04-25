---
layout: page
title: Tour
permalink: /tour/
---

A walkthrough of the phone interface, captured straight from the e2e test harness — same screens any kid sees, same heraldic gold-bezel UI, no mockups.

<div class="tour-grid">

<figure class="tour-shot">
  <div class="tour-bezel">
    <img src="{{ '/assets/screens/01-profile-picker.png' | relative_url }}" alt="Profile picker">
  </div>
  <figcaption>
    <span class="tour-step">01</span>
    <strong>Welcome, portal master.</strong>
    First screen after a phone scans the QR code on the TV. Each kid gets their own profile bezel, gated by a 4-digit PIN so a sibling can't nuke their save.
  </figcaption>
</figure>

<figure class="tour-shot">
  <div class="tour-bezel">
    <img src="{{ '/assets/screens/02-pin-entry.png' | relative_url }}" alt="PIN entry">
  </div>
  <figcaption>
    <span class="tour-step">02</span>
    <strong>Three strikes, you're out.</strong>
    Tap the profile, punch in the PIN. Three wrong tries gates the profile for 5 seconds — anti-sibling, not anti-adversary.
  </figcaption>
</figure>

<figure class="tour-shot">
  <div class="tour-bezel">
    <img src="{{ '/assets/screens/03-game-picker.png' | relative_url }}" alt="Game picker">
  </div>
  <figcaption>
    <span class="tour-step">03</span>
    <strong>Pick a game, any game.</strong>
    The six PS3-era Skylanders titles, surfaced from whatever's installed in your local RPCS3 library. Tap one and the launcher boots it on the TV behind you.
  </figcaption>
</figure>

<figure class="tour-shot">
  <div class="tour-bezel">
    <img src="{{ '/assets/screens/04-portal-empty.png' | relative_url }}" alt="Empty portal">
  </div>
  <figcaption>
    <span class="tour-step">04</span>
    <strong>An honest empty state.</strong>
    Empty portal slots used to be visible bezels — kids tapped them and nothing happened. The v1.1.0 fix hides them entirely; the toy-box hint is the only thing to tap when there's no figure on the portal.
  </figcaption>
</figure>

<figure class="tour-shot">
  <div class="tour-bezel">
    <img src="{{ '/assets/screens/05-toy-box.png' | relative_url }}" alt="Toy box overlay">
  </div>
  <figcaption>
    <span class="tour-step">05</span>
    <strong>Open the toy box.</strong>
    The whole owned collection, filterable by element, by game-of-origin, by category, plus a free-text search. Reposes collapse behind a "N variants" badge so a stack of Spyros doesn't drown out everyone else.
  </figcaption>
</figure>

<figure class="tour-shot">
  <div class="tour-bezel">
    <img src="{{ '/assets/screens/06-figure-detail.png' | relative_url }}" alt="Figure detail">
  </div>
  <figcaption>
    <span class="tour-step">06</span>
    <strong>Pick your champion.</strong>
    Tap a card and you see the figure detail — name, variant cycle, level / gold / playtime stats parsed straight off the firmware dump. PLACE ON PORTAL drives the RPCS3 dialog automation behind the scenes.
  </figcaption>
</figure>

<figure class="tour-shot">
  <div class="tour-bezel">
    <img src="{{ '/assets/screens/07-portal-loaded.png' | relative_url }}" alt="Portal with a placed figure">
  </div>
  <figcaption>
    <span class="tour-step">07</span>
    <strong>On the portal.</strong>
    The slot shows up in its bezel; the gold ownership chip identifies whose figure it is. Tap the slot to remove. Co-op players can each see whose figure is whose at a glance.
  </figcaption>
</figure>

<figure class="tour-shot">
  <div class="tour-bezel">
    <img src="{{ '/assets/screens/08-menu-overlay.png' | relative_url }}" alt="Menu overlay">
  </div>
  <figcaption>
    <span class="tour-step">08</span>
    <strong>Hand the phone off.</strong>
    The kebab menu carries the join QR (so an existing player can invite a new joiner) and the per-profile actions: switch profile, manage profiles, hold-to-switch-games, hold-to-shut-down. The ENABLE KAOS toggle lives here too.
  </figcaption>
</figure>

<figure class="tour-shot">
  <div class="tour-bezel">
    <img src="{{ '/assets/screens/09-kaos-swap.png' | relative_url }}" alt="Kaos mid-game swap">
  </div>
  <figcaption>
    <span class="tour-step">09</span>
    <strong>KAOS strikes!</strong>
    With Kaos opt-in turned on, the timer fires somewhere in every hour after the 20-minute warmup. A random placed figure swaps for a compatible random pick from the collection, taunt overlay lands for ~5 seconds, then vanishes. Vehicles only swap in for SuperChargers; figures only swap into games at or after their game-of-origin.
  </figcaption>
</figure>

<figure class="tour-shot">
  <div class="tour-bezel">
    <img src="{{ '/assets/screens/10-kaos-takeover.png' | relative_url }}" alt="Kaos takeover">
  </div>
  <figcaption>
    <span class="tour-step">10</span>
    <strong>Kaos reigns.</strong>
    A third connection evicted this session via the FIFO seat policy. Kickback button greys out and counts down the 60-second forced-evict cooldown, so the player isn't tapping into a wall.
  </figcaption>
</figure>

</div>

> Captured by `cargo test -p skylander-e2e-tests --test screenshot_tour -- --ignored --nocapture`. The harness drives a real Chrome via WebDriver against a real server with the mock RPCS3 driver, so every screen is exactly what a kid sees on the iPad — no mockups, no Photoshop, no marketing pixels.
