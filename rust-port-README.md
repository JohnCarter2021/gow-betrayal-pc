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

## Next steps (see ../REPORT.md)
1. Slice 3: gameplay state 1 — level loading (`a.g`, RP level packs),
   tile renderer (`b.java`), player/enemy update.
2. Window layer (keyboard input, integer scaling, frame timing) on top
   of the framebuffer — not yet built; the shim is backend-free by design.
3. MIDI/3GP media (`d.java`) — not yet ported.
4. RP chunk types 247/245/255, RP2 def-chunk-6 anomaly — still open.

The original jar is loaded from the user's own copy at runtime; no Glu
assets are redistributed in this repo.
