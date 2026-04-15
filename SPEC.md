# Skylander Portal Controller  
  
# Goal:  
- [ ] Produce an application to allow for the remote control of the RPCS3 emulated Skylander portal using a phone/ipad with a touch focused web interface.  
  
# Background:  
- [ ] My kids love playing skylanders games but all our old ps3 hardware is broken so we play via the RPCS3 emulator on a home theater pc using Steam Big Picture Mode.  
- [ ] Skylanders are a toys come to life game and while we have endless bins of them, the kids lose them or have them up for a display so they aren’t available to use on an actual physical portal. Fortunately RPCS3 has a screen to reconfigure a virtual portal while the game is running. Unfortunately its a huge pain to alt tab with a keyboard back and forth between the game and the virtual portal.  
  
# User:  
- [ ] My son(s) love playing old skylanders games on our HTPC. One is very young and not the most computer literate so navigating a windows file pickers from across the room is beyond him. Also we as parents are sick of having to do it.  
  
# Application Flow / Technical Considerations:  
- [ ] The application will be running on a Windows 11 PC, interfacing with RPCS3 a Qt based PlayStation 3 emulator.  
- [ ] The application will be launched by Steam with the goal of prompting the user to connect a browser to the portal controller web application, which will be served by this application by listening on a web socket. The prompt will take the form of a scannable QR code that launches the users browser at the web application.  
- [ ] All communication occurs over a local network that the user’s browser and the Windows PC share. There is no need for any user provisioning, the server should be http based. Up to **two** phones may be connected simultaneously (matching the co-op player count); a third connection evicts the oldest session (FIFO) with a Chaos-themed "taken over" screen. See Round 4 (Q99+) for the full revision — earlier "last device wins" answers are superseded.  
- [ ] Once a device is connected, the remote controller prompts the user for which skylander game they wish to play. When selected, the application running on the windows pc, launches RPCS3 with the specified game. There is a static list of skylanders games to choose from, also by having the application launch it, the remote application should be able to ask to restart the game or quit it since RPCS3 sometimes freezes.  
- [ ] The remote interface switches at this time into a portal control mode. It shows what skylanders are on the emulated portal and allows the user to add and remove skylanders. All the actions the user takes are mirrored by the application by clicking buttons on the RPCS3 emulated portal dialog, preferably without showing this dialog to the user on the PC (and interrupting game flow).   
- [ ] RPCS3 has an ability to generate skylanders but its broken for certain games. As a result we have backed up the firmware of all the skylanders we own. However, skylanders progress and grow which means that the user may want to re-experience the journey many times. This is a long way of saying that when a skylander is picked for use, we will likely want to make a copy of the firmware file, use that and then offer the user the ability to reset a skylander by copying a fresh version over. This can all be handled by windows file operations, and the emulated portal is really just a series of slots that trigger a windows file dialog and a clear button.  
- [ ] On the remote application, we’ll need two main views. What is on the portal, and another view allowing the sort of skylanders by elemental type, game it came from and if its a trap/vehicle/sensai/etc. This is static data that needs to be associated with the firmware files. I don’t have this assembled but its all on the skylanders wiki pages so an effort to download and built the dataset will be needed. This includes images since frequently the names don’t make a lot of sense unless you memorize them. Additionally a search function by name will probably be needed since my oldest son does know the names. (The wiki https://skylanders.fandom.com/wiki/Main_Page is licensed under creative commons so the use of data and images is legal, this project will be licensed in the public domain.)  
- [ ] Secret surprise feature: Chaos is always the main villain of the skylanders games, he has numerous catchphrases, taunts the player and professes superiority. Once the main development is done an extra feature will be designed and added that on a random interval during a game session, no sooner than 20mins in and then repeating randomly every hour, choas will infect the remote application with “mind magic”. This will will cause an animation on the portal selection screen (very purple, dark pinks) and then chaos will swap the players slylanders to something random (which WILL swap them as if the player had done it). A demisable text overlay will taunt the player. This is a late feature but is noted so that any technical considerations can be incorporated into the designs. It will be implemented LAST.  
  
# Technical direction, known speed bumps:  
- [ ] I only have a single Windows computer and its going to be where this program runs. Its also hooked up to a tv, development is happening locally over remote desktop, so I am not interupting anyone.
- [ ] Ideally the application that wraps everything is written in Rust. Its the language I’m most comfortable with and its powerful enough to hook into almost anything. The ui for the application needs to be readable across the room and be able to show basic instructions and a QR code until it launches RPCS3. Dioxus might be a great way to get started without having to build it all ourselves.  
- [ ] The remote control application will be served as a web app by the main application. The frontend language doesn’t matter but it needs to run as a single page app and be very touch friendly. The static assets, rest endpoints and websockets(if any) will be served by the main application. If we go dioxus would it be able to serve a remote web client and the desktop app at the same time?   
- [ ] As another option my @chotchki/hotchkiss-io repo in github has full working example of how to do embedded web apps this with tokio (note the ui served has a different audience so htmx may not be the right choice here).  
- [ ] The remote application needs to stay in sync with the state of the portal in the RPCS3 application, this likely means websockets with some form of message passing. Hopefully dioxus has a solution but htmx can do it too.  
- [ ] The main application will be launched as a 3rd game from Steam, Steam is configured to run in big picture mode as the windows shell. This may result in unforeseen window management challenges in wrapping RPCS3, this may be a later research item since technically I could switch to a regular windows user.  
Research Considerations:  
- [ ] How to control the RPCS3 application is an unknown that should be validated first.  
- [ ] I have been rebuffed trying to contribute changes in the past and am unwilling to fork the code base so our control will be as if its a block box but at least it is still an open source application so we can see its code.  
- [ ] There is another MAYBE easier approach than hitting buttons on the RPCS3 application. The application has an ability to talk to a real skylander portal. Since we can read the source code for RPCS3, we maybe could figure out how to emulate how the real portal acts. That said, the RPCS3 may just instead pass the usb device on to the PS3 software which would be a far harder reverse engineering feature. Probably worth a research spike on direction. Being able to control other parts of the RPCS3 application has some value to manage start up and shutdown.  

# Visual Considerations:  
- [ ] All UI should fit the style of skylanders, light airy with whites and blues. Rounded corners in the Mac Liquid or Aqua styles will fit.  
- [ ] The UI should be theme-able so a surprise feature could be implemented.  

# Security Considerations:  
- [ ] As an application that is going to accept unauthenticated commands to push buttons and copy files, the application should be very careful how those commands work.   
- [ ] A weak form of authentication/security could be to encode a key into the qr code and then sign all commands sent to the server. This is probably a decent approach the more I think of it.  
- [ ] File names and paths should not be shown remotely.   
- [ ] Any input should not be directly used to take an action unless validated.  
- [ ] If custom names are allowed, check to ensure special characters that have meaning to the operating system are rejected or the path through the program does not expose them to be used in a operating system command.
  
# Testing Considerations:  
- [ ] This application with have many small parts that should be able to be tested separately before being combined.  
- [ ] Also the RPCS3 team has moved gui elements around during application updates. We need to validate our control works correctly.

# Distribution:
- [ ] I've already had people express interest in this. I think a zip file using github's releases will work for distribution.

## Questions

### Scope & User Flow
1. You mention RPCS3 throughout — do you mean **RPCS3** (the PS3 emulator)? Want to confirm before I go research control strategies.
  - yes that's correct, I misnamed it. Its code is here: https://github.com/rpcs3
  - please update the wording in the spec
2. How many concurrent remote users do we support? You said "last device to connect is authorized" — does that mean earlier sessions get kicked, shown a "taken over" screen, or silently stop receiving updates?
  - If a user gets kicked due to this they should be shown a taken over screen as if Chaos character did it and ask if they want to kick back and take over.
3. What happens if the connected phone disconnects (locks screen, walks out of wifi)? Does the PC-side app pause the game, show a reconnect QR, just wait, or does control fall back to local input?
  - so most phones go to sleep, which I don't know if there is a way to keep the connection alive in the face of that
  - putting a small reconnect overlay with a qr code in the lower right of the screen, would be ideal if the device is completely gone
4. Is the PC-side app expected to terminate when the user chooses "quit game" in the remote, or return to the QR code/launcher screen so a different game can be picked without relaunching from Steam?
  - I would have two buttons, return to launcher and exit to desktop
5. When a game is running and the remote asks to switch to a different skylanders game, should that restart RPCS3 automatically, or prompt for confirmation?
  - it should restart rpsc3 automatically since a major point of the app is to not have to mess with a keyboard/mouse

### Skylander / Firmware Management
6. Where do the "backed up firmware files" live today? A flat directory, per-game folders, or something else? Does every skylander have exactly one canonical .bin/.dump file, or multiples (different save states)?
  - There is a folder with them all here "C:\Users\chris\workspace\Skylanders Characters Pack for RPCS3"
  - These are considered fresh versions (I have another backup just in case these get messed up during development)
  - It is expected the user configures on first launch (with a standard file picker) where things are installed. This includes RPCS3.
7. For the "copy fresh version" reset flow — do you want per-skylander save slots (so the kids can resume progress across sessions), or is reset-to-fresh the only persistence model?
  - Ideally I would do a per user model that requires a simple 4 digit pin to access since our youngest son loves messing with his older brother's stuff.
8. When the same physical skylander is used across different games, is the firmware file shared, or are there separate per-game working copies?
  - I would share the file.
9. Skylanders figures span multiple games (SSA, Giants, Swap Force, Trap Team, SuperChargers, Imaginators). Does the filter by "game it came from" mean the game of origin, or games it's compatible with? These differ — e.g. a Giants figure works in later games too.
  - I would do two different filters, game of origin and a works with filter

### RPCS3 Control
10. You flagged the USB-portal-emulation path as a research spike. Do you want me to treat that as the default plan (given GUI automation is fragile) and fall back to button-clicking, or the reverse?
  - I just checked, usb-pass through is no longer supported so we will have to go the gui automation route.
11. For the "don't interrupt game flow" requirement — if we're stuck driving the GUI dialog, is it acceptable to briefly flash the portal window off-screen / minimized, or must the game truly never lose focus?
  - I would REALLY like to avoid it. now the game will be played with controllers so they might not lose focus. Another approach could be moving the gui window outside the display bounds (if windows will let us get away with it)

### Dioxus / Architecture
12. You asked whether Dioxus can serve both the desktop UI and a remote web client simultaneously. Do you want me to research that and come back with a recommendation, or have you already decided and want me to just try it?
 - I'm wanting to try to stay with a full rust stack to avoid pulling in a full typescript code base too. I'm not prescriptive on Dioxus if there is another more popular framework.
13. Is the "Rust main app + embedded web server" a hard requirement, or would you accept e.g. a Tauri shell if Dioxus+Axum coexistence turns out painful?
 - I'm open to ideas, just know the window management can be challenging.

### Wiki Data Ingestion
14. One-shot scrape committed as static JSON + images into the repo, or a periodic/on-demand refresher? (I'd lean one-shot given the data barely changes.)
  - One-shot makes sense, we just need to ensure we give credit to the source.
15. Are you okay committing the images into the repo, or should they be downloaded on first launch?
  - Commit to the repo. The dump files nor emulator should be committed, we do NOT encourage piracy.

### Chaos Feature
16. "Swap to something random" — from the full owned collection, or only skylanders compatible with the current game? And should it avoid swapping in something the player already has on the portal?
  - Since we'll have the data, a compatible skylander will make more sense.
17. "No sooner than 20min in, then every hour" — is that cumulative session time, or wall-clock since game launch? Does it pause when the game is paused/backgrounded?
  - I don't think we'll be able to tell the game is paused since its nested in the emulator. Wall-clock is a good approach

### Deployment & Dev Loop
18. Since dev happens over SSH into the HTPC that's also the play machine, how do we handle the case where the kids want to play while you're mid-development? Separate build dir + Steam entry for the "stable" build?
  - So I have taken over the desktop so Claude is now running locally and I control the desktop until we're done. I'll remove that from the spec and hopefully this makes it easier.
19. Any preference on logging/telemetry location, given this runs under Steam Big Picture as shell?
  - I think just a flat file to log to is good to start. We'll get fancier later.

## Follow-up Questions (Round 2)

### User Profiles & PINs
20. How many user profiles do you expect (you, wife, two sons = 4ish)? Does each kid get their own, or is it just "kid A / kid B" and parents share one?
 - I have three kids but only the two sons will play. I think maxing out as 4 profiles will be reasonable. Visually using a chest from skylander wiki would represent it well.
21. Is the PIN purely to gate access to a user's saves from a sibling, or also to gate parent-only actions (resetting other people's saves, changing config)? i.e. do we need a distinct "parent/admin" PIN?
 - gate against sibling messing with it. Its fine to save the pins to a simple config file since they aren't intended to be secure and I think that's the only persistent state outside the figure files. If there's more state, I would jump up to a sqlite database.
22. When a user picks a skylander that they've played with before, should the app auto-load their personal working copy, or always ask "resume your save vs. start fresh"?
 - auto load to minimize friction
23. If a skylander has never been used by this profile, do we silently fork a fresh copy on first use, or prompt?
 - fork a copy on first use
24. Where should profile data + working copies live on disk? Alongside the firmware pack folder, or under `%APPDATA%\skylander-portal-controller\`?
 - I'd use APPDATA so when distributed it plays nicely with other windows apps.

### First-launch Config
25. You mentioned first launch asks for install locations via standard file picker. Should that config UI be on the **PC window** (since it needs a real file picker and the user will be at the keyboard for first-time install anyway), or should we try to do it phone-side somehow?
 - on the windows PC, a one time keyboard use is totally reasonable.
26. What paths do we need to capture on first launch? My guess: (a) RPCS3 executable, (b) firmware pack root, (c) RPCS3 games library root. Anything else — BIOS, save directory, per-game ISOs?
 - This depends on what RPCS needs. I think its reasonable to assume to ask the user to load all games they want to use before launching, or remind them after first time setup if we can't find the games loaded. A user may not have ALL the skylander games.

### RPCS3 Control & Windowing
27. The "move RPCS3 window off-screen" idea — are you okay if this *only* applies to the portal-config dialog (child window), while the main emulator window stays on-screen and visible? Or do you want the whole emulator hidden too once the game is running fullscreen?
- we should minimize the main emulator window and not bring it up unless we have no choice
28. Does RPCS3 run the game in exclusive fullscreen, borderless fullscreen, or windowed? (Affects whether we can overlay a reconnect-QR on top without breaking the game.)
- I don't know, we'll have to test when we get there AND if its easy to keep a phone connected.
29. For the reconnect-QR overlay in the lower right of the screen: is it okay if that's a separate always-on-top transparent window owned by our app, or does it need to be painted into RPCS3?
- Separate window is fine. I suspect paining in will cause performance issues.
30. Is there any problem with our app capturing/synthesizing Qt clicks while a controller is the active input device? (Some anti-cheat-ish setups reject synthetic input — unlikely here, but worth asking.)
- I doubt there is any anti-cheat going on. I'd assume its fine until proven otherwise.

### Takeover / Session
31. "Kick back" takeover — should there be a cooldown or require the PC-side to show a physical button press/confirmation, to prevent two phones in the house ping-ponging forever?
- I think a minute cooldown is reasonable to cool any tempers.
32. When takeover happens, does the incoming phone inherit the current game/profile/portal state, or does it land back at the launcher?
- Inherits but the pin protected profile should lock
33. Should "exit to desktop" require a PIN (so a kid can't accidentally kill the whole app)?
- Nah, he's old enough to know better

### Rust Stack
34. For the Rust-only stack, I'd lean toward **Axum (server) + a separate SPA frontend in Dioxus-web or Leptos for the phone**, and **egui or Slint for the PC-side "launcher / QR" window** — these are boring and mature. Are you open to two separate UI frameworks (one for desktop, one for web), or do you specifically want one framework doing both?
- I am okay with separate UI frameworks, boring is good
35. Acceptable to use a bit of JS for the phone SPA if a Rust-WASM framework turns out to have awkward touch support, or is WASM-only a firm line?
- That's acceptable only if the rust version doesn't work. the touches should all be clicks so hopefully its fine

### Wiki Scrape
36. The fandom wiki has per-figure pages with images, stats, element, game-of-origin, figure type (core/giant/swap/trap/vehicle/sensei/imaginator), and "reposes." Do you want reposes treated as distinct entries (since their firmware is distinct) or grouped under one canonical figure?
- I would examine the dump file folder and check the varieties I have. I suspect it will be distinct.
37. Any attribute you specifically want that I might skip (e.g. soul gem name, signature moves, alignment for Trap Team traps)?
- It may be worth grabbing any attributes found so we could build an extra info pop up or something.

### Chaos Feature
38. Should there be a parent-side kill switch (settings toggle or env var) that disables Chaos entirely? Useful for "kid is having a bad day, no surprises tonight."
- Once we have a working remote app I'll think about a secret menu to disable that and/or reset pins. Not phase 1.
39. Does Chaos ever *remove* a skylander without replacing it, or always swap one-for-one?
- I would only swap one for one since there's a better chance of removal breaking things.

### Distribution
40. Since the zip is GitHub Releases, do you want CI set up now (GitHub Actions builds + attaches the zip on tag), or defer that until after the feature is working locally?
- Not until we have stuff working

## Follow-up Questions (Round 3)

### Firmware File Identification
41. Can we identify which skylander a `.dump` file represents from its **filename alone** (e.g. "Spyro.dump"), or do we need to parse bytes to extract the figure ID? I need this to decide how the dump-folder → wiki-metadata join works.
- The folder structure / file name should give all the info. I expect the info gathering to have a few passes. When we get to planning it will probably be, do a round to get the easy matches so we can keep moving and then later do another find them all.
42. Can you list (or should I scan) the contents of `C:\Users\chris\workspace\Skylanders Characters Pack for RPCS3` once, so the questions above are grounded in reality rather than my guessing?
- when I next ask for questions, its reasonable to scan the folder and see what you're faced with

### Portal State
43. How many slots does the RPCS3 emulated portal expose? (Real portals vary: main portal has 2 character + trap slot; SuperChargers uses vehicle slots; etc.) Does the slot count change per game, and do we need the UI to reflect that?
- It shows 8, probably to handle swap force plus vehicles 
44. After a session ends (quit or return to launcher), should skylanders left on the portal auto-unload, or do we remember the last portal layout per user as a convenience?
- I'd remember and reload (here's more state for the app)

### Profile & Takeover Details
45. Is the profile chosen **once per session** (shown at launcher before game select), or can profiles be switched mid-session without ending the game?
- Profiles can switch per session but the skylanders should unload only once the new profile is unlocked to handle interuption in the face of disconnect / takeover
46. On takeover (after 1-min cooldown), the incoming phone inherits the running game but the PIN-locked profile re-locks. While locked, can the new user (a) still see what's on the portal, (b) add/remove *non-profile-owned* figures, or is the UI gated until they enter a PIN?
- I would gate it with a screen saying someone else is in the academy and ask for a profile selection and pin.
47. If a guest with no profile wants to play (friend comes over), do we support a "guest / no-save" mode that uses fresh firmware every session and discards working copies on exit?
- That's a good idea, doesn't need to be a day one feature.

### State Storage
48. Given the need to track per-profile → per-figure working-copy mappings (plus PINs, plus possibly portal-layout history), I'd lean toward **SQLite from day 1** rather than starting with a flat config file and migrating later. Okay with that, or would you prefer to start flat and migrate when warranted?
- I think sqlite is reasonable to start with. I'd recommend using a rust wrapper or orm.

### Networking / QR
49. The QR code needs a URL. On Windows we often see multiple adapters (Wi-Fi, ethernet, VPN, Hyper-V virtual switches). How do you want us to pick? Options: (a) first non-loopback IPv4, (b) let user pick during first-launch config, (c) prefer a specific interface name pattern. My default: ask once during first-launch config, remember it, with a "redetect" button.
- I'd recommend the first non-loopback IP (could even be IPv6). Maybe a can't connect button. I'm assuming we will face some level of Windows firewall challenges.
50. Do you want mDNS/Bonjour (e.g. `skyportal.local`) as an alternative to IP, or is the QR code sufficient since no one is typing anything manually?
- QR code is fine thanks to the lack of typing.

### Security Phasing
51. The spec proposes HMAC-signing commands with a key embedded in the QR. Do you want this enforced from day 1, or is it okay to ship phase 1 with unsigned commands on the trusted LAN and layer signing in before public release?
- I'd start without signing but more from a step by step implementation approach. Once communication is figured out, that will be an immediate next step so its just treated as a communications common feature.

### Steam / Shell Behavior
52. Once distributed to others, will they also run Steam Big Picture as shell, or is that just your personal setup? (Affects whether we bake Big-Picture assumptions into the app or keep it vanilla-desktop-friendly.)
- No idea, Big Picture support should be later in the process since I see it as more a compatibility check item.
53. When the user picks "return to launcher," should RPCS3 be terminated gracefully (quit via GUI) or just killed? Gracefully is safer for save data; kill is faster if RPCS3 is frozen.
- graceful with a 30sec timer that the user can then request kill

### Wiki Scrape Mechanics
54. Any preference on scrape tooling — a one-shot Rust binary in this repo, or a Python/Node script in a `/tools` subfolder? The output is committed either way; I'm asking about the scraping code itself.
- whatever is easiest, I don't have python or node installed but powershell may be easy too
55. Fandom serves webp/png images in a few sizes. Target size for the phone UI — e.g. 256×256 thumbnails plus a larger hero image per figure, or just one size?
- thumbnail plus hero image gives flexiblity for multple screens

## Follow-up Questions (Round 4)

Context: I scanned `C:\Users\chris\workspace\Skylanders Characters Pack for RPCS3`. Structure confirmed as:
- Top: `Skylanders Spyros Adventure`, `Skylanders Giants`, `Skylanders Swapforce`, `Skylanders Trap Team`, `Skylanders Superchargers`, `Skylanders Imaginators`, plus cross-game `Items`, `Sidekicks`, `Adventure Packs`.
- Inside each game: element folders (`Air`, `Earth`, `Fire`, `Life`, `Magic`, `Tech`, `Undead`, `Water`), plus game-specific extras (`Giants` subfolder in Giants, `Dark`/`Light`/`Kaos`/`Creation Crystals` in Imaginators, `Sidekicks` inside Giants).
- Inside each element: `.sky` files (e.g. `Eruptor.sky`, `Flameslinger.sky`) plus an `Alternate types` subfolder for reposes.
- Junk to ignore: `desktop.ini`, `Poster.png`/`Poster.jpg`, `FireSymbolSkylanders.png` element icons, `.txt` readme files.
- Note: Extension is `.sky`, not `.dump` as I'd been guessing.

### Firmware Indexing
56. Folder path → metadata mapping looks like `{game}/{element}/[Alternate types/]{figure name}.sky`. Confirm the first-pass indexer should treat (game folder, element folder, filename) as the source of truth, with wiki metadata layered on top via name match. Fuzzy-match fallback for mismatched names, or is the wiki's search API good enough?
- Let's start with the search api and see if we get a 80% hit rate.
57. Element icon PNGs in each folder (`FireSymbolSkylanders.png` etc.) — want me to reuse these as the in-app element icons (saves a scrape step), or scrape fresh from the wiki for consistency?
- Let's reuse!
58. "Alternate types" appears everywhere. Is the distinction between a base figure and its repose meaningful to the kids (do they care that it's "Legendary Spyro" vs "Spyro"), or should we collapse them in the UI and only surface the variant when they drill in?
- I would collapse but the drill in or maybe a corner swap appearance button would make sense. and yes they do care about the varieties
59. `Sidekicks` exists both as a top-level folder AND inside `Skylanders Giants`. Which is authoritative, and is the other a duplicate / different thing (Giants-specific vs SSA-era mini-versions)?
- I think the top level is a duplicate and be ignored
60. `Items`, `Adventure Packs`, and `Creation Crystals` — do these show up on the emulated portal slots same as figures, or do they go into different RPCS3 UI slots we'd need to drive differently?
- they all go in the same slots
61. For Imaginators specifically, creation crystals persist player-authored characters. Does a "reset to fresh" on a crystal wipe the kid's created character? If so, we probably want per-profile crystal copies to be **never auto-reset** without explicit confirmation.
- yes, agreed

### Game Catalogue
62. The spec says "static list of skylanders games." Do we hardcode the six games (SSA, Giants, Swap Force, Trap Team, Superchargers, Imaginators) and have the first-launch config let the user check which ones they own? Or auto-detect by scanning RPCS3's game library directory for the known title IDs?
- I would auto detect on launch since the user may aquire more. If one isn't autodetected don't delete any per game settings.

63. Does each game have a unique title ID / disc image we can launch RPCS3 with via CLI args (e.g. `rpcs3.exe --no-gui "path/to/EBOOT.BIN"`), or do we always go through RPCS3's game list UI? This decides whether "launch game X" is a clean subprocess invocation or another GUI-driving exercise.
- I see a unique serial, hopefully that makes it easier. However I suspect --no-gui will not let us get to the portal config so we'll likely have to drive that window anyway.

### Rust Stack Commitment
64. You said boring is good and separate UI frameworks are fine. Do you want me to recommend a concrete stack in the plan (my default: **Axum** for HTTP/WS server + **Leptos** compiled to WASM for the phone SPA + **egui** via `eframe` for the PC-side launcher/QR window), or leave framework selection as an explicit early task in the plan?
- Recommend that stack in the plan and we'll change if it doesn't work out. All are good picks.
65. SQLite access — preference between `sqlx` (async, compile-time-checked queries, no ORM), `rusqlite` (sync, lightweight), or `sea-orm` (full ORM)? My lean is `sqlx` for the async fit with Axum, but you mentioned ORMs.
- I always like sqlx, stronger type checking makes everything easier.

### GUI Automation Tech
66. For driving the RPCS3 Qt dialog on Windows, the realistic options are (a) **UI Automation** (Windows' accessibility API — queries the widget tree by name/role, most robust), (b) **image/OCR-based** (brittle but framework-independent), (c) **raw SendInput coordinates** (fragile to DPI/resizing). I'd prototype with UI Automation first. Okay to commit to that as the default research path?
- Yes, I like that order, C is very much a last resort.
67. Would you accept pinning a known-good RPCS3 version (distributing or at least recommending a specific release) so GUI layout drift becomes a deliberate upgrade event, not a silent breakage?
- For this development we will stick to a known RPCS3 version and should check for it on start up. Longer term, if the GUI driving works okay we may need to just throw errors if it breaks.

### UI Scale / Theming
68. "Readable across the room" — any rough dimensions for the TV and viewing distance? Affects base font size and hit-target sizing for the PC-side launcher. (Phone-side I'll assume arm's length.)
- This is for first start correct? Its a large 86" TV but 10 feet away.
69. The Skylanders aesthetic is strong (hand-painted, cartoony). Are you okay with a modern iOS-style "light/airy + rounded corners" riff on that, or do you want asset-heavy skins (textured borders, painted buttons) from day 1?
- I'd recommend matching the skylander aesthetic with CSS as much as possible. The assets on the wiki probably won't be scaled for a high-res phone.

### Portal Slot UX
70. Portal shows 8 slots. Do you want the phone UI to always show 8 equal slots, or to dynamically show fewer when the game only uses fewer (e.g. 2 in SSA)? Showing 8 is simpler; showing game-correct count is nicer but needs per-game slot metadata.
- (overridden in 73) I would scale it to the game.
71. When a user taps a slot to put a figure in it, is the interaction (a) tap slot → modal with filterable collection → pick figure, or (b) pick figure from collection view → tap the slot to place it? Both work; (a) matches how the kid thinks ("this slot, this guy"), (b) matches drag-and-drop patterns.
- I think the portal is a separate screen, ideally like opening a drawer. you then pick the skylander and tap add. no drag and drop. once on the portal, show a remove button to remove them

## Follow-up Questions (Round 5)

### RPCS3 Version & Per-Game Metadata
72. You want to pin a known-good RPCS3 for dev. Do you have a specific release in mind (e.g. latest stable as of today, or a version you know works for all six games), and should the app hard-fail vs. warn if the running RPCS3 reports a different version?
- I would just use the version I already have setup / installed at C:\emuluators\rpcs3
- I would warn for now
73. Per-game slot count + slot semantics (e.g. SSA = 2 figure slots; Trap Team = 2 figures + 1 trap; Swap Force = 2 swappables; SuperChargers = vehicle slot; Imaginators = creation-crystal slot). Do you want me to hardcode a small per-game config file committed to the repo, or scrape it from the wiki? Hardcoded is simpler and this data never changes.
- I take back my prior decision, just allow 8 slots, if too many are placed on the portal, the game will show an error automatically and the user can remove.

### Figure Compatibility & Collection View
74. We need a `figure → list of compatible games` mapping so the "works with" filter (Q9) and Chaos's "compatible with current game" logic both function. Source options: (a) wiki per-figure pages, (b) hardcoded rules (figures work in their game of origin and all later games, with known exceptions like vehicles only in SuperChargers). (b) is much less scrape work. Which do you prefer?
- I think b is a good rule of thumb, we can enhance later.
75. When reposes are collapsed in the browse view, what's shown as the default card — the base figure, or the variant the kid most recently used? My lean: base figure, with a small badge indicating N variants available.
- base figure with badge works
76. The "corner swap-appearance button" on a figure card — does it cycle through variants, or open a picker? Picker gives parity with a lot of reposes (e.g. Legendary / Dark / original); cycling is fine if there are usually only 2–3.
- start with cycling, I think most have only a couple variants.

### First-Launch & Paths
77. First-launch needs to capture: RPCS3 executable, firmware-pack root (the `.sky` folder tree), and optionally a notes/games-library path. Anything else I should ask for on first launch?
- I think that's it to start, we'll figure out more as we go.
78. If a figure's `.sky` file is referenced by a profile but later the pack folder moves or the file is missing, what's the right behavior — grey out in UI with warning, or prompt to re-locate?
- grey out with a not found warning on just that figure

### Session Resume
79. You said remember-and-reload last portal layout per user. When a profile unlocks a game session, should we (a) immediately restore their last layout (fire the GUI-drive actions automatically), (b) show a "resume last setup?" prompt before doing it, or (c) show the last layout as pre-selected on the portal screen but not actually loaded until they tap confirm?
- I would do a resume last setup

### UI / Aesthetic
80. Confirming PC-side egui window uses the 86"@10ft sizing. Is that window expected to be **full-screen** (since Steam Big Picture replaces the shell and nothing else is visible), or a regular window? Fullscreen is simpler for the "readable across the room" goal.
- Fullscreen
81. For the Skylanders CSS aesthetic on the phone, do you have a reference screenshot or game UI you want to match (e.g. the in-game menu from Trap Team), or is "blues/whites/rounded/light" enough direction for me to take a pass and you'll react?
- Take a look at the screenshot found at https://www.reddit.com/media?url=https%3A%2F%2Fi.redd.it%2F7o10e6ofwfyf1.jpeg I think that has hints that will help

### Naming & Security
82. Do kids ever need to rename their working copies (e.g. name their Imaginator), or is the figure name always the figure's canonical name? If renaming is in scope, we need a whitelist-only text sanitizer for the input.
- keep the figure's canonical name
83. For the "no file paths shown remotely" rule — confirming the phone only ever sees a stable figure ID (e.g. UUID or hash) and the PC maps it to `.sky` paths internally. Sound right?
- that works, stops from reading random system files

### Error Handling & Logging
84. If a GUI-driving action fails mid-flow (e.g. RPCS3 dialog didn't appear in time, or a button moved), what's the desired phone-side behavior — (a) silent retry up to N times then error toast, (b) immediate error + retry button, (c) show a "something went wrong" screen and drop back to launcher?
- let's start a and see how it goes
85. Logs — flat file under `%APPDATA%\skylander-portal-controller\logs\`, rotated daily, 7-day retention. Sensible defaults, or do you want something different?
- that works, I'd note that should only happen for a release build. Logging to a build folder during development will be way easier and less noisy

### Testing Strategy
86. You flagged many-small-parts testability and RPCS3 GUI drift. My proposed test layers: (1) unit tests for pure logic (figure indexer, protocol, state machine), (2) integration tests for DB + filesystem, (3) a separate **manual** GUI-automation smoke test suite you run against a real RPCS3 install before releases (not CI, since CI can't run RPCS3 headlessly). Acceptable?
- 1+2 yes, 3 I want a e2e automated test suite with the remote tested using a headless browser. The url in the QR code may have to be scraped from the application's log output. This isn't CI since I want the suite to be run on this machine during development.
- By using a headless browser, RPCS3 can still fullscreen. If that proves problemmatic maybe we run testing using windowed screens and I'll do a final manual UAT test when appropiate.
87. Do you want CI on GitHub Actions from day 1 just for build + unit/integration tests (no RPCS3), or defer all CI until the app is functional (consistent with your "no CI yet" answer on distribution)?
- defer all CI. The e2e suite can't run in CI I know.

## Follow-up Questions (Round 6 — hopefully final)

These are the last genuine gaps I see. After this I'd suggest we move to the planning phase.

### Aesthetic Reference
88. You linked a Reddit screenshot (https://www.reddit.com/media?url=https%3A%2F%2Fi.redd.it%2F7o10e6ofwfyf1.jpeg). Would you like me to fetch and examine it during the planning phase to ground the CSS direction, or would you prefer to paste a saved copy into the repo (e.g. `/docs/aesthetic/`) so it lives with the project and survives the reddit link rotting?
- Placed in that folder as asked

### E2E Test Harness
89. For the headless-browser e2e suite driving the phone side, any preference between **Playwright** (Node, best-in-class but pulls in JS toolchain), **fantoccini** (pure Rust, WebDriver), or **thirtyfour** (pure Rust, also WebDriver)? A Rust-native option keeps the toolchain tight.
- Stay with pure rust, don't have a preference between the two.
90. For scraping the QR URL from logs during tests: is it acceptable for the test harness to read the log file directly, or would you prefer a dedicated `--print-url` CLI flag / stdout line the app emits during test mode?
- I think its fine for the test harness to read the log file for a pattern to find the url.

### Dev vs Release Runtime
91. Dev-mode behavior differences I'm planning to bake in: (a) logs to `./logs/` in workspace instead of APPDATA, (b) verbose log level, (c) optional skip of first-launch config (read paths from a `.env.dev`), (d) allow the e2e harness to inject profile + bypass PINs. Anything else you want dev-only, or any of these you'd reject?
- those sound good. I'm going to want you while testing to be able to idenpendently loop through and fix problems and that sounds reasonable.
92. How do we switch dev vs release — `cargo run` vs `cargo run --release`, a `DEV=1` env var, or a Cargo feature flag? I'd use a feature flag (`dev-tools`) so release builds physically cannot take the dev shortcuts.
- feature flag is fine

### Slot-Action Feedback
93. When the user taps "add figure X to portal," do we (a) optimistically show it on the slot immediately and roll back on failure, or (b) show a spinner on that slot until RPCS3 confirms the file-load completed? GUI-driving has 200ms–1s latency; (b) is more honest, (a) feels snappier. My lean: (b) with a snappy skeleton state.
- option b

### Session & Takeover Edge Cases
94. On first connect (before any profile is chosen), should the UI (a) immediately prompt for profile + PIN, or (b) show the game-picker first and only gate at portal-control time? Spec suggests (a); confirming.
- let's do "a" welcoming the portal master and asking them to secure their profile
95. Takeover after 1-min cooldown: does the **current** holder get any notification (e.g. "someone else is trying to connect, hold/release?"), or does it silently cut over once the cooldown elapses? My lean: silent cutover — kids will ping-pong otherwise, and the kicked user gets the "Chaos took it" screen per Q2.
- silent cutover

### Chaos Implementation Hook
96. Chaos's text overlay and any sound — text from the wiki's list of Kaos catchphrases is fine, yes? Any audio (if we ever add it later) — voice lines from the games would be copyrighted. Assume text-only forever unless you say otherwise?
- text is fine and avoids any copyright issues

### Networking Confirm
97. Confirming: **no first-launch network prompt** — we just pick the first non-loopback IP (preferring IPv4 over IPv6 for phone compatibility) on each launch, and if connection fails the user can click "can't connect" which offers a manual interface picker. Sound right?
- yes

### Planning Readiness
98. Any area you feel we haven't covered that should be nailed before planning starts? If not, I'd propose: **next turn, I scan the RPCS3 source and do the real research spike on portal-control (UI Automation vs. USB emulation vs. file drop), then write the implementation plan.**
- If you don't have any questions feel free to start planning. 
- We will need a PLAN.md and I think a CLAUDE.md to distill the requirements in a more compact format while this document remains a larger reference.
- I expect that this will take a LOT of steps/phases and we don't know it all up front. Focus on getting a plan with step by step checkboxes that focuses on research first. Post research we should review and then write a plan for the initial skeleton. Getting to a minimal end to end testable solution is more important first than building it all out.

## Follow-up Questions (Round 4) — Concurrent Users Revision

Context: original spec assumed 1 concurrent phone with "last device wins" takeover. Revising to **2** concurrent phones (matches co-op player count). This round supersedes the relevant portions of Q2, Q31, Q32, Q46, Q94, Q95.

### Concurrency Model

99. How many phones can be connected at once, and what happens on overflow?
- Up to **2** active sessions at any time. A 3rd connection evicts the **oldest** session (FIFO) — the evicted phone shows the existing Chaos "taken over" screen with a "kick back" action. Eviction is what triggers the cooldown below; two free slots admit new phones immediately with no cooldown.

100. Do the two phones share a profile or each unlock independently?
- **Each phone unlocks its own profile independently.** Two kids, two profiles, two PINs. One profile may be unlocked on both phones if a kid is using two devices, but unlocks don't propagate — each device enters its PIN.

101. When both phones are controlling the portal, how are conflicts resolved?
- **Free-for-all.** Either phone can add/remove any slot. The driver already serialises operations (single tokio worker + `spawn_blocking`), so throttling is a natural consequence — no extra arbitration logic. Last writer wins on a given slot; both phones see the same `SlotChanged` broadcast.

102. Does the cooldown from Q31 still apply?
- Yes, but only for **forced eviction** (the 3rd-connection case). The prior holder of the evicted slot can't immediately kick back for 1 minute — this is the anti-ping-pong guard. A phone joining into a free slot has no cooldown.

103. How does a new player join when both slots are full and neither player wants to be evicted?
- New requirement: **in-app QR display**. Any connected phone can open a "show join code" button that renders the same QR the TV shows. An existing player can walk the QR over to a new player so they consciously initiate the eviction themselves rather than it being a surprise.

104. On the portal-control view, how do the two players tell which figure belongs to whom?
- New requirement: **ownership indicator** on each occupied slot. Minimum: a small profile-coloured dot or initial on the slot; the owning phone sees its own figures highlighted. "Ownership" = the profile that placed the figure into that slot (not profile-of-origin for the working-copy file, which is separate).

105. What happens to on-portal figures when a phone disconnects?
- **Deferred.** The current session-resume model (Q44: remember and reload on unlock) assumes one player. With two, the questions multiply: does Player 2's slot stay populated if Player 1 drops? Does the kick-back flow restore the exact prior layout? Revisit after the 2-concurrent server logic lands and we can see the real failure modes. Flag this in PLAN.md 3.17 (reconnect overlay) as a follow-up.

### Impact Summary (for PLAN updates)

- Server session registry: `[Option<Session>; 2]` keyed by connection id; FIFO eviction on 3rd.
- Per-session state: `profile_unlock` is per-slot, not global. Portal state (8-slot array) remains global + shared.
- Figure ownership: add `placed_by_session` or `placed_by_profile` to `SlotState`; broadcast in `SlotChanged`.
- Phone UI: "show QR" action; ownership badge on portal slots; "taken over" screen already exists for evicted phones.
- E2E harness: support 2 concurrent `Phone` instances; add scenarios for concurrent edits, 3rd-connection eviction, cooldown enforcement.

