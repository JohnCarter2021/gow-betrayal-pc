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
- Slice 8 DONE (2026-11-04): real MIDI sound in
  `midi.rs` — full parser (VLQ deltas, running status,
  tempo map, track merge by tick) + a Windows winmm
  player (MIDI mapper, background thread, 5 ms stop
  slices, slot 0 loops, slots 1/2 play once). Game
  wires it at the audio prompt (yes) and the Sound
  toggle; disabling stops playback. Non-Windows
  builds keep the exact same state tracking with an
  inert player. Music 1063: 2,422 events, 33,020 ms
  per loop. Sound itself awaits the user's PC (this
  VM has no MIDI synth). `cargo test`: 38/38.
- Slice 9 DONE (2026-11-04): player jump + gravity in
  `player.rs`. Up key (grounded only) starts action 24,
  whose arc is animation root motion (measured: dy 0
  -> -48 -> +34 over 16 frames, dx 0 -> 77); landing
  mid-arc snaps to the solid feet row and returns to
  idle; if the arc finishes without landing, action 26
  falls with the original gravity (gb += hK, hK += 4
  every 200 ms, capped at 8) until it lands. Wired to
  `--play` (up key), `--game` state 1 (Up), and the
  live window (ArrowUp, queued like Attack).
  Verified: --play jump ends ga 115, gb 560, idle;
  `cargo test`: 40/40.
- Slice 10 DONE (2026-11-04): enemy death animation
  in `enemies.rs`. Killing blow (ek <= 0 in e.java)
  starts the death action instead of removing the
  enemy: kind 11 -> action 8, kinds 12/13 -> action
  7 (measured on set 18432: 8 = 8 frames ~1.6 s,
  7 = 12 frames ~0.8 s); dying enemies are still
  drawn, take no hits and deal no damage, and are
  removed only when the animation ends. Not yet:
  the orb/gold drop (b(...)), the ei/eh i(11) case,
  death root-motion slide, stagger on non-lethal
  hits. Verified: --play kill shows the fallen pose
  mid-animation and the enemy gone after it ends;
  `cargo test`: 40/40.
- Slice 11 DONE (2026-11-04): enemy stagger on
  non-lethal hits in `enemies.rs`. A hit that does
  not kill starts the flinch action (kinds 11/12 ->
  action 2, ~600 ms; kind 13 -> action 4, ~200 ms,
  both measured on set 18432, repeat 1); during the
  flinch the enemy does not move or attack, then
  returns to idle. Not yet: locked-target stagger
  variants (7/9/8/13), the dX cooldown gate, facing
  change on stagger, stagger root-motion slide.
  Verified: --play single hit leaves enemy 4
  flinching (action 2) with health 1; `cargo test`:
  41/41.
- Slice 12 DONE (2026-11-04): orb/gold drops in
  `pickups.rs`, wired into gameplay and --play.
  Death spawns orbs from the enemy's em value
  (type 0 gold for kinds 11/13, type 2 magic for
  kind 12; health/magic values shift left 3; split
  18/9 over 60 else 6/3, big while over 24 remains,
  max 19 orbs, scatter from one random int each).
  Orbs home on the player (accel 360/tick, clamp
  3584, trail of 4), draw from sprite set 1034
  frame type*4+slot, and collect past 75 ms of age
  into gold/health/magic (damage orbs home on
  enemies, though none spawn yet — no magic
  weapons). Known faithful quirk: an orb parked
  exactly on the player with ~0 velocity never
  ages and never collects until a jump breaks
  overlap. Not yet: level-placed collectibles (the
  fm/ft system), the gold-full gate (hs untraced).
  Verified: kill drops 3,3,3,1 = 10 gold (6
  mid-flight, 10 after a jump breaks the glue);
  `cargo test`: 46/46.
- Slice 13 DONE (2026-11-04): script triggers +
  level-complete flow in `script.rs` + gameplay /
  game wiring. Type-8 entities are script triggers
  (tile flag 128, script start m10, length m3);
  standing on one starts its script: `^` strips
  its 2-char prefix and shows dialog text, `!`
  cutscene steps auto-advance (actions not
  ported), `@` boss-bar triggers latch the boss bar
  (Phase 2: slot 131, anim set 1027, drawn at screen
  (12, 295) while latched and facing the right way),
  a lone `*` advances the level
  (progress saved, next level loads, or menu
  after the last level). Level 1's end script
  is at tiles (214, 30-33): [^dialog, !, *].
  Tutorial scripts along the path fire correctly
  in --play. Not yet: level intro dialog (level
  header Q, e.g. string 122), state-109
  interstitial, `^` weapon switches.
  `cargo test`: 52/52. Note: walking to tile 214
  in --play is blocked at ga=2161 by a tall
  building wall — climb / box-push mechanics
  are not ported yet, so the Game-level advance
  (save + next-level load) is code-reviewed, not
  end-to-end run; the trigger itself is unit-test
  verified by teleport.
- Slice 14 DONE (2026-11-04): weakened-enemy
  markers (slot 132). Slot 132 = anim set 1027
  action 10, drawn from sprite set 1040 above
  enemies below half health (original's
  `r(Graphics)` condition; `ge`/`dX`/`eh` and the
  dQ/dR boss exceptions not ported, so the
  health rule alone decides). The slot advances
  once per tick (the original's `t(132)`), so the
  marker pulses through action 10's 7 frames on
  a ~1 s cycle. Enemy height `ef` = spawn-time
  bbox height of the initial action's first
  frame. Boss bar (slot 131) NOT in this slice
  — it needs the boss system ('@' triggers).
  `cargo test`: 54/54.
- Slice 15 DONE (2026-11-04): level intro
  dialog. Levels with header Q > 0 (only 9217,
  23552, 27648) open with dialog string Q before
  gameplay; Q <= 0 goes straight in. The dialog
  freezes the world; Select/Exit dismisses it.
  All three intros fit the box (longest 243
  chars, ~7 lines), so no scrolling. Language
  changes re-resolve the intro text.
  `cargo test`: 55/55.
- Slice 16 DONE (2026-11-04): combat combos
  + air attacks. Ground combo 54/55/56, then
  finisher 60; air combo 67/69 (67/68/69 at
  blade level 3+). Fire during a swing after
  the 150 ms cancel window buffers the next
  swing, consumed when the swing ends; no
  buffer means idle + combo reset. Damage is
  fI[step] << shift (2/2/4/4 at base level),
  wired from the save's gN/gP. Air swings use
  their own root motion and land or fall when
  they end. `cargo test`: 59/59.
- Slice 17 DONE (2026-11-05): weapons.
  Left softkey cycles fM 0/1/2/3 (blades,
  Artemis, magic, magic); Artemis ground
  72/73/77 and air 83/84/85 with fJ damage
  (4/10/10 ground at base level) and fJ*3
  magic drain per hit; empty magic unequips
  (A()); `^` script steps with digit > 3
  switch modes (the combo tutorial forces
  blades). Magic modes exist but their casts
  are not ported, so fire does nothing there.
  `cargo test`: 63/63.
- Slice 18 DONE (2026-11-05): blocking.
  Hold Down to block (pose action 3, held
  by re-starting its 100 ms frame); release
  to stop. While blocking, enemy hits deal
  no damage and play the block-impact
  reaction (action 97, 345 ms, facing the
  attacker), then the stance resumes while
  Down stays held. Attacks are ignored
  while blocking. Live window: hold
  ArrowDown. `cargo test`: 65/65.
- Still open: v0.2 (push + tag + Windows zip with
  MSVC audit, when asked).

## Phase 1 — Movement & world (slices 19-23, DONE 2026-11-05)

- **Slice 19 — double jump:** Up while airborne in action 24
  double-jumps (action 25). Combined peak 96 px. `cargo test`: the
  double-jump test peaks at gb<=470; the ga=690 step is cleared.
- **Slice 20 — full tile flags:** `TileFlags::build` covers the
  whole load-time catalog (header bits + types 22/31/5/9/29/0/18/
  10/25/14/30/28/8 + sprite footprints 27/7/35). Climb columns
  are flag 9 (solid + climbable). The crate is solid now.
- **Slice 21 — climb:** wall attach from a jump/fall into a
  flag-8 wall (hold 33), climb up 35 (-19 px/cycle), down 34
  (+20 px/cycle), turn 44, attack 36. Camera sits 80 px lower
  while climbing.
- **Slice 22 — box grab/push:** new module `boxes.rs`. Walk into
  a flag-512 tile to grab (21); push (22) / pull (23) on a fresh
  Left/Right press; `u()` moves the box 2 tiles (push) or 1
  (pull) at once, with a linear slide to its new tile. Jump out
  of a grab (21 -> 24).
- **Slice 23 — ledge mount, camera zones, f()/g() completion:**
  ledge grab (37, hang at gb = row*16 + 34) and mount (38,
  vault onto the ledge); walk-off-ledge falls; camera zones
  (type 3, step 7 px/tick); one-way platform drop-through
  (flag 8192, Down); switches (type-9, flag 256) and doors
  (type-35) in new module `doors.rs`; levers (type-34, Down
  near one toggles its doors — the (206,32) lever opens the
  pillar door before the level-end trigger). The level-1 wall
  crossing and the temple-floor run to the level end are both
  verified by Gameplay tests. `cargo test`: 100/100. Windows
  cross-build OK (2,468,657 bytes). Shipped in v0.6.0
  (2026-11-05, commit 26766b77).

- Phase 2 DONE (2026-11-05): boss research, then boss bars
  (slot 131). The bar is a looping 2-frame animation (anim set
  1027 actions 2/6/8), NOT a damage-driven health bar: `@` +
  two digits latches aR/aS/aT, the bar draws at screen (12, 295)
  while latched and facing the right way, and clears when the
  player leaves the trigger tiles. fR/fS/fQ are the grapple
  mini-game (not ported), not boss-bar fields. `cargo test`:
  103/103. Windows cross-build OK (2,474,209 bytes). v0.6.0
  (Phases 1+2) shipped 2026-11-05 ~14:18 IST (commit 26766b77,
  release tag v0.6.0, marked Latest).

- Phase 3 DONE (2026-11-05): magic. Mode 3 (`hd`) fires action
  95 and spawns type-3 damage orbs totalling `fL[gW]`
  (128 at gW 0) from above the player's feet, for a flat 256
  magic, then resets to blades; a failed gate (magic <= 256
  or 10+ active orbs) falls back to a blades swing. Mode 2
  (`hc`) fires action 94, a channeled cast draining 12 magic
  per tick while attack is pressed, ending on release or at 0
  magic; its hit effect (dX stagger, `fK[gV]`) is deferred
  to Phase 6 (dX not ported). Magic does not regenerate yet
  (original: `hl += hn/64`/tick) — it comes only from MAGIC
  orbs. `cargo test`: 109/109. Windows cross-build OK
  (2,482,068 bytes). Cargo version 0.7.0. v0.7.0 shipped
  2026-11-05 ~15:20 IST (commit 5208b861, release tag v0.7.0).

- Phase 4 DONE (2026-11-05): enemy AI depth + cutscenes +
  respawn. The `eu` idle-to-walk timer (enemies pause a
  random 0-511 ms before walking toward the player in
  chase range, then re-randomise). Real `!` cutscene
  playback (state 105): the type-6 aY/aZ tables play
  (aY[0] = action 96 in level 9217), the world freezes,
  and the script advances when the tables are exhausted
  (`ba`/`bb` persist across cutscenes in a level). Flag-4
  enemy respawn (type-14): crossing a flag-4 tile updates
  off-screen enemies' spawn points and respawns them
  there at full health. `cargo test`: 112/112. Windows
  cross-build OK (2,495,363 bytes).

- Phase 5 PARTIAL (2026-11-05): lever pull animations
  (Down near a lever plays action 15 pull / 17 un-pull,
  doors toggle at once); type-28 rectangle door triggers
  (standing on a flag-32 tile inside a rect toggles its
  door, up to `count` times, 2 s cooldown); level-placed
  collectibles (type-4: gold/health/magic orb bundles
  totalling `fs`, and upgrades — ft=3 raises max health
  or magic by 192 and refills). `cargo test`: 119/119.
  Windows cross-build OK (2,499,145 bytes). Cargo version
  0.9.0. v0.9.0 shipped 2026-11-05 ~16:07 IST (commit
  905f0519, release tag v0.9.0, release id 435064795).

- Phase 5 COMPLETE (2026-11-05): the rest — pause menu
  (state 102: Exit / Main menu / Help / Sound, with
  confirm sub-pages; Exit during gameplay now opens it
  instead of quitting straight to the menu); settings
  store "-9" (one byte, sound on/off, read at boot,
  written on toggle); Arena kills HUD ("Kills: N",
  kill count increments on every enemy death); state-109
  level-complete interstitial (Select continues with
  save, Exit goes to the menu). `cargo test`: 128/128.
  Windows cross-build OK (2,502,842 bytes). Shipped
  as v1.0.0 (2026-11-05 ~17:26 IST, push commit
  22d27c95, browser-verified, marked Latest). Gaps:
  Arena wave respawn untraced, best score / "You Are
  Dead" screen not ported (no death screen yet).
- Phase 6 COMPLETE (2026-11-05): combat polish —
  hurt reactions (5/7/100, damage > 50 turns to face
  the attacker and plays 7), block poses (27 enter,
  3 hold, 28 exit; Up while blocking exits to idle
  with no jump), death/stagger root-motion slide
  (raw dx with the facing sign; enemy 2: -26, 7: -15,
  8: -32, 9: -18; player 5/7: -32, 100: +46), heavy
  stagger variants (kind 11 -> 7, kind 12 -> 9,
  kind 13 -> 8), dX cooldown + slot-137 stagger
  indicator (mode-2 hits add (fK[gV] << 5) + 40;
  decay 40 + max_health/64 per tick; swings gated
  at dX > 500; weakened marker requires dX <= 0),
  HUD weapon icon per mode (frame 5 + fM), fire-
  while-grabbing release (attack from 21 -> idle).
  `cargo test`: 136/136. Windows cross-build OK
  (2,507,732 bytes). Not yet shipped — awaiting a
  go-ahead to ship (v1.1.0 per the plan). Deviation:
  walking out of block on a Left/Right press is NOT
  ported (port uses held dir, not press edges).
## Next steps (see ../REPORT.md)
1. Ship Phase 6 (v1.1.0 per the plan) — awaiting go-ahead.
2. Phase 7: death, Arena completion & the full playthrough
   (player death + "You Are Dead" screen, Arena best
   score, checkpoint respawn, Arena wave respawn,
   state-104 upgrade interstitial, magic regeneration,
   full playthrough verification).
3. Phase 8: grapple, presentation & loose ends.

The original jar is loaded from the user's own copy at runtime; no Glu
assets are redistributed in this repo.
