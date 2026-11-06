# gow-betrayal-pc

A Rust PC port, in progress, of *God of War: Betrayal* (J2ME, Glu Mobile, v1.4.8).

Status, 2026-11-06 — v1.4.1: everything in v1.4.0, plus two
bugfixes the user found while playing v1.4.0: silent music — the
MIDI parser never consumed the first data byte in the
running-status branch, so the slot-0 song parsed as 38 note-ons /
2363 note-offs; now 867/867 — and the screen shaking up and down
— the base camera measured its delta from the final camera
instead of the base camera, so the vertical zones at y=416
flickered on the edge of the base viewport; now the camera is
stable with the zone offset at its snap value:

- **MIDP shim, std-only, no crates:** 240×320 framebuffer, `Graphics`
  (clip / translate / `drawRegion` with all 8 MIDP transforms), sprite
  drawing per `c.java`, `RecordStore` (589,824-byte record split),
  std-only PNG decoder/encoder, RP pack parser, sprite-def
  (flag 254, field-major, 2,814 frames) and palette-key (flag 251, CRC-checked) parsers.
- **Game logic, slices 1–2:** boot steps 0/1/3, language select (state 78),
  audio prompt (state 79) with dialog wrapping, bitmap font (height 12),
  camera transition to the main menu, main-menu table (chunk 1028,
  32 entries, LEFT/RIGHT carousel), level visibility from save progress,
  and menu actions (Play Game, Arena, level select, Sound toggle,
  Clear Save, language cycle, Exit).
- **Gameplay, slice 3:** level tile maps, entity groups, spawn + start
  camera, static sprites, player walk + tile collision + camera follow,
  enemy spawn/movement (chase, patrol fallback), combat (player swing
  action 54, enemy attacks 5/6, damage, death), HUD (portrait, health/
  magic bars, Weapon/Menu bottom bar, "N Hits" combo).
- **Wiring:** `--game` state 1 runs the slice-3 engine; `--play`,
  `--scene`, `--level`, `--entities`, `--window` (real-time, 40 ms
  frames, integer scaling to 720×960), `--levels` (all levels),
  `--audio` (MIDI extraction) modes.
- **Save, slice 4b:** the original's 76-byte `z(1)`/`A(1)` format via
  RecordStore "2" (18 big-endian i32, hm = ho×192+1024), saved on
  audio → menu, menu Exit, and cleared by Clear Save.
- **All levels + Arena, slice 5:** all 10 levels (bB) + Arena 9216
  load and render, with per-level palette keys from the `bG` table
  (tileset 2068+bG[i], sprites 3084+bG[i]).
- **Help/About, slice 6:** state 107 dialog — Help (string 114 +
  support email 292), About (string 115, version 1.4.8) — with
  scrolling and Back to menu.
- **Audio, slices 7–8:** the 11 RP1 MIDI chunks are standard MIDI
  (format 1, division 960); iM slots 0 = 1063 (music), 1 = 1064,
  2 = 1062; audio prompt and Sound toggle drive playback state.
  Slice 8 adds real playback on Windows (winmm MIDI mapper):
  a full MIDI parser (VLQ deltas, running status, tempo map,
  track merge) and a background player thread; music loops,
  the other slots play once. Music 1063: 2,422 events, 33 s.
- **Playable window, slice 4a part 2:** a real OS window (winit)
  with live keyboard and software rendering (softbuffer, no GPU),
  40 ms frames, integer-scaled to 720×960 by default. Arrows move
  (hold to walk), Enter/Space = Select, Space in gameplay = Attack,
  Escape = Back. Feature-gated: build with `--features window`;
  the default build stays std-only.
- **Jump + gravity, slice 9:** Up starts the jump arc (action 24),
  with landing snaps and falling (action 26) under the original's
  gravity code.
- **Enemy death + stagger, slices 10–11:** killing blows play the
  death action (8/7); non-lethal hits stagger (2/4) and interrupt
  attacks.
- **Orbs, slice 12:** enemies drop gold/health/magic orbs (split
  18/9 over 60, else 6/3, max 19) that home in on the player with a
  4-frame trail.
- **Scripts + level flow, slice 13:** type-8 tile triggers fire
  story dialogs (`^`), auto-advance cutscene steps (`!`), and the
  level-end (`*`) which saves progress and loads the next level.
  Boss-latch triggers (`@`) are consumed silently for now.
- **Markers + intros, slices 14–15:** a pulsing marker above
  enemies below half health; levels with a header Q (9217, 23552,
  27648) open with an intro dialog.
- **Combos + air attacks, slice 16:** ground chain 54/55/56 +
  finisher 60, air attacks 67/68/69, 150 ms cancel-window buffering,
  damage from the save's combo level/shift.
- **Weapons, slice 17:** left softkey cycles blades / Blade of
  Artemis / two magic modes; Artemis has its own combos (72/73/77,
  83/84/85) with heavier damage and a magic drain per hit; empty
  magic unequips back to blades. Magic casts are not ported yet.
- **Blocking, slice 18:** hold Down for the guard pose (action 3);
  blocked hits deal no damage and play the block-impact reaction
  (action 97) facing the attacker; attacks are ignored while
  blocking.

Controls in the window: hold Left/Right to walk, Up to jump,
hold Down to block, Space to attack, Enter to cycle weapon
(Select) / advance dialogs, Escape to go back.

Verified locally: `cargo test --release` 65/65. The v0.2.0 Windows
exe was verified on a real Windows PC (all modes match Linux).
The v0.5.0 Windows exe is cross-compiled on Linux
(x86_64-pc-windows-gnu, MinGW target) with the window feature;
the window and the MIDI sound themselves have **not** been run
yet — this VM is headless and has no MIDI synth, so the user's
PC is the final test environment for both.

## No game assets are included

This repository contains **only the port code written for this project**.
It does **not** contain the original jar, its extracted contents, the
decompiled sources, or any sprite, sound, or video data from the game.
Those belong to Glu Mobile and are not redistributed here.
To run it, supply your own legally obtained jar and point the tool
at its extracted RP files (see `rust-port-README.md`).

## Build

```sh
cargo build --release
cargo test --release
# With the playable window (winit + softbuffer):
cargo build --release --features window
```

## Modes

```sh
# Playable window (needs the --features window build)
./gow-betrayal-pc <extracted-dir> --play-window
# Menus, scripted keys: up,down,left,right,select,exit,attack,tick
./gow-betrayal-pc <extracted-dir> --game out select,select,select
# Real-time window-layer mode (40 ms frames, 3x scaled PNG)
./gow-betrayal-pc <extracted-dir> --window out 3 select,select,select
# Gameplay, all levels, audio extraction
./gow-betrayal-pc <extracted-dir> --play 9217 right,tick
./gow-betrayal-pc <extracted-dir> --levels out
./gow-betrayal-pc <extracted-dir> --audio out
```

## Honest limits

- The playable window and the MIDI sound have not been run on
  a real display/synth yet (this VM is headless and has no
  MIDI synth); `--window` remains the scripted real-time
  runner for headless checks.
- Death/stagger animations, hitbox rectangles (`ht[]`), Arena
  waves/scoring, level-complete flow, and 3GP video are not yet ported.
