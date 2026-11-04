# gow-betrayal-pc — Rust port

Steps 1 and 2 done, std-only (no crates), compiled and tested here.

## Step 1 — RP packs
RP pack parser + string-table decoder, translated from `a.a` / `a.f` in
`../decompiled/a.java` and verified against all 33 RP files in the jar
(sum of chunk sizes == total == payload, every file).

## Step 2 — MIDP shim (2026-11-04)
- `src/midp.rs`: 240x320 `Framebuffer`, `Graphics` (clip, translate,
  fillRect, drawRGB, drawRegion with all 8 MIDP transforms and anchors),
  sprite drawing exactly as `c.a` in `decompiled/c.java` (anchor flips,
  source clipping, drawRegion), and a file-backed `RecordStore` mirroring
  `a.a(int)` / `a.a(int, byte[])` (store name = integer string, records
  split at 589,824 bytes). Game-loop timing from `a.run()`: min frame
  10 ms, delta clamped to 100 ms.
- `src/sprite.rs`: sprite definitions (flag 254, `a.b`) and palette keys
  (flag 251, `a.c`, CRC-checked PLTE swap).
- `src/png.rs`: std-only PNG decoder (8-bit palette + tRNS, DEFLATE,
  all 5 filters) and a small RGBA PNG encoder for test output.
- `src/crc.rs`: CRC-32 (standard check value passes).

## Build

Cargo is at `~/.cargo/bin/cargo` (not on the default PATH in this VM's
shells). On Windows, plain `cargo` works.

```powershell
cargo run --release -- ..\extracted
cargo run --release -- ..\extracted --render-test ..\test-frame.png
cargo run --release -- ..\extracted --decode-all
cargo run --release -- ..\extracted --defs-all
cargo test --release
```

## Verified 2026-11-04
- `cargo test --release`: 4 tests pass (CRC vector, clip, RecordStore
  round-trip + split, RP1 def 5 frames).
- `--decode-all`: all 34 PNG sheets decode, dimensions match
  `../sprites/frames.json` exactly, 0 mismatches.
- `--defs-all`: all 34 sprite defs parse, 2,814 frames total — matches
  `frames.json` exactly.
- `--render-test`: renders RP13's sheet, its largest frame (43x10) normal
  and X-flipped, and its palette-variant sheet onto a 240x320
  framebuffer, written as `../test-frame.png`. Transparency works via
  tRNS (background shows through, not magenta).

## Step 3, slice 1 — game logic: boot, language select, audio prompt (2026-11-04)
- `src/game.rs`: states 0 (Boot, loader steps 0/1/3), 78 (LanguageSelect),
  79 (AudioPrompt), with the original's input codes (Up=1, Down=6,
  Select=27/8, Exit=29), language tables `x`/`y` (English, Français,
  Español, Deutsch, Italian, Português), save defaults from `A(1)`,
  and dialog wrapping from `e.a(int[], byte[], ...)`.
- `src/resources.rs`: pack cache, string tables, sprite sets.
- `src/font.rs`: bitmap font from `a.a(c[])` (glyph transform when
  frame 3's height is 255, char-code binary search, width, centred draw);
  font height is 12.
- Also fixed: RP string-table count was off by one in `rp.rs`
  (count, not count+1 — it returned the right strings only because the
  extra read broke out of the loop).

Run it with scripted keys:
```powershell
cargo run --release -- ..\extracted --game ..\game-frame down,down,select,select
```
That boots, draws LanguageSelect to `game-frame-language.png`, applies
down, down, Select, Select, and draws the result to
`game-frame-after.png`: Spanish selected ("Elegir"), audio prompt
("Audio" / "Lord Kratos, ¿quieres activar el sonido?"), audio = true.
`cargo test --release`: 7/7 pass, including the full language-select flow.

## Step 3, slice 2 — camera transition + main menu (2026-11-04)
- `src/menu.rs`: table `J()` (chunk 1028, 32 entries), `H()` level
  visibility from save progress, `k()`/`l()` navigation exactly as in
  `e.java`, carousel range (iG/iH/n5).
- `src/game.rs`: camera (T/U, target, speed 35, countdown V=375),
  `tick(40)` frame step, state 80 MainMenu, state 1 Playing (entered,
  not yet gameplay). Menu actions: Play Game -> level 9217, Arena ->
  9216, level select -> bB[bC], Sound toggle, Clear Save, language
  cycle, Exit confirm. Help/About only set flags + camera target —
  their screens are not yet traced.
- `cargo test --release`: 11/11. Scripted runs:
  `select,select` -> MainMenu, "Betrayal" / "Play Game" with arrow;
  `select,select,right,right,right,select,right,right,select` ->
  Options -> Language -> French ("Trahison" / "Español" as next
  language, bottom bar "Sélect" / "Quitt." — both are the pack's
  literal French strings, verified, not rendering bugs).

## Step 4, slice 3a — level tile map (2026-11-04)
- `src/level.rs`: `e.a(byte[], false)` case 0 up to the tile map,
  with the `e.m(int)` bit reader (low bits of each byte first, as
  in `e.m`). Palette + map + collision bits, tileset 2058
  (128x256, 16x16 tiles, 8 per row); drawn tile = palette[map cell]
  (the `1 + nArray` in `b.a` minus the `-1` in `b.b`).
- Verified: For Sparta 9217 = 232x39 tiles (3712x624 px), palette
  107, q=122, hs=1250; 19456 = 204x46, 20480 = 112x59.
  `cargo run --release -- ../extracted --level 9217 1600 200 <prefix>`
  renders a mid-level viewport (buildings, roofs, windows) that
  matches the tile data; camera 0,0 is correctly mostly sky with
  ground at row 19. `cargo test --release`: 14/14.
- Slice 3b-1 DONE (2026-11-04): `src/entities.rs` + `--entities`
  mode parses all entity groups after the tile map (u=10 branch,
  `d()` extra reads per type, positions cumulative with `bz` bits).
  For Sparta: 27 groups, 1032 entities, bits 102815/102816 (1
  padding bit — the whole chunk is consumed, so no read pattern
  is wrong). Player spawn type 26 at (48, 560); type 23 static
  placements 653; enemies type 11 x8, type 13 x1. Extra values are
  stored flattened in read order, semantics not yet named.
- Slice 3b-2 DONE (2026-11-04): start camera + group names in
  `src/entities.rs` (`start_camera`, `kind_name`). Spawn type 26
  (48, 560), facing right, so iQ = 48-120-60 = -132 -> clamped 0,
  iR = 560-266 = 294; first frame camera (0, 294). Named from
  `d()` usage: 26 player spawn, 11/12/13 enemies, 23 static
  sprites, 3 camera zones, 6 action tables, tile-flag markers,
  sprite objects. Type 4 and a few others stay unnamed — parsed,
  not yet proven. `cargo test --release`: 16/16.
- Slice 3b-3 DONE (2026-11-04): `src/anim.rs` + `--scene` mode.
  `a.g(3073)` loader (k/l variable ints, B/D/F/G/H/J tables,
  exact stream end at 6270 bytes) + frame extraction, then
  `e.s()` layer order (0..7) for type-23 placements with sprite
  set 3076 (224 frames, max sprite index 223 — matches).
  Start-camera scene shows bushes, pots and doorway detail that
  tiles alone lacked. `cargo test --release`: 17/17.
- Slice 3c-1 DONE (2026-11-04): player sprite on `--scene`.
  Slot 135, anim set 28672 action 0 (idle, from `s(0)` in `t()`),
  sprite set 28673 (279 frames, action 0 = 20 frames), at spawn
  (48, 560), transform facing_right ? 0 : 2. `frames_for_action`
  gained a transform-XOR variant, because `b(Graphics)` uses raw
  dx/dy + final transform while `a(short[])` pre-flips dx/dy —
  both now reproduce `c.a` exactly. `cargo test --release`: 18/18.
- Slice 3c-2 DONE (2026-11-04): walk + camera follow, in
  `player.rs` + `--play` mode. `AnimSlot::advance` ports
  `a.b(int, int)` (root motion from the D table, visual frame
  from C>>6, repeat 63); walk action 9 = 6 px per 70 ms frame.
  `TileFlags` builds `iW` from the level header bits + type 22
  crosses + type 31 singles (flag 1 only; other flag-1 sources
  like sprite footprints are not yet included). Walk checks
  use (gb-1)/16, not gb/16 — the latter tests the solid ground
  row itself and blocks every step (found by debugging a
  walk that did not move). Camera ports `e.o()` (target
  ga-120+60, L=40, cap 40 px/tick, clamped). 40 ticks right:
  ga 48 -> 183, camera 0 -> 104. `cargo test`: 19/19.
- Slice 3d-1 DONE (2026-11-04): enemy spawn, in `enemies.rs`.
  Type 11/12/13 groups, extra layout, sprite/anim sets by
  level flags (9217: dA true -> 18432/18433; type-13 special
  pair -> 13312/13313, health 25). 9 enemies for 9217, drawn
  on `--scene`/`--play` after static sprites, before the
  player, as in `r(Graphics)`. `cargo test`: 20/20.
- Slice 3d-2 DONE (2026-11-04): enemy movement, in
  `enemies.rs` `EnemyState`. Chase (type-1 AI): within
  144/32 px of the player, face him and walk (action 1,
  root motion ~4-5 px/100 ms frame); within 48/16 px stop
  (attack is 3d-3). Patrol fallback for `ev` enemies at
  spawn +/- 64 px. All 9217 enemies have ev=0, so they
  idle until the player approaches — verified: after 80
  ticks right, enemy at 336 walked to 297 and stopped in
  attack range, enemy at 448 walked to 439. `cargo test`:
  21/21.
- Slice 3d-3 DONE (2026-11-04): combat, in `player.rs` /
  `enemies.rs` + `--play` `attack` key. Player swing action
  54 (320 ms, hit once at frame >= 2, range 32 px in front,
  damage 2 = fI[0]<<gP). Enemy attack action 5 (damage
  el<<1) / 6 (el<<2, every 4th), hit at frame >= 2.
  Player health 1024 (hj/hm). Verified: enemy at 336
  (health 3) took 2+2 and died, player took 6 damage
  (1018/1024). Dead enemies are not drawn (death
  animation not yet ported). `cargo test`: 22/22.
- Slice 3e DONE (2026-11-04): HUD, in `hud.rs` + `--play`.
  Top-left portrait (anim 1025 action 4, sprite 1036),
  weapon icon frame 5, health/magic bars (width value*6>>8,
  colours as in `f(Graphics)`), gold text, bottom bar
  (black 306-320, "Weapon"/"Menu" softkeys, "N Hits"
  centre when combo > 1, 2000 ms combo timer).
  Verified on the combat run: "2 Hits" shown, health
  1018/1024. `cargo test`: 23/23.
- Wiring DONE (2026-11-04): `--game` state 1 now runs the
  slice-3 engine via `gameplay.rs` (load/tick/draw shared
  with `--play`). Menu Play Game -> Playing loads level
  9217; keys left/right walk, attack swings, tick
  advances, exit returns to menu. Verified: `--game`
  select,select,select + walk keys renders Kratos in
  level with HUD. `cargo test`: 24/24.
- Slice 4a part 1 DONE (2026-11-04): window layer in
  `window.rs` — FrameTimer (40 ms frames, drift report,
  resync on overrun), integer nearest-neighbour scaling
  (3x = 720x960), `--window` real-time mode (scripted
  keys, one frame per tick, scaled PNG + timing).
  Verified: 278 frames in 11,181 ms (40.2 ms/frame,
  max drift 0 ms), sharp 720x960 frame.
- Slice 4a part 2 DONE (2026-11-04): real OS window in
  `play_window.rs` — winit window + live keyboard,
  softbuffer software presentation (no GPU needed),
  40 ms frame loop via ControlFlow::WaitUntil, same
  240x320 framebuffer integer-scaled (default 3x).
  Controls: arrows move / hold to walk, Enter or Space
  = Select, Space in gameplay = Attack, Escape = Back.
  Feature-gated (`--features window`) so the default
  build stays std-only and `cargo test` needs no
  display. Run: `gow-betrayal-pc <extracted-dir>
  --play-window [factor] [save-dir]`.
  Compile-verified on Linux and Windows (MinGW);
  the window itself can only be run on a machine
  with a display, so it awaits the user's PC test.
- Slice 4b DONE (2026-11-04): file-backed save in
  `save.rs` — the original's 76-byte `z(1)`/`A(1)`
  format (18 big-endian i32, store "2", hm = ho*192+1024,
  gN>=3/gO>=2 clamps), via the file-backed RecordStore.
  Save points: audio -> MainMenu, menu Exit, Clear Save
  deletes the store. `--game`/`--window` take a save-dir
  argument. Verified: 76-byte file [0,0,3,2,0...], a
  progress=3/hq=42 save survived a `--game` run intact.
  Also fixed a latent bug this exposed: save was loaded
  AFTER the menu was built, so progress never applied
  on boot. `cargo test`: 29/29.
- Slice 5 DONE (2026-11-04): all levels + Arena in
  `gameplay.rs` — LEVEL_IDS (bB), BG palette-key table,
  tileset key 2068+bG[i] / sprite key 3084+bG[i] when
  bG[i]>=0, `--levels` mode renders every level's
  start-camera frame. All 11 load: 10 levels (41x138 to
  232x39 tiles, 12-15 enemies each) + Arena 9216
  (86x38, 15 enemies). `cargo test`: 31/31.
- Slice 6 DONE (2026-11-04): Help/About in `--game`
  (state 107). Menu type 3 -> Help (string 114 +
  support email 292), type 4 -> About (string 115
  with % -> 1.4.8), via camera (160,160) then dialog
  screen; Up/Down scroll, Exit (Back) returns to menu.
  Also fixed: `--game` settle loop broke before the
  camera moved, and menu banners leaked over the
  dialog. `cargo test`: 33/33.
- Slice 7 DONE (2026-11-04): audio in `audio.rs` +
  `--audio` mode. The 11 RP1 flag-0 chunks are standard
  MIDI (format 1, division 960, track counts match);
  extracted as .mid files. iM slots: 0 = 1063 (music,
  6,274 bytes), 1 = 1064, 2 = 1062; `y(n)` only plays
  slots 0-2. Game wires audio prompt (yes -> enable +
  play slot 0, no -> disable) and menu Sound toggle.
  Actual MIDI sound needs a synth on the target
  (Windows winmm) — state is tracked, sound is NOT
  faked. `cargo test`: 35/35.
- Still open: v0.2 (push + tag + Windows zip with
  MSVC audit, when asked).

## Next steps (see ../REPORT.md)
1. Slice 3b: entity/object data after the tile map, spawn, then
   player/enemy update in state 1.
2. Window layer (keyboard input, integer scaling, frame timing) on top
   of the framebuffer — not yet built; the shim is backend-free by design.
3. MIDI/3GP media (`d.java`) — not yet ported.
4. RP chunk types 247/245/255, RP2 def-chunk-6 anomaly — still open.

The original jar is loaded from the user's own copy at runtime; no Glu
assets are redistributed in this repo.
