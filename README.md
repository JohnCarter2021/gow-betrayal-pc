# gow-betrayal-pc

A Rust PC port, in progress, of *God of War: Betrayal* (J2ME, Glu Mobile, v1.4.8).

Status, 2026-11-04 — v0.2.0: slices 1–2 (menus), slice 3 (gameplay),
wiring, window layer (timing/scaling), file-backed save, all levels +
Arena, Help/About, and MIDI audio state:

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
- **Audio, slice 7:** the 11 RP1 MIDI chunks are standard MIDI
  (format 1, division 960); iM slots 0 = 1063 (music), 1 = 1064,
  2 = 1062; audio prompt and Sound toggle drive playback state.

Verified locally: `cargo test --release` 35/35. A Windows exe was
cross-compiled on Linux (x86_64-pc-windows-gnu, MinGW target);
it has **not** been run on Windows yet — the user's PC is the
final test environment.

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
```

## Modes

```sh
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

- Real OS window + live keyboard (winit) is not yet implemented;
  `--window` is a real-time scripted runner, not a GUI.
- Actual MIDI sound output needs a synth on the target (Windows winmm).
- Death/stagger animations, hitbox rectangles (`ht[]`), Arena
  waves/scoring, level-complete flow, and 3GP video are not yet ported.
