# gow-betrayal-pc

A Rust PC port, in progress, of *God of War: Betrayal* (J2ME, Glu Mobile, v1.4.8).

Status, 2026-11-04 — slices 1–2 done, slice 3 (gameplay) not yet started:

- **MIDP shim, std-only, no crates:** 240×320 framebuffer, `Graphics`
  (clip / translate / `drawRegion` with all 8 MIDP transforms), sprite
  drawing per `c.java`, `RecordStore` (589,824-byte record split),
  std-only PNG decoder/encoder, RP pack parser, sprite-def
  (flag 254, field-major, 2,814 frames) and palette-key (flag 251, CRC-checked) parsers.
- **Game logic, slice 1:** boot steps 0/1/3, language select (state 78),
  audio prompt (state 79) with dialog wrapping, bitmap font (height 12),
  save defaults from `A(1)`.
- **Game logic, slice 2:** camera transition to the main menu
  (target 160,160, speed 35, V=375 countdown, then 0,0 + save + state 80),
  main-menu table (chunk 1028, 32 entries, LEFT/RIGHT carousel — not a
  list), level visibility from save progress, and menu actions
  (Play Game → level 9217, Arena → 9216, level select, Sound toggle,
  Clear Save, language cycle, Exit confirm).

Verified locally: `cargo test --release` 11/11, with scripted
`--game` runs rendering the language select, audio prompt, and main menu.

## No game assets are included

This repository contains **only the port code written for this project**.
It does **not** contain the original jar, its extracted contents, the
decompiled sources, or any sprite, sound, or video data from the game.
Those belong to Glu Mobile and are not redistributed here.

To run it you need your own legally obtained copy of the game:

```sh
unzip -o God-of-War-Betrayal_J2ME_EN_v148.jar -d extracted
cargo run --release -- extracted --game game-frame select,select
```

`--game` takes a frame-output prefix and a comma-separated key list
(`up,down,left,right,select,exit,tick`). Tests in `src/` also read
`../extracted` by default; they will fail without an extracted copy,
which is expected — the code is not a standalone game without the
original data it was written to run.

## Layout

- `src/rp.rs` — RP pack parser and string tables
- `src/png.rs`, `src/crc.rs` — PNG decoder/encoder, CRC-32
- `src/sprite.rs` — sprite definitions and palette keys
- `src/midp.rs` — framebuffer, graphics, record store
- `src/resources.rs`, `src/font.rs` — pack cache, bitmap font
- `src/menu.rs`, `src/game.rs` — menu table/navigation, game states
- `src/main.rs` — inspect / `--render-test` / `--decode-all` /
  `--defs-all` / `--game` modes

## Licence

The code in this repository is MIT (see `Cargo.toml`). That licence
covers the port code only; it does not grant any rights in the original
game, its name, or its assets.
