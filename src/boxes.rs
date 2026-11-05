//! Type-27 boxes: grab, push, pull (slice 22).
//!
//! Trace (from `decompiled/e.java` + JADX, 2026-11-05):
//!
//! - Load (case 27, CFR ~2895 / JADX ~3050): arrays `eN/eO/eP/eQ/
//!   eR/eS/eT/eU/eV/eW`, count = group size, `eX = count - 1`. Per
//!   item: `eN = m(3)` (sprite/action index), `eW = (m(1) == 1)`
//!   (climbable), slot `111 + i <- anim set 6144` (`a(111 + i,
//!   6144, eN)`), bbox `a(eM, 111 + i, 0, 0, 0, P)` where `eM =
//!   b(6146, bG[bC] >= 0 ? c(6150 + bG[bC]) : null)` — sprite def
//!   6146 (RP6 chunk 2). `eO = bs` (px x), `eP = bt` (px y), `eQ
//!   = bs/16`, `eR = bt/16`, `eS = P[2]`, `eT = P[3]`, `eU =
//!   eS/16`, `eV = eT/16`. Footprint flags are slice 20's
//!   `load_footprints` (flag 1 + 512, + 8 when `eW`, over
//!   `(eQ+i, eR-k)` for `i in 0..eU`, `k in 1..=eV`, plus flag 1
//!   on the base tile `(eQ, eR)`).
//! - Grab (`g()`, ~4085-4115): while sweeping, on hitting a
//!   solid tile while walking (`gt`, action 9) whose flag 512
//!   is set, the scan finds the box's left edge (walking the
//!   512 run at the footprint's bottom row) and matches it by
//!   `eQ == n + n5 && eR == n2 + 1`; then `hF = true`, `s(21)`,
//!   `gd = box`. The port's grab check (`check_grab`) runs
//!   after the player tick instead: when the player is idle /
//!   walking and the tile ahead (in the held direction) has
//!   flag 512, the box containing that tile is grabbed. The
//!   snap is the climb-attach snap (slice 21): facing right
//!   `ga = wall*16 - hx`, facing left `ga = (wall+1)*16 + hx`.
//!   (The original's `g()` snap, `ga = n*16 + (!hu ? hx : hy)`,
//!   agrees facing right but puts the player a tile inside the
//!   box facing left; the port keeps the verified flush snap.)
//! - Push/pull trigger (pressed-key cases 5/2): `if (gk && hA
//!   == 21)`: pressing the FACING direction = push (`s(22)`,
//!   `gl = 1`); pressing the OPPOSITE = pull (`s(23)`, `gl =
//!   0`). Then `if (!u()) { s(21); break; } y(3);` — `u()` runs
//!   immediately at the press (the box's tiles/flags/sprite
//!   change at once); failure returns to the grab pose 21.
//!   `y(3)` (the push sound) is NOT playable in the port —
//!   audio only plays slots 0-2; noted as a gap.
//! - `u()` (JADX e.java:3885-3931, transcribed verbatim in
//!   `try_push`): `b` = push direction (+1/-1: push -> facing
//!   sign, pull -> -facing sign); `b2` = 1 for a push (tries 2
//!   tiles), 0 for a pull (1 tile). `i` = leading column
//!   (`eQ + (b<0 ? eU-1 : 0)`), `i2` = player tile row `gx`,
//!   `i3` = `eV` (or `hg+1` when pulling a short box). The
//!   clearance loop fails when the 2-tile-ahead tile is solid,
//!   and falls back to 1 tile (`b2 = 0`) when the far edge of
//!   a 2-tile push is solid; a pull additionally checks the
//!   tiles 3-4 out and the tile beside the player. Then the
//!   old footprint's flags (1, 512, 8 when `eW`) are cleared
//!   over rows `[i2-eV+1, i2]`, `eQ` moves by `1+b2` tiles in
//!   direction `b`, and the new footprint is set the same way
//!   (the flag-8 column is offset by the original's
//!   off-by-(eU-1) quirk, transcribed faithfully; it never
//!   fires in 9217 since `eW` is false there). The box's slot
//!   action becomes `eN + (3 << b2)` (eN+3 for 1 tile, eN+6
//!   for 2).
//! - Box draw (`n(Graphics)`, ~2480-2493): `a(graphics, eM, n
//!   + 111, n2, n3, 0)` — the box is drawn at its px position
//!   `(eO, eP)` with its slot's current frame.
//! - Box slide update (~5742): per frame, when the box's slot
//!   is still animating (`t(111+n)` true) and the player is
//!   pushing (22, facing direction) or pulling (23, against
//!   it), `eO` moves by the frame's movement `a` and `eP` by
//!   `b`; when the slot finishes, it resets to action `eN`.
//!   The port slides the drawn position linearly toward the
//!   new `eQ*16` over the push/pull action's duration
//!   (fixed-point, snapped exactly at the end) — the original
//!   slides by the frame's movement vector, which the port's
//!   per-branch tick does not expose; the observable end
//!   state (box at its new tile, sprite reverted) is the
//!   same.
//! - Jump out of grab (pressed-key case 1): `if (hA == 21)
//!   { s(24); break; }` — see `Player::jump`.
//!
//! Actions (set 28672, player; the box's own slot actions are
//! in set 6144, `eN`/`eN+3`/`eN+6`): 20 = 10f repeat 1, no
//! motion, ~990 ms (grab family; `gk` covers 20-23); 21 = 3f
//! repeat 63, 190 ms each, no motion (grab hold); 22 = 10f
//! repeat 1, no motion, same durations as 20, ~990 ms (push);
//! 23 = 8f repeat 1, dx 0 -> -16, 150 ms each = 1,200 ms
//! (pull; the player steps back 16 px — the port applies the
//! raw dx with the facing sign, not `dx.abs()`).

use crate::anim::{self, AnimSet, AnimSlot};
use crate::entities::{self, LevelEntities};
use crate::midp::{Framebuffer, Graphics};
use crate::player::{TileFlags, PLAYER_HG, PLAYER_HX};
use crate::resources::{Resources, SpriteSet};

/// Anim set for the box slots (111 + i).
pub const BOX_ANIM_SET: u32 = 6144;
/// Sprite def for the box images.
pub const BOX_SPRITE_SET: u32 = 6146;
/// Palette-key base for the box sprite set (`6150 + bG[i]`,
/// as in the loader's `c(6150 + bG[bC])`).
pub const BOX_SPRITE_KEY_BASE: u32 = 6150;

/// One type-27 box (the original's `eN/eO/eP/eQ/eR/eU/eV/eW`
/// arrays, one entry per item).
pub struct GameBox {
    /// Group item index (the original's `gd` / `n`).
    pub index: usize,
    /// Sprite/action index `eN` (extra[0]).
    pub e_n: usize,
    /// Climbable `eW` (extra[1] != 0): the footprint also
    /// carries flag 8.
    pub e_w: bool,
    /// Drawn px position (`eO`, `eP`).
    pub x: i32,
    pub y: i32,
    /// Tile position (`eQ`, `eR`): left column, base row.
    pub e_q: i32,
    pub e_r: i32,
    /// Footprint size in tiles (`eU`, `eV`): width, height.
    pub e_u: i32,
    pub e_v: i32,
    /// The box's anim slot (111 + index), action `eN` at
    /// rest, `eN + (3 << b2)` while sliding after a push.
    pub slot: AnimSlot,
    /// Slide state (fixed point << 8): the drawn x moves
    /// toward `eQ*16` over the push/pull action.
    slide_x_fp: i32,
    slide_step_fp: i32,
    slide_target_x: i32,
}

impl GameBox {
    /// True when the tile is inside this box's flag-512
    /// footprint: columns `[eQ, eQ+eU)`, rows `[eR-eV, eR)`.
    /// (The base tile `(eQ, eR)` has flag 1 only, no 512,
    /// so it is not part of the grab footprint.)
    pub fn contains_tile(&self, x: i32, y: i32) -> bool {
        x >= self.e_q && x < self.e_q + self.e_u && y >= self.e_r - self.e_v && y < self.e_r
    }

    /// Advance the box slot one 40 ms tick and move the
    /// drawn position along the slide. When the pushed
    /// action finishes, the slot reverts to the base
    /// action `eN` (the original's `else { a(n6+111,
    /// 6144, eN[n6]); }`).
    pub fn advance(&mut self, anim: &AnimSet) {
        self.slot.advance(anim, 40);
        if self.slot.repeat == 0 && self.slot.action != self.e_n {
            self.slot = AnimSlot::new(anim, self.e_n);
        }
        if self.slide_step_fp != 0 {
            self.slide_x_fp += self.slide_step_fp;
            let x = self.slide_x_fp >> 8;
            let done = if self.slide_step_fp > 0 {
                x >= self.slide_target_x
            } else {
                x <= self.slide_target_x
            };
            if done {
                self.x = self.slide_target_x;
                self.slide_x_fp = self.x << 8;
                self.slide_step_fp = 0;
            } else {
                self.x = x;
            }
        }
    }

    /// Snap the drawn position to the tile position
    /// (used when a push/pull ends, to correct any
    /// fixed-point remainder).
    pub fn snap(&mut self) {
        self.x = self.e_q * 16;
        self.slide_x_fp = self.x << 8;
        self.slide_step_fp = 0;
    }

    /// Start the slide toward the new `eQ*16` over the
    /// push/pull action's duration (`ticks` 40 ms ticks).
    fn start_slide(&mut self, ticks: i32) {
        let target = self.e_q * 16;
        self.slide_target_x = target;
        self.slide_x_fp = self.x << 8;
        self.slide_step_fp = if ticks > 0 {
            ((target - self.x) << 8) / ticks
        } else {
            0
        };
        if self.slide_step_fp == 0 {
            self.x = target;
            self.slide_x_fp = self.x << 8;
        }
    }

    /// Draw the box at its px position with its slot's
    /// current frame (the original's `n(Graphics)`).
    pub fn draw(
        &self,
        g: &Graphics,
        fb: &mut Framebuffer,
        anim: &AnimSet,
        sprites: &SpriteSet,
        cam_x: i32,
        cam_y: i32,
    ) {
        let Ok(frames) = anim::frames_for_timeline_frame(anim, self.slot.action, self.slot.visual, 0)
        else {
            return;
        };
        for fr in frames {
            if let Some(frame) = sprites.frame(fr.sprite_idx) {
                crate::midp::draw_sprite_frame(
                    g,
                    fb,
                    &sprites.image,
                    frame,
                    self.x + fr.dx - cam_x,
                    self.y + fr.dy - cam_y,
                    fr.transform,
                );
            }
        }
    }
}

/// Load the type-27 boxes for a level (the loader's case
/// 27). `palette_key` colours the sprite set only; the
/// bbox (and hence the footprint) does not depend on it.
pub fn load_boxes(
    res: &mut Resources,
    parsed: &LevelEntities,
    palette_key: Option<u32>,
) -> Result<Vec<GameBox>, String> {
    let mut out = Vec::new();
    if let Some(g) = parsed.group(27) {
        let anim_chunk = res.chunk(BOX_ANIM_SET).map_err(|e| format!("{e:?}"))?;
        let anim = anim::load_anim_set(&anim_chunk).map_err(|e| e.to_string())?;
        let sprites = res
            .sprite_set(BOX_SPRITE_SET, palette_key)
            .map_err(|e| format!("{e:?}"))?;
        for (p, &(x, y)) in g.positions.iter().enumerate() {
            let e_n = g.extra[p * 2] as usize;
            let e_w = g.extra[p * 2 + 1] != 0;
            let (w, h) = entities::action_bbox(&anim, &sprites, e_n);
            let (e_u, e_v) = (w / 16, h / 16);
            let (e_q, e_r) = (x / 16, y / 16);
            out.push(GameBox {
                index: p,
                e_n,
                e_w,
                x,
                y,
                e_q,
                e_r,
                e_u,
                e_v,
                slot: AnimSlot::new(&anim, e_n),
                slide_x_fp: x << 8,
                slide_step_fp: 0,
                slide_target_x: x,
            });
        }
    }
    Ok(out)
}

/// Total duration of an anim action in 40 ms ticks
/// (sum of its timeline-frame durations `J`, rounded
/// down, at least 1).
fn action_duration_ticks(set: &AnimSet, action: usize) -> i32 {
    let Some(&b_entry) = set.b.get(action) else {
        return 1;
    };
    let off = (b_entry as u32 >> 16) as usize;
    let count = (b_entry & 0x3FF) as usize;
    let mut total = 0;
    for i in 0..count {
        if let Some(&c) = set.c.get(off + i) {
            if let Some(&d) = set.j.get((c & 0x3F) as usize) {
                total += d;
            }
        }
    }
    (total / 40).max(1)
}

/// The grab check (slice 22): when the player is idle /
/// walking and the tile ahead (in the held direction) has
/// flag 512, the box containing that tile is grabbed:
/// the player snaps flush against its face, starts the
/// hold pose (21) and records the box index (`gd`).
/// Returns true when a box was grabbed.
pub fn check_grab(
    boxes: &[GameBox],
    flags: &TileFlags,
    player: &mut crate::player::Player,
    anim: &AnimSet,
    dir: i32,
) -> bool {
    if dir == 0 || (player.action != 0 && player.action != 9) {
        return false;
    }
    let gy = player.walk_tile_y();
    let ahead = if dir > 0 {
        player.tile_x() + 1
    } else {
        player.tile_x() - 1
    };
    // The three rows the walk uses, topmost first
    // (the `g()` vertical scan finds the topmost
    // solid tile of the box's footprint).
    for row in [gy - 2, gy - 1, gy] {
        if !flags.has(ahead, row, 512) {
            continue;
        }
        if let Some(bx) = boxes.iter().position(|b| b.contains_tile(ahead, row)) {
            player.ga = if dir > 0 {
                ahead * 16 - PLAYER_HX
            } else {
                (ahead + 1) * 16 + PLAYER_HX
            };
            player.action = 21;
            player.slot = AnimSlot::new(anim, 21);
            player.grabbed_box = Some(bx);
            return true;
        }
    }
    false
}

/// `u()` (JADX e.java:3885-3931): move the grabbed box
/// `gd` by the push/pull. `action` is 22 (push, `gl = 1`)
/// or 23 (pull, `gl = 0`); `gw`/`gx` are the player's
/// tile column / walk row. Returns false when the box
/// cannot move (the caller returns to the grab pose 21).
/// On success the box's tiles and flags change at once,
/// its slot becomes `eN + (3 << b2)`, and its drawn
/// position starts sliding toward the new `eQ*16`.
#[allow(clippy::too_many_arguments)]
pub fn try_push(
    boxes: &mut [GameBox],
    gd: usize,
    flags: &mut TileFlags,
    facing_right: bool,
    action: usize,
    gw: i32,
    gx: i32,
    anim: &AnimSet,
) -> bool {
    let Some(bx) = boxes.get_mut(gd) else {
        return false;
    };
    let gl = if action == 22 { 1 } else { 0 };
    // `b`: push -> facing sign, pull -> -facing sign.
    let b = if (facing_right && action == 22) || (!facing_right && action == 23) {
        1
    } else {
        -1
    };
    let mut b2 = if gl == 1 { 1 } else { 0 };
    let e_u = bx.e_u;
    let e_v = bx.e_v;
    let e_q = bx.e_q;
    let i = e_q + if b < 0 { e_u - 1 } else { 0 };
    let i2 = gx;
    let mut i3 = e_v;
    if action == 23 && e_v <= PLAYER_HG {
        i3 = PLAYER_HG + 1;
    }
    for i4 in 0..i3 {
        if flags.get(i + b + b, i2 - i4) & 1 != 0 {
            return false;
        }
        if action == 23
            && (flags.get(i + 3 * b, i2 - i4) & 1 != 0
                || flags.get(i + 4 * b, i2 - i4) & 1 != 0)
        {
            return false;
        }
        if flags.get(i + b * (e_u + b2), i2 - i4) & 1 != 0 {
            b2 = 0;
        }
    }
    if action == 23 && flags.get(gw + b, i2) & 1 != 0 {
        return false;
    }
    // Clear the old footprint (rows [i2-eV+1, i2]).
    for i5 in 0..e_u {
        for i6 in 0..e_v {
            flags.clear_flag(e_q + i5, i2 - i6, 1);
            flags.clear_flag(e_q + i5, i2 - i6, 512);
            if bx.e_w {
                flags.clear_flag(e_q + i5, i2 - i6, 8);
            }
        }
    }
    // Move the tile position by 1+b2 tiles.
    bx.e_q += if b > 0 { 1 + b2 } else { -(1 + b2) };
    let e_q_new = bx.e_q;
    // Set the new footprint (the flag-8 column carries
    // the original's off-by-(eU-1) quirk, transcribed
    // faithfully; it never fires in 9217, eW is false).
    for i8 in 0..e_u {
        for i9 in 0..e_v {
            flags.set_flag(e_q_new + i8, i2 - i9, 1);
            flags.set_flag(e_q_new + i8, i2 - i9, 512);
            if bx.e_w {
                flags.set_flag(i + i8 + b2 + e_u * b, i2 - i9, 8);
            }
        }
    }
    // The pushed-look action, then slide the drawn
    // position to the new tile over its duration.
    let new_action = bx.e_n + (3 << b2);
    let ticks = action_duration_ticks(anim, new_action);
    bx.slot = AnimSlot::new(anim, new_action);
    bx.start_slide(ticks);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn level_9217() -> (LevelEntities, TileFlags, AnimSet, Vec<GameBox>, AnimSet) {
        let data = std::fs::read("../extracted/RP9").unwrap();
        let pack = crate::rp::parse_pack(&data).unwrap();
        let chunk = pack.chunk(1).unwrap();
        let parsed = crate::entities::parse_level_entities(9217, &chunk).unwrap();
        let mut res = crate::resources::Resources::new("../extracted");
        let footprints = crate::entities::load_footprints(&mut res, &parsed).unwrap();
        let flags = TileFlags::build(&parsed, &footprints);
        let anim_data = std::fs::read("../extracted/RP28").unwrap();
        let anim_pack = crate::rp::parse_pack(&anim_data).unwrap();
        let player_anim = crate::anim::load_anim_set(&anim_pack.chunk(0).unwrap()).unwrap();
        let boxes = load_boxes(&mut res, &parsed, None).unwrap();
        let box_anim_data = std::fs::read("../extracted/RP6").unwrap();
        let box_anim_pack = crate::rp::parse_pack(&box_anim_data).unwrap();
        let box_anim = crate::anim::load_anim_set(&box_anim_pack.chunk(0).unwrap()).unwrap();
        (parsed, flags, player_anim, boxes, box_anim)
    }

    #[test]
    fn boxes_load_for_level_9217() {
        let (parsed, _flags, _anim, boxes, _box_anim) = level_9217();
        assert_eq!(boxes.len(), 2, "9217 has two type-27 boxes");
        // Stream order (verified by probe): item 0 is
        // the tall pillar at (3008, 528), item 1 is the
        // crate at (2048, 560).
        let b0 = &boxes[0];
        assert_eq!((b0.x, b0.y), (3008, 528));
        assert_eq!((b0.e_q, b0.e_r), (188, 33));
        assert_eq!((b0.e_u, b0.e_v), (2, 6));
        assert_eq!(b0.e_n, 2);
        assert!(!b0.e_w);
        let b1 = &boxes[1];
        assert_eq!((b1.x, b1.y), (2048, 560));
        assert_eq!((b1.e_q, b1.e_r), (128, 35));
        assert_eq!((b1.e_u, b1.e_v), (2, 2));
        assert_eq!(b1.e_n, 0);
        assert!(!b1.e_w);
        let _ = parsed;
    }

    #[test]
    fn box_footprint_contains_its_tiles() {
        let (_parsed, _flags, _anim, boxes, _box_anim) = level_9217();
        // Item 0 is the tall pillar (188, 33), 2x6.
        let b0 = &boxes[0];
        assert!(b0.contains_tile(188, 27));
        assert!(b0.contains_tile(189, 32));
        assert!(!b0.contains_tile(188, 33), "base tile has no flag 512");
        assert!(!b0.contains_tile(190, 27));
        assert!(!b0.contains_tile(188, 26));
        // Item 1 is the crate (128, 35), 2x2.
        let b1 = &boxes[1];
        assert!(b1.contains_tile(128, 33));
        assert!(b1.contains_tile(129, 34));
        assert!(!b1.contains_tile(128, 35));
        assert!(!b1.contains_tile(130, 33));
        assert!(!b1.contains_tile(128, 32));
    }

    #[test]
    fn grab_push_moves_box_two_tiles() {
        let (parsed, mut flags, player_anim, mut boxes, box_anim) = level_9217();
        let spawn = (1900, 560);
        let (cx, cy) = crate::entities::start_camera(&parsed.level, spawn);
        let mut pl = crate::player::Player::new(&player_anim, spawn, true, cx, cy);
        // Walk right into the box: the walk stops at the
        // face, then the grab check attaches (21).
        let w = parsed.level.pixel_width() as i32;
        let h = parsed.level.pixel_height() as i32;
        for _ in 0..40 {
            pl.tick(&player_anim, &flags, w, h, 1);
            if check_grab(&boxes, &flags, &mut pl, &player_anim, 1) {
                break;
            }
        }
        assert_eq!(pl.action, 21, "grab: action {} ga {}", pl.action, pl.ga);
        assert_eq!(pl.grabbed_box, Some(1), "the crate is item 1");
        assert_eq!(pl.ga, 128 * 16 - PLAYER_HX, "grab snap: ga {}", pl.ga);
        // Fresh press right starts the push (22); u()
        // runs at once: 2 tiles (128 -> 130), flags move.
        let gd = pl.grabbed_box.unwrap();
        pl.action = 22;
        pl.slot = AnimSlot::new(&player_anim, 22);
        assert!(
            try_push(&mut boxes, gd, &mut flags, true, 22, pl.tile_x(), pl.walk_tile_y(), &box_anim),
            "push into clear space must succeed"
        );
        assert_eq!(boxes[1].e_q, 130, "2-tile push: eQ {}", boxes[1].e_q);
        assert_eq!(boxes[1].slot.action, 6, "pushed-look action eN+6");
        assert!(flags.has(130, 33, 512), "new footprint has 512");
        assert!(flags.has(131, 34, 512));
        assert!(!flags.has(128, 33, 512), "old footprint cleared");
        assert!(!flags.has(129, 34, 512));
        // The slide finishes at the new tile.
        for _ in 0..40 {
            boxes[1].advance(&box_anim);
        }
        assert_eq!(boxes[1].x, 130 * 16, "slide ends at eQ*16: {}", boxes[1].x);
        assert_eq!(boxes[1].slot.action, 0, "slot reverts to eN");
    }

    #[test]
    fn push_into_wall_falls_back_or_fails() {
        let (_parsed, mut flags, _anim, mut boxes, box_anim) = level_9217();
        // The crate is item 1 (eN 0). Put it against
        // the building wall (tile 136 is solid): a push
        // from eQ 134 has the 2-tile target (136) solid,
        // so u() fails.
        boxes[1].e_q = 134;
        assert!(
            !try_push(&mut boxes, 1, &mut flags, true, 22, 134, 34, &box_anim),
            "2-tile target solid: push must fail"
        );
        assert_eq!(boxes[1].e_q, 134, "failed push leaves eQ");
        // A push from eQ 133: the 2-tile target (135)
        // is clear but the far edge (136) is solid, so
        // it falls back to 1 tile (eN+3).
        boxes[1].e_q = 133;
        assert!(
            try_push(&mut boxes, 1, &mut flags, true, 22, 133, 34, &box_anim),
            "1-tile fallback push must succeed"
        );
        assert_eq!(boxes[1].e_q, 134);
        assert_eq!(boxes[1].slot.action, 3, "1-tile push action eN+3");
    }

    #[test]
    fn pull_moves_box_one_tile_toward_player() {
        let (_parsed, mut flags, _anim, mut boxes, box_anim) = level_9217();
        // Push the crate (item 1) 128 -> 130 first (so
        // the flags match its position), then pull it
        // back one tile: the player stands left of the
        // box (gw 129, after grabbing it at 130) facing
        // right and presses left (23).
        assert!(
            try_push(&mut boxes, 1, &mut flags, true, 22, 127, 34, &box_anim),
            "setup push must succeed"
        );
        assert_eq!(boxes[1].e_q, 130);
        assert!(
            try_push(&mut boxes, 1, &mut flags, true, 23, 129, 34, &box_anim),
            "pull must succeed"
        );
        assert_eq!(boxes[1].e_q, 129, "pull moves 1 tile: eQ {}", boxes[1].e_q);
        assert_eq!(boxes[1].slot.action, 3, "pull action eN+3");
        assert!(flags.has(129, 33, 512), "new footprint has 512");
        assert!(!flags.has(131, 34, 512), "old footprint cleared");
    }
}
