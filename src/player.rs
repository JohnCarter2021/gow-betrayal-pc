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
    /// Set when the current attack has already
    /// applied its hit this swing.
    pub attack_hit_done: bool,
    /// Combo step `gG` (slice 16): the post-increment
    /// value, i.e. the damage-table index of the
    /// current swing. 0 = fresh combo.
    pub combo_step: i32,
    /// Buffered next swing `gE` (slice 16): fire
    /// pressed during the cancel window of the
    /// current swing; consumed when the swing ends.
    pub attack_buffered: bool,
    /// Time in the current swing `gF` (ms): the
    /// cancel window opens after 150 ms (600 ms
    /// past the combo max, which the original's
    /// `gG > gN` check makes nearly unreachable).
    pub attack_time_ms: i32,
    /// Finisher played `fN` (slice 16): the next
    /// swing after a finisher restarts the combo.
    pub finisher_pending: bool,
    /// Damage of the current swing, from `fI`
    /// (slice 16): `fI[combo_step] << combo_shift`.
    pub swing_damage: i32,
    /// Combo cap `gN` (slice 16): ground combo is
    /// 54..54+max, then finisher 60. Fresh-save
    /// default 3; Game overwrites from the save.
    pub combo_max: i32,
    /// Blade damage shift `gP` (slice 16): damage
    /// is `fI[step] << combo_shift`. Fresh-save
    /// default 0; Game overwrites from the save.
    /// Also picks the air-combo shape: < 3 gives
    /// 67, 69 alternating; >= 3 gives 67, 68, 69.
    pub combo_shift: i32,
    /// Weapon mode `fM` (slice 17): 0 normal
    /// blades, 1 Blade of Artemis (`hb`), 2/3
    /// magic modes (`hc`/`hd`, casts not ported:
    /// fire does nothing in those modes).
    pub weapon_mode: i32,
    /// Blocking `hw` (slice 18): true while Down
    /// is held and the block stance holds.
    pub blocking: bool,
    /// Artemis combo cap `gO` (slice 17): ground
    /// combo is 72..72+max, then finisher 77.
    /// Fresh-save default 2; Game overwrites.
    pub combo_max_artemis: i32,
    /// Artemis damage shift `gQ` (slice 17):
    /// damage is `fJ[step] << combo_shift_artemis`.
    /// Fresh-save default 0; Game overwrites.
    pub combo_shift_artemis: i32,
    /// Gravity state for action 26 (fall), from e.java:
    /// `hK` = vertical speed (px/tick), `hL` = ms accumulator
    /// (hK += 4 every 200 ms, capped at 8). Both are 0
    /// whenever the action is not 26.
    pub h_k: i32,
    pub h_l: i32,
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
            combo_step: 0,
            attack_buffered: false,
            attack_time_ms: 0,
            finisher_pending: false,
            swing_damage: 0,
            combo_max: 3,
            combo_shift: 0,
            weapon_mode: 0,
            blocking: false,
            combo_max_artemis: 2,
            combo_shift_artemis: 0,
            h_k: 0,
            h_l: 0,
        }
    }

    /// Ground-attack actions: normal blades
    /// 54/55/56, finisher 60; Artemis 72/73,
    /// finisher 77 (slice 17).
    pub fn is_ground_attack(action: usize) -> bool {
        matches!(action, 54 | 55 | 56 | 60 | 72 | 73 | 77)
    }

    /// Air-attack actions: blades 67/68/69;
    /// Artemis 83/84/85 (slice 17).
    pub fn is_air_attack(action: usize) -> bool {
        matches!(action, 67 | 68 | 69 | 83 | 84 | 85)
    }

    /// True while the Blade of Artemis is equipped
    /// (`hb`, slice 17).
    pub fn is_artemis(&self) -> bool {
        self.weapon_mode == 1
    }

    /// Ground damage table `fI` (slice 16), indexed
    /// by the post-increment combo step.
    const F_I: [i32; 7] = [2, 2, 2, 4, 4, 4, 8];

    /// Artemis damage table `fJ` (slice 17), indexed
    /// the same way.
    const F_J: [i32; 6] = [4, 4, 10, 10, 18, 18];

    /// Start a ground swing from `s()` (slice 16):
    /// 54 + step while step < max, else finisher 60
    /// (a buffered swing after a finisher restarts
    /// at 54 via `finisher_pending`). Artemis
    /// (slice 17): 72 + step while step < artemis
    /// max, else finisher 77, damage from `fJ`.
    fn start_ground_swing(&mut self, anim: &AnimSet) {
        if self.finisher_pending {
            self.finisher_pending = false;
            self.combo_step = 0;
        }
        if self.is_artemis() {
            if self.combo_step < self.combo_max_artemis {
                self.action = 72 + self.combo_step as usize;
                self.combo_step += 1;
            } else {
                self.action = 77;
                self.finisher_pending = true;
            }
        } else if self.combo_step < self.combo_max {
            self.action = 54 + self.combo_step as usize;
            self.combo_step += 1;
        } else {
            self.action = 60;
            self.finisher_pending = true;
        }
        self.slot = AnimSlot::new(anim, self.action);
        self.swing_damage = if self.is_artemis() {
            Self::F_J[self.combo_step.clamp(0, 5) as usize] << self.combo_shift_artemis
        } else {
            Self::F_I[self.combo_step.clamp(0, 6) as usize] << self.combo_shift
        };
        self.attack_hit_done = false;
        self.attack_time_ms = 0;
        self.attack_buffered = false;
    }

    /// Start an air swing from `s()` (slice 16).
    /// Low blade level (shift < 3): 67, 69, 67, ...
    /// High blade level (shift >= 3): 67, 68, 69, 67, ...
    /// Artemis (slice 17) uses the same shape with
    /// 83/84/85 (the original's air branch keys the
    /// shape off `gP` for both weapons).
    fn start_air_swing(&mut self, anim: &AnimSet) {
        let (base, fin) = if self.is_artemis() {
            (83, 85)
        } else {
            (67, 69)
        };
        if self.combo_shift < 3 {
            if self.combo_step == 1 {
                self.action = fin;
                self.combo_step = 2;
            } else if self.combo_step >= 2 {
                self.action = base;
                self.combo_step = 0;
            } else {
                self.action = base;
                self.combo_step = 1;
            }
        } else if self.combo_step >= 3 {
            self.action = base;
            self.combo_step = 0;
        } else if self.combo_step == 2 {
            self.action = fin;
            self.combo_step = 3;
        } else {
            // High-level middle swing is base + 1
            // (68 blades, 84 Artemis).
            self.action = base + self.combo_step as usize;
            self.combo_step += 1;
        }
        self.slot = AnimSlot::new(anim, self.action);
        self.swing_damage = if self.is_artemis() {
            Self::F_J[self.combo_step.clamp(0, 5) as usize] << self.combo_shift_artemis
        } else {
            Self::F_I[self.combo_step.clamp(0, 6) as usize] << self.combo_shift
        };
        self.attack_hit_done = false;
        self.attack_time_ms = 0;
        self.attack_buffered = false;
    }

    /// Reset the combo (the original's `!gp && !gv`
    /// rule in `s()`: any non-attack action clears gG).
    fn reset_combo(&mut self) {
        self.combo_step = 0;
        self.finisher_pending = false;
        self.swing_damage = 0;
        self.attack_buffered = false;
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

    /// Start a jump (action 24, from Up key 1 when grounded,
    /// as in the gameplay case 1 -> s(24)). The arc itself is
    /// animation root motion (measured: dy 0 -> -48 -> +34
    /// over 16 frames, dx 0 -> 77); gravity (action 26)
    /// takes over only if the arc finishes without landing.
    /// Returns false if already airborne or otherwise busy.
    pub fn jump(&mut self, anim: &AnimSet) -> bool {
        if self.action != 0 && self.action != 9 {
            return false;
        }
        self.action = 24;
        self.slot = AnimSlot::new(anim, 24);
        self.h_k = 0;
        self.h_l = 0;
        true
    }

    /// Fire key (slice 16). During a swing, fire
    /// after the cancel window (150 ms, or 600 ms
    /// past the combo max) buffers the next swing
    /// and returns true; earlier fire is ignored.
    /// Grounded (idle/walk) starts a ground swing;
    /// airborne (jump arc / fall) starts an air
    /// swing. Returns false when nothing happens.
    pub fn attack(&mut self, anim: &AnimSet) -> bool {
        // Stance exclusivity (slice 18): no attack
        // while blocking (the original guards
        // several actions with `hw`; its
        // fire-while-blocking branch is untraced,
        // so the port blocks the swing outright).
        if self.blocking {
            return false;
        }
        if Self::is_ground_attack(self.action) || Self::is_air_attack(self.action) {
            // Cancel window cap is per weapon: gO
            // for Artemis, gN for blades (slice 17).
            let max = if self.is_artemis() {
                self.combo_max_artemis
            } else {
                self.combo_max
            };
            let threshold = if self.combo_step > max { 600 } else { 150 };
            if self.attack_time_ms > threshold {
                self.attack_buffered = true;
                return true;
            }
            return false;
        }
        if self.weapon_mode == 2 || self.weapon_mode == 3 {
            // Magic modes: casts are not ported yet,
            // so fire does nothing (rather than a
            // wrong blades swing).
            return false;
        }
        if self.action == 0 || self.action == 9 {
            self.start_ground_swing(anim);
            return true;
        }
        if self.action == 24 || self.action == 26 {
            self.start_air_swing(anim);
            return true;
        }
        false
    }

    /// Cycle weapons (slice 17): left softkey / '*'
    /// in the original, `fM` 0 -> 1 -> 2 -> 3 -> 0.
    /// Blocked while a ground swing runs (the
    /// original's `gv` guard; air swings do not
    /// block it). Returns false when blocked.
    pub fn cycle_weapon(&mut self) -> bool {
        if Self::is_ground_attack(self.action) {
            return false;
        }
        self.weapon_mode = (self.weapon_mode + 1) % 4;
        self.reset_combo();
        true
    }

    /// Unequip everything (`A()` in e.java): back
    /// to normal blades. Used when Artemis magic
    /// runs out.
    pub fn unequip_all(&mut self) {
        self.weapon_mode = 0;
    }

    /// Start the block stance (slice 18): Down
    /// pressed while grounded and idle/walking,
    /// as in the original's case 6 (`hw = true;
    /// s(3)`). Returns false otherwise (attacking,
    /// airborne, or already blocking).
    pub fn start_block(&mut self, anim: &AnimSet) -> bool {
        // Already posing: nothing to do. But
        // blocking + idle (a 97 reaction just
        // ended) must re-enter the pose - the
        // flag stays true through the reaction.
        if self.blocking && self.action == 3 {
            return false;
        }
        if self.action != 0 && self.action != 9 {
            return false;
        }
        self.action = 3;
        self.slot = AnimSlot::new(anim, 3);
        self.blocking = true;
        true
    }

    /// End the block stance (slice 18): Down
    /// released (`t != 6` -> `hw = false; s(0)`).
    pub fn stop_block(&mut self, anim: &AnimSet) {
        if self.blocking {
            self.action = 0;
            self.slot = AnimSlot::new(anim, 0);
            self.blocking = false;
        }
    }

    /// One 40 ms tick. `dir`: -1 left, 0 none, +1 right.
    /// Returns the root-motion delta applied (px, signed).
    pub fn tick(&mut self, anim: &AnimSet, flags: &TileFlags, level_w: i32, level_h: i32, dir: i32) -> i32 {
        let mut applied = 0;
        // Attack timer `gF`: accumulates while
        // attacking, else 0 (e.java, end of z()).
        if Self::is_ground_attack(self.action) || Self::is_air_attack(self.action) {
            self.attack_time_ms += 40;
        } else {
            self.attack_time_ms = 0;
        }
        if Self::is_ground_attack(self.action) {
            // Ground swing: root motion only, no walk.
            // When the slot finishes (repeat 0), a
            // buffered swing chains immediately, else
            // return to idle and reset the combo.
            let (dx, _dy) = self.slot.advance(anim, 40);
            self.ga += if self.facing_right { dx.abs() } else { -dx.abs() };
            applied = dx;
            if self.slot.repeat == 0 {
                if self.attack_buffered {
                    self.start_ground_swing(anim);
                } else {
                    self.action = 0;
                    self.slot = AnimSlot::new(anim, 0);
                    self.reset_combo();
                }
            }
            // Camera still follows below; skip walk logic.
            let target_x = self.ga - 120 + if self.facing_right { 60 } else { -60 };
            let target_y = self.gb - 266;
            self.update_camera(target_x, target_y, level_w, level_h);
            return applied;
        }
        if self.action == 3 {
            // Block stance (slice 18): a single
            // 100 ms pose, re-started when it ends
            // so the stance holds while Down is held
            // (the original's `hw && hA == 3 -> s(3)`
            // re-affirmation). No walk, no root motion.
            self.slot.advance(anim, 40);
            if self.slot.repeat == 0 {
                self.slot = AnimSlot::new(anim, 3);
            }
            let target_x = self.ga - 120 + if self.facing_right { 60 } else { -60 };
            let target_y = self.gb - 266;
            self.update_camera(target_x, target_y, level_w, level_h);
            return 0;
        }
        if self.action == 97 {
            // Blocked-hit reaction (slice 18): a
            // 345 ms stationary pose, then idle
            // (the block stance resumes next tick if
            // Down is still held - see Gameplay).
            self.slot.advance(anim, 40);
            if self.slot.repeat == 0 {
                self.action = 0;
                self.slot = AnimSlot::new(anim, 0);
            }
            let target_x = self.ga - 120 + if self.facing_right { 60 } else { -60 };
            let target_y = self.gb - 266;
            self.update_camera(target_x, target_y, level_w, level_h);
            return 0;
        }
        if Self::is_air_attack(self.action) {
            // Air swing: root motion (the swing's own
            // slow sink; no hK gravity during it).
            // Landing cancels it; a finished swing
            // chains when buffered, else falls.
            let (dx, dy) = self.slot.advance(anim, 40);
            let step = dx.abs();
            let gy = self.walk_tile_y();
            let ahead = if self.facing_right {
                self.tile_x() + 1
            } else {
                self.tile_x() - 1
            };
            let blocked = flags.solid(ahead, gy)
                || flags.solid(ahead, gy - 1)
                || flags.solid(ahead, gy - 2);
            if !blocked {
                self.ga += if self.facing_right { step } else { -step };
            }
            self.gb += dy;
            if dy > 0 && self.gb >= 0 && flags.solid(self.tile_x(), self.gb / 16) {
                self.gb = (self.gb / 16) * 16;
                self.action = 0;
                self.slot = AnimSlot::new(anim, 0);
                self.h_k = 0;
                self.h_l = 0;
                self.reset_combo();
            } else if self.slot.repeat == 0 {
                if self.attack_buffered {
                    self.start_air_swing(anim);
                } else {
                    self.action = 26;
                    self.reset_combo();
                }
            }
            let target_x = self.ga - 120 + if self.facing_right { 60 } else { -60 };
            let target_y = self.gb - 266;
            self.update_camera(target_x, target_y, level_w, level_h);
            return dx;
        }
        if self.action == 24 {
            // Jump arc: root motion from the animation. Horizontal
            // step uses the facing sign (as walk/attack do); the
            // walk blocked check stops horizontal motion at walls
            // but never stops the vertical arc. Landing is checked
            // only while descending: feet entering a solid tile
            // row snap to that row's top and return to idle, as
            // the original's gy ground flag does mid-arc.
            let (dx, dy) = self.slot.advance(anim, 40);
            let step = dx.abs();
            let gy = self.walk_tile_y();
            let ahead = if self.facing_right {
                self.tile_x() + 1
            } else {
                self.tile_x() - 1
            };
            let blocked = flags.solid(ahead, gy)
                || flags.solid(ahead, gy - 1)
                || flags.solid(ahead, gy - 2);
            if !blocked {
                self.ga += if self.facing_right { step } else { -step };
            }
            self.gb += dy;
            if dy > 0 && self.gb >= 0 && flags.solid(self.tile_x(), self.gb / 16) {
                self.gb = (self.gb / 16) * 16;
                self.action = 0;
                self.slot = AnimSlot::new(anim, 0);
            } else if self.slot.repeat == 0 {
                // Arc finished without landing (jumped into a
                // pit or off a ledge): fall with gravity.
                self.action = 26;
            }
            let target_x = self.ga - 120 + if self.facing_right { 60 } else { -60 };
            let target_y = self.gb - 266;
            self.update_camera(target_x, target_y, level_w, level_h);
            return dx;
        }
        if self.action == 26 {
            // Fall, e.java gravity block: gb += hK each tick;
            // hL accumulates ms, and every 200 ms hK += 4 up
            // to 8. Landing snaps to the solid row's top.
            self.gb += self.h_k;
            self.h_l += 40;
            if self.h_l > 200 {
                self.h_l -= 200;
                if self.h_k < 8 {
                    self.h_k += 4;
                }
            }
            if self.gb >= 0 && flags.solid(self.tile_x(), self.gb / 16) {
                self.gb = (self.gb / 16) * 16;
                self.action = 0;
                self.slot = AnimSlot::new(anim, 0);
                self.h_k = 0;
                self.h_l = 0;
                self.reset_combo();
            }
            let target_x = self.ga - 120 + if self.facing_right { 60 } else { -60 };
            let target_y = self.gb - 266;
            self.update_camera(target_x, target_y, level_w, level_h);
            return 0;
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

    fn level_9217() -> (
        crate::entities::LevelEntities,
        super::TileFlags,
        crate::anim::AnimSet,
    ) {
        let data = std::fs::read("../extracted/RP9").unwrap();
        let pack = crate::rp::parse_pack(&data).unwrap();
        let chunk = pack.chunk(1).unwrap();
        let parsed = crate::entities::parse_level_entities(9217, &chunk).unwrap();
        let flags = super::TileFlags::build(&parsed);
        let anim_data = std::fs::read("../extracted/RP28").unwrap();
        let anim_pack = crate::rp::parse_pack(&anim_data).unwrap();
        let anim = crate::anim::load_anim_set(&anim_pack.chunk(0).unwrap()).unwrap();
        (parsed, flags, anim)
    }

    #[test]
    fn jump_arc_rises_and_lands_on_flat_ground() {
        let (parsed, flags, anim) = level_9217();
        let spawn = parsed.group(26).unwrap().positions[0];
        let (cx, cy) = crate::entities::start_camera(&parsed.level, spawn);
        let mut pl = super::Player::new(&anim, spawn, true, cx, cy);
        assert!(pl.jump(&anim));
        let mut min_gb = pl.gb;
        let mut landed = false;
        for _ in 0..120 {
            pl.tick(
                &anim,
                &flags,
                parsed.level.pixel_width() as i32,
                parsed.level.pixel_height() as i32,
                0,
            );
            min_gb = min_gb.min(pl.gb);
            if pl.action == 0 && pl.gb == spawn.1 {
                landed = true;
                break;
            }
        }
        assert!(min_gb < spawn.1, "jump never rose: min gb {min_gb}");
        assert!(landed, "jump did not land: action {} gb {}", pl.action, pl.gb);
        assert_eq!(pl.gb, spawn.1);
        assert!(pl.ga > spawn.0, "jump moved forward: ga {}", pl.ga);
    }

    #[test]
    fn fall_gravity_accelerates_then_lands() {
        let (parsed, flags, anim) = level_9217();
        let spawn = parsed.group(26).unwrap().positions[0];
        let (cx, cy) = crate::entities::start_camera(&parsed.level, spawn);
        let mut pl = super::Player::new(&anim, spawn, true, cx, cy);
        // Drop from mid-air above the start area.
        pl.gb = 400;
        pl.action = 26;
        pl.slot = crate::anim::AnimSlot::new(&anim, 26);
        pl.h_k = 0;
        pl.h_l = 0;
        let w = parsed.level.pixel_width() as i32;
        let h = parsed.level.pixel_height() as i32;
        for _ in 0..6 {
            pl.tick(&anim, &flags, w, h, 0);
        }
        // hK is still 0 through the first 200 ms window.
        assert_eq!(pl.gb, 400, "gravity moved too early: gb {}", pl.gb);
        for _ in 0..10 {
            pl.tick(&anim, &flags, w, h, 0);
        }
        assert!(pl.gb > 400, "gravity never accelerated: gb {}", pl.gb);
        let mut landed = false;
        for _ in 0..200 {
            pl.tick(&anim, &flags, w, h, 0);
            if pl.action == 0 {
                landed = true;
                break;
            }
        }
        assert!(landed, "fall never landed: gb {}", pl.gb);
        assert_eq!(pl.gb % 16, 0, "landing not snapped to a row: gb {}", pl.gb);
    }
}
