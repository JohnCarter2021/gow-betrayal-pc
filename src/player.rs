//! Player walk + camera follow (slice 3c-2).
//!
//! Tile flags `iW` built exactly as the original fills them for the
//! flag-1 (solid) sources used here:
//! - level header `b(bw, false)`: the palette-count bits go to
//!   `iW[0..bw]` (first tile positions, NOT palette entries).
//! - entity type 22: cross of `flag` at (x, y +/- up/down,
//!   x +/- left/right), flag = extra[0], counts = extra[1..5].
//! - entity type 31: single tile `flag` at (x, y), flag = extra[0].
//! Other flag-1 sources (sprite footprints, etc.) are NOT yet
//! included, so walk collision is approximate until those are ported.
//! Walk itself follows the original left/right input code for
//! actions 0/3: if tiles (gw+/-1, gx), (gw+/-1, gx-1),
//! (gw+/-1, gx-2) are all flag-1-free, set action 9 (walk);
//! root motion comes from `AnimSlot::advance` on anim 28672
//! (walk: 6 px per 70 ms frame, measured from the D table).
//! Camera follows `e.o()`: target = ga - 120 + (facing ? 60 : -60),
//! gb - 266, fixed-point smoothing with L = 40 ms, capped at
//! 40 px/tick, clamped to the level (camera zones type 3 not yet
//! applied, aI/aJ = 0).

use crate::anim::{AnimSet, AnimSlot};
use crate::entities::LevelEntities;

pub struct TileFlags {
    pub flags: Vec<i16>,
    pub width: usize,
    pub height: usize,
}

impl TileFlags {
    pub fn build(parsed: &LevelEntities) -> TileFlags {
        let level = &parsed.level;
        let w = level.width_tiles;
        let h = level.height_tiles;
        let mut flags = vec![0i16; w * h];
        let mut set = |flags: &mut Vec<i16>, x: i32, y: i32, flag: i32| {
            if x >= 0 && (x as usize) < w && y >= 0 && (y as usize) < h {
                flags[(x as usize) + w * (y as usize)] |= flag as i16;
            }
        };
        for (i, &bit) in level.collision.iter().enumerate() {
            if bit && i < flags.len() {
                flags[i] |= 1;
            }
        }
        for group in &parsed.groups {
            match group.kind {
                22 => {
                    for (p, &(x, y)) in group.positions.iter().enumerate() {
                        let e = &group.extra[p * 5..p * 5 + 5];
                        let (flag, up, down, left, right) = (e[0] as i32, e[1] as i32, e[2] as i32, e[3] as i32, e[4] as i32);
                        let tx = x / 16;
                        let ty = y / 16;
                        for k in 0..=up {
                            set(&mut flags, tx, ty - k, flag);
                        }
                        for k in 0..=down {
                            set(&mut flags, tx, ty + k, flag);
                        }
                        for k in 0..=left {
                            set(&mut flags, tx - k, ty, flag);
                        }
                        for k in 0..=right {
                            set(&mut flags, tx + k, ty, flag);
                        }
                    }
                }
                31 => {
                    for (p, &(x, y)) in group.positions.iter().enumerate() {
                        set(&mut flags, x / 16, y / 16, group.extra[p] as i32);
                    }
                }
                _ => {}
            }
        }
        TileFlags { flags, width: w, height: h }
    }

    pub fn get(&self, x: i32, y: i32) -> i32 {
        if x < 0 || (x as usize) >= self.width || y < 0 || (y as usize) >= self.height {
            return 65; // out of bounds, as in e.h()
        }
        self.flags[(x as usize) + self.width * (y as usize)] as i32
    }

    pub fn solid(&self, x: i32, y: i32) -> bool {
        self.get(x, y) & 1 != 0
    }
}

pub struct Player {
    pub ga: i32,
    pub gb: i32,
    pub facing_right: bool,
    pub action: usize,
    pub slot: AnimSlot,
    pub cam_x: i32,
    pub cam_y: i32,
    cam_fx: i32,
    cam_fy: i32,
    /// Player health `hj`, max `hm` = 1024, as in `t()`
    /// (hk = hj = hm at level start).
    pub health: i32,
    pub max_health: i32,
    /// Set when the current attack (action 54) has already
    /// applied its hit this swing.
    pub attack_hit_done: bool,
}

impl Player {
    pub fn new(anim: &AnimSet, spawn: (i32, i32), facing_right: bool, cam_x: i32, cam_y: i32) -> Player {
        Player {
            ga: spawn.0,
            gb: spawn.1,
            facing_right,
            action: 0,
            slot: AnimSlot::new(anim, 0),
            cam_x,
            cam_y,
            cam_fx: cam_x << 8,
            cam_fy: cam_y << 8,
            health: 1024,
            max_health: 1024,
            attack_hit_done: false,
        }
    }

    pub fn tile_x(&self) -> i32 {
        self.ga / 16
    }
    pub fn tile_y(&self) -> i32 {
        self.gb / 16
    }

    /// Tile row used by the left/right walk checks: the point just
    /// above the feet, (gb - 1) / 16, as in f() (gb is decremented
    /// before the tile walk). Using gb / 16 would test the solid
    /// ground row itself and block every step.
    pub fn walk_tile_y(&self) -> i32 {
        (self.gb - 1) / 16
    }

    /// Start a sword swing (action 54, from fire key 8 when
    /// idle/standing, as in the gameplay case 8). Returns false
    /// if already attacking or otherwise busy.
    pub fn attack(&mut self, anim: &AnimSet) -> bool {
        if self.action == 54 {
            return false;
        }
        if self.action != 0 && self.action != 9 {
            return false;
        }
        self.action = 54;
        self.slot = AnimSlot::new(anim, 54);
        self.attack_hit_done = false;
        true
    }

    /// One 40 ms tick. `dir`: -1 left, 0 none, +1 right.
    /// Returns the root-motion delta applied (px, signed).
    pub fn tick(&mut self, anim: &AnimSet, flags: &TileFlags, level_w: i32, level_h: i32, dir: i32) -> i32 {
        let mut applied = 0;
        if self.action == 54 {
            // Attack swing: root motion only, no walk. When the
            // slot finishes (repeat 0), return to idle.
            let (dx, _dy) = self.slot.advance(anim, 40);
            self.ga += if self.facing_right { dx.abs() } else { -dx.abs() };
            applied = dx;
            if self.slot.repeat == 0 {
                self.action = 0;
                self.slot = AnimSlot::new(anim, 0);
            }
            // Camera still follows below; skip walk logic.
            let target_x = self.ga - 120 + if self.facing_right { 60 } else { -60 };
            let target_y = self.gb - 266;
            self.update_camera(target_x, target_y, level_w, level_h);
            return applied;
        }
        if dir != 0 {
            self.facing_right = dir > 0;
            let gw = self.tile_x();
            let gx = self.walk_tile_y();
            let ahead = if dir > 0 { gw + 1 } else { gw - 1 };
            let blocked = flags.solid(ahead, gx) || flags.solid(ahead, gx - 1) || flags.solid(ahead, gx - 2);
            if blocked {
                if self.action != 0 {
                    self.action = 0;
                    self.slot = AnimSlot::new(anim, 0);
                }
                self.slot.advance(anim, 40);
            } else {
                if self.action != 9 {
                    self.action = 9;
                    self.slot = AnimSlot::new(anim, 9);
                }
                let (dx, _dy) = self.slot.advance(anim, 40);
                let step = dx.abs();
                self.ga += if dir > 0 { step } else { -step };
                applied = if dir > 0 { step } else { -step };
            }
        } else if self.action != 0 {
            self.action = 0;
            self.slot = AnimSlot::new(anim, 0);
            self.slot.advance(anim, 40);
        } else {
            self.slot.advance(anim, 40);
        }
        // Camera: e.o(), L = 40, aI = aJ = 0.
        let target_x = self.ga - 120 + if self.facing_right { 60 } else { -60 };
        let target_y = self.gb - 266;
        self.update_camera(target_x, target_y, level_w, level_h);
        applied
    }

    fn update_camera(&mut self, target_x: i32, target_y: i32, level_w: i32, level_h: i32) {
        let cap = 40 << 8;
        let mut nx = (target_x - self.cam_x) * 40;
        if nx > cap {
            nx = cap;
        } else if nx < -cap {
            nx = -cap;
        }
        let mut ny = (target_y - self.cam_y) * 40;
        if ny > cap {
            ny = cap;
        } else if ny < -cap {
            ny = -cap;
        }
        self.cam_fx += nx;
        self.cam_fy += ny;
        self.cam_x = self.cam_fx >> 8;
        self.cam_y = self.cam_fy >> 8;
        let max_x = level_w - 240;
        let max_y = level_h - 320;
        self.cam_x = self.cam_x.clamp(0, max_x.max(0));
        self.cam_y = self.cam_y.clamp(0, max_y.max(0));
        self.cam_fx = self.cam_x << 8;
        self.cam_fy = self.cam_y << 8;
    }
}


#[cfg(test)]
mod tests {
    #[test]
    fn walk_right_advances_and_camera_follows() {
        let data = std::fs::read("../extracted/RP9").unwrap();
        let pack = crate::rp::parse_pack(&data).unwrap();
        let chunk = pack.chunk(1).unwrap();
        let parsed = crate::entities::parse_level_entities(9217, &chunk).unwrap();
        let flags = super::TileFlags::build(&parsed);
        let anim_data = std::fs::read("../extracted/RP28").unwrap();
        let anim_pack = crate::rp::parse_pack(&anim_data).unwrap();
        let anim = crate::anim::load_anim_set(&anim_pack.chunk(0).unwrap()).unwrap();
        let spawn = parsed.group(26).unwrap().positions[0];
        let (cx, cy) = crate::entities::start_camera(&parsed.level, spawn);
        let mut pl = super::Player::new(&anim, spawn, true, cx, cy);
        for _ in 0..40 {
            pl.tick(
                &anim,
                &flags,
                parsed.level.pixel_width() as i32,
                parsed.level.pixel_height() as i32,
                1,
            );
        }
        assert!(pl.ga > spawn.0, "ga {} should exceed spawn {}", pl.ga, spawn.0);
        assert_eq!(pl.action, 9);
        assert!(pl.cam_x >= 0);
    }
}
