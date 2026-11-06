//! Player walk + camera follow (slice 3c-2).
//!
//! Tile flags `iW` (slice 20): the full load-time flag set
//! from `e.java`'s level loader, applied in stream order
//! (header collision bits first, then each group in order,
//! so a type-35 clear lands after an overlapping set).
//! Sources, with the flag each writes:
//! - level header: palette-count bits -> `iW[0..bw]`, flag 1.
//! - type 22: cross of flag `extra[0]` (0/1) at (x, y +/- up/down,
//!   x +/- left/right), counts `extra[1..5]`.
//! - type 31: single tile flag `extra[0]` (0/1).
//! - type 5: vertical column flag 8, `extra[0]` tiles down.
//! - type 9: vertical column flag 256, `extra[0]` tiles down.
//! - type 29/0/18/10: horizontal run, `extra[0]` tiles right,
//!   of flag 16384 / 8192 / 16 / 64 respectively.
//! - type 25: single tile flag 1024.
//! - type 14: vertical run flag 4, `extra[0]` tiles down
//!   (the 15 (m(10), m(10), m(1)) pairs per item are the
//!   enemy-teleport table, not flags).
//! - type 30: single tile flag 2048 if `extra[0] != 0`,
//!   else flag 4096.
//! - type 28: rectangle flag 32, `extra[0]` wide x `extra[1]` high.
//! - type 8: single tile flag 128 (script trigger; the
//!   script engine keeps its own trigger list too).
//! - sprite footprints (from `entities::load_footprints`):
//!   type 27 boxes (1 + 512, +8 if climbable), type 7 (1),
//!   type 35 (1 set, or cleared when `cV`).
//! Runtime flag changes (enemy tiles, box pushes, trigger
//! clears) are NOT load-time and are not in this build;
//! they belong to slices 22-23. Walk itself follows the
//! original left/right input code for actions 0/3: if
//! tiles (gw+/-1, gx), (gw+/-1, gx-1), (gw+/-1, gx-2)
//! are all flag-1-free, set action 9 (walk); root motion
//! comes from `AnimSlot::advance` on anim 28672 (walk:
//! 6 px per 70 ms frame, measured from the D table).
//! Camera follows `e.o()`: target = ga - 120 + (facing ? 60 : -60),
//! gb - 266, fixed-point smoothing with L = 40 ms, capped at
//! 40 px/tick, clamped to the level (camera zones type 3 not yet
//! applied, aI/aJ = 0).

use crate::anim::{AnimSet, AnimSlot};
use crate::entities::{Footprint, LevelEntities};

pub struct TileFlags {
    pub flags: Vec<i16>,
    pub width: usize,
    pub height: usize,
}

impl TileFlags {
    pub fn build(parsed: &LevelEntities, footprints: &[Footprint]) -> TileFlags {
        let level = &parsed.level;
        let w = level.width_tiles;
        let h = level.height_tiles;
        let mut flags = vec![0i16; w * h];
        let mut set = |flags: &mut Vec<i16>, x: i32, y: i32, flag: i32| {
            if x >= 0 && (x as usize) < w && y >= 0 && (y as usize) < h {
                flags[(x as usize) + w * (y as usize)] |= flag as i16;
            }
        };
        let mut clear = |flags: &mut Vec<i16>, x: i32, y: i32, flag: i32| {
            if x >= 0 && (x as usize) < w && y >= 0 && (y as usize) < h {
                flags[(x as usize) + w * (y as usize)] &= !(flag as i16);
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
                // Vertical columns: type 5 climbs (flag 8),
                // type 9 switches (flag 256).
                5 => {
                    for (p, &(x, y)) in group.positions.iter().enumerate() {
                        let len = group.extra[p] as i32;
                        for k in 0..=len {
                            set(&mut flags, x / 16, y / 16 + k, 8);
                        }
                    }
                }
                9 => {
                    for (p, &(x, y)) in group.positions.iter().enumerate() {
                        let len = group.extra[p] as i32;
                        for k in 0..=len {
                            set(&mut flags, x / 16, y / 16 + k, 256);
                        }
                    }
                }
                // Horizontal runs: 29 -> 16384, 0 -> 8192
                // (the `gz` ground flag), 18 -> 16, 10 -> 64.
                29 | 0 | 18 | 10 => {
                    let flag = match group.kind {
                        29 => 16384,
                        0 => 8192,
                        18 => 16,
                        _ => 64,
                    };
                    for (p, &(x, y)) in group.positions.iter().enumerate() {
                        let len = group.extra[p] as i32;
                        for k in 0..=len {
                            set(&mut flags, x / 16 + k, y / 16, flag);
                        }
                    }
                }
                25 => {
                    for &(x, y) in &group.positions {
                        set(&mut flags, x / 16, y / 16, 1024);
                    }
                }
                // Type 14: vertical run flag 4, length =
                // extra[0]; the 15 pairs after it are the
                // enemy-teleport table (g()'s flag-4 handler).
                14 => {
                    for (p, &(x, y)) in group.positions.iter().enumerate() {
                        let len = group.extra[p * 46] as i32;
                        for k in 0..len {
                            set(&mut flags, x / 16, y / 16 + k, 4);
                        }
                    }
                }
                30 => {
                    for (p, &(x, y)) in group.positions.iter().enumerate() {
                        let flag = if group.extra[p] != 0 { 2048 } else { 4096 };
                        set(&mut flags, x / 16, y / 16, flag);
                    }
                }
                28 => {
                    for (p, &(x, y)) in group.positions.iter().enumerate() {
                        let (dc, dd) = (group.extra[p * 4] as i32, group.extra[p * 4 + 1] as i32);
                        for a in 0..=dc {
                            for b in 0..=dd {
                                set(&mut flags, x / 16 + a, y / 16 + b, 32);
                            }
                        }
                    }
                }
                8 => {
                    for &(x, y) in &group.positions {
                        set(&mut flags, x / 16, y / 16, 128);
                    }
                }
                // Sprite footprints (kinds 27/7/35), applied
                // here in stream order via the precomputed list.
                27 | 7 | 35 => {
                    for (p, _) in group.positions.iter().enumerate() {
                        for fp in footprints.iter().filter(|f| f.kind == group.kind && f.item == p) {
                            for dy in 0..fp.h {
                                for dx in 0..fp.w {
                                    if fp.set != 0 {
                                        set(&mut flags, fp.x + dx, fp.y + dy, fp.set as i32);
                                    }
                                    if fp.clear != 0 {
                                        clear(&mut flags, fp.x + dx, fp.y + dy, fp.clear as i32);
                                    }
                                }
                            }
                        }
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

    /// Landing-solid (slice 23): flag 1 (solid) OR
    /// flag 8192 (one-way platform, type-0 runs).
    /// A one-way platform holds the player from
    /// above but is pass-through from below; the
    /// drop-through (`drop_platform_row`) excludes
    /// the platform row being dropped through so
    /// the fall goes through instead of landing
    /// back on it.
    pub fn landing_solid(&self, x: i32, y: i32, drop_row: Option<i32>) -> bool {
        let f = self.get(x, y);
        if f & 1 != 0 {
            return true;
        }
        if f & 8192 != 0 {
            if let Some(r) = drop_row {
                if r == y {
                    return false;
                }
            }
            return true;
        }
        false
    }

    /// True when any of the `flag` bits are set at (x, y).
    pub fn has(&self, x: i32, y: i32, flag: i32) -> bool {
        self.get(x, y) & flag != 0
    }

    /// Set flag bits at (x, y) (the original's `b(x, y,
    /// flag)`). Used at runtime by the box push (`u()`,
    /// slice 22); out-of-bounds writes are ignored.
    pub fn set_flag(&mut self, x: i32, y: i32, flag: i32) {
        if x >= 0 && (x as usize) < self.width && y >= 0 && (y as usize) < self.height {
            self.flags[(x as usize) + self.width * (y as usize)] |= flag as i16;
        }
    }

    /// Clear flag bits at (x, y) (the original's `c(x, y,
    /// flag)`). Used at runtime by the box push (`u()`).
    pub fn clear_flag(&mut self, x: i32, y: i32, flag: i32) {
        if x >= 0 && (x as usize) < self.width && y >= 0 && (y as usize) < self.height {
            self.flags[(x as usize) + self.width * (y as usize)] &= !(flag as i16);
        }
    }
}

/// Player half-width snap `hx` and height in tiles `hg`
/// (slice 21): from the action-0 bbox (anim 28672 +
/// sprite 28673), which is 42x34 px, so `gC = 42>>1 =
/// 21`, `gD = 34`, `hg = 34/16 = 2`, `hx = 21>>1 =
/// 10`. Fixed for the level (the original computes them
/// once in `t()`); pinned by `player_bbox_constants`.
pub const PLAYER_HX: i32 = 10;
pub const PLAYER_HG: i32 = 2;
/// Player body height `gD` (action-0 bbox height,
/// 34 px): the hang snap is `gb = ledge_row*16 +
/// gD`, and the vault (38) nets -gD, so the mount
/// ends exactly on the ledge top.
pub const PLAYER_GD: i32 = 34;

/// A camera zone (slice 23): a type-3 entity point
/// (`aE`/`aF`) that shifts the camera while it is
/// inside the viewport. `vertical` is `aH` (true =
/// the zone shifts the camera vertically, `aJ`);
/// `negative` is `aG` (true = the offset decreases).
/// The offset step is `aK * L >> 8` = `45 * 40 >> 8`
/// = 7 px per tick (`aK = 45`, from e.java:102).
#[derive(Clone, Copy, Debug)]
pub struct CameraZone {
    pub x: i32,
    pub y: i32,
    pub vertical: bool,
    pub negative: bool,
}

/// A type-7 camera zone (Phase 8, e.java:3193
/// loader + 1588 camera): a horizontal-only
/// zone. `x`/`y` are the `fd`/`fe` px point,
/// `half_w` is `ff` (half the zone sprite's
/// width, from set 2048 action `fa`), `enabled`
/// is `fj`, `negative` is `fk`. When enabled and
/// the point `(x + half_w if negative, y)` is in
/// the viewport, the camera offset `aI` steps
/// toward `negative` and snaps so the point
/// stays in view.
#[derive(Debug, Clone, Copy)]
pub struct CameraZone7 {
    pub x: i32,
    pub y: i32,
    pub half_w: i32,
    pub enabled: bool,
    pub negative: bool,
}

/// Camera-zone offset step (`aK * L >> 8`).
pub const CAMERA_ZONE_STEP: i32 = 7;

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
    /// Camera-zone offsets `aI`/`aJ` (slice 23):
    /// added to the camera while a zone is in view,
    /// decaying toward 0 when none is.
    pub cam_zone_i: i32,
    pub cam_zone_j: i32,
    /// Type-3 camera zones for the current level.
    pub camera_zones: Vec<CameraZone>,
    /// Type-7 camera zones (Phase 8): horizontal
    /// only, from `entities::camera_zones_7`.
    pub camera_zones_7: Vec<CameraZone7>,
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
    /// magic modes (`hc`/`hd`, Phase 3 casts).
    pub weapon_mode: i32,
    /// Magic upgrade levels (Phase 3, save fields
    /// `gV`/`gW`, 0-4; fresh-save default 0; Game
    /// overwrites from the save): gV indexes the
    /// mode-2 stagger table `fK`, gW indexes the
    /// mode-3 damage-orb table `fL`.
    pub magic_level_mode2: i32,
    pub magic_level_mode3: i32,
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
    /// Climbing off the top `gu` (slice 21): set when a
    /// climb-up runs out of wall above; the ledge
    /// mount (slice 23) consumes it.
    pub climbing_top: bool,
    /// Grabbed box index `gd` (slice 22): Some while
    /// the player holds a type-27 box (action 21/22/23),
    /// None otherwise.
    pub grabbed_box: Option<usize>,
    /// Push/pull direction `gl` (slice 22): 1 = push
    /// (action 22), 0 = pull (action 23). Set when a
    /// push/pull starts; read by `u()`.
    pub push_pull: i32,
    /// One-way drop-through row (slice 23): `Some(R)`
    /// while the player is falling through the
    /// flag-8192 platform at row `R` (set by
    /// `drop_through`, cleared once the player is
    /// below that row). The landing checks ignore
    /// flag 8192 at this row so the drop goes
    /// through instead of landing back on it.
    pub drop_platform_row: Option<i32>,
    /// Death animation (action 52, Phase 7) has
    /// finished playing: the caller (Gameplay) moves
    /// to the death screen (state 108). Set only by
    /// the action-52 tick branch.
    pub death_finished: bool,
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
            magic_level_mode2: 0,
            magic_level_mode3: 0,
            blocking: false,
            combo_max_artemis: 2,
            combo_shift_artemis: 0,
            h_k: 0,
            h_l: 0,
            climbing_top: false,
            grabbed_box: None,
            push_pull: 0,
            drop_platform_row: None,
            death_finished: false,
            cam_zone_i: 0,
            cam_zone_j: 0,
            camera_zones: Vec::new(),
            camera_zones_7: Vec::new(),
        }
    }

    /// Set the type-3 camera zones for the level
    /// (slice 23), from `entities::camera_zones`.
    /// The offsets are NOT reset here (they decay
    /// naturally when no zone is in view); a fresh
    /// Player starts them at 0.
    pub fn set_camera_zones(&mut self, zones: Vec<CameraZone>) {
        self.camera_zones = zones;
    }

    /// Set the type-7 camera zones for the level
    /// (Phase 8), from `entities::camera_zones_7`.
    pub fn set_camera_zones_7(&mut self, zones: Vec<CameraZone7>) {
        self.camera_zones_7 = zones;
    }

    /// True while the player holds a box (the grab pose
    /// 21 or a push/pull 22/23, slice 22).
    pub fn grabbing(&self) -> bool {
        matches!(self.action, 21 | 22 | 23)
    }

    /// Anim action for a logical action (Phase 8,
    /// e.java `s()`): when the Blade of Artemis is
    /// equipped (`hb = (fM == 1)`, i.e. weapon_mode
    /// 1), some actions play a variant animation
    /// while `player.action` stays the logical one:
    /// 27->29, 3->4, 97->98, 5->6, 8->9, 100->101,
    /// 38->39. All other actions (and all actions
    /// with the blades or magic modes) play as-is.
    pub fn anim_action(action: usize, weapon_mode: i32) -> usize {
        if weapon_mode != 1 {
            return action;
        }
        match action {
            27 => 29,
            3 => 4,
            97 => 98,
            5 => 6,
            8 => 9,
            100 => 101,
            38 => 39,
            _ => action,
        }
    }

    /// Set the player's action and (re)create its
    /// anim slot with the Artemis variant applied
    /// (see `anim_action`). Use this wherever the
    /// player's action changes, so the variant is
    /// never skipped.
    pub fn set_action(&mut self, anim: &AnimSet, action: usize) {
        self.action = action;
        self.slot = AnimSlot::new(anim, Self::anim_action(action, self.weapon_mode));
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

    /// Magic mode 2 (`hc`, Phase 3): channeled
    /// stagger magic, action 94.
    pub fn is_magic_mode2(&self) -> bool {
        self.weapon_mode == 2
    }

    /// Magic mode 3 (`hd`, Phase 3): burst damage
    /// orbs, action 95.
    pub fn is_magic_mode3(&self) -> bool {
        self.weapon_mode == 3
    }

    /// Mode-2 stagger table `fK` (e.java:591),
    /// indexed by `gV` (magic_level_mode2, 0-4).
    /// A mode-2 hit adds `(fK[gV] << 5) + L` to
    /// the enemy's `dX` stagger timer (Phase 6;
    /// dX itself is not ported yet).
    pub const F_K: [i32; 5] = [2, 3, 4, 5, 6];

    /// Mode-3 damage-orb value table `fL`
    /// (e.java:592), indexed by `gW`
    /// (magic_level_mode3, 0-4). The cast spawns
    /// type-3 orbs totalling this value.
    pub const F_L: [i32; 5] = [128, 256, 512, 1024, 1536];

    /// Mode-3 cast cost (e.java:3712): one flat
    /// 256 magic per `b()` call, not per orb.
    pub const MODE3_COST: i32 = 256;

    /// Mode-2 channel drain (e.java:5190): 12
    /// magic per tick while action 94 is held.
    pub const MODE2_DRAIN_PER_TICK: i32 = 12;

    /// Mode-2 start gate (e.java:5029): magic
    /// must exceed `(gV + 1) << 2`.
    pub fn mode2_start_gate(&self) -> i32 {
        (self.magic_level_mode2 + 1) << 2
    }

    /// Start the mode-2 channeled cast (action 94,
    /// 16 frames, repeat 63 = loops while held).
    pub fn start_magic_cast_94(&mut self, anim: &AnimSet) {
        self.action = 94;
        self.slot = AnimSlot::new(anim, 94);
    }

    /// Start the mode-3 burst cast (action 95,
    /// 17 frames, repeat 1 = plays once).
    pub fn start_magic_cast_95(&mut self, anim: &AnimSet) {
        self.action = 95;
        self.slot = AnimSlot::new(anim, 95);
    }

    /// End a magic cast: back to idle and reset
    /// the weapon to blades (`A()`: fM = 0).
    pub fn end_magic_cast(&mut self, anim: &AnimSet) {
        self.action = 0;
        self.slot = AnimSlot::new(anim, 0);
        self.unequip_all();
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
    ///
    /// Double jump (slice 19): Up while the first arc
    /// runs (action 24) starts the second arc (action
    /// 25, 15 frames, peak -48, dx -> 74) — the
    /// original's case 1 handler: `if (hA != 24)
    /// break; s(25)`. No double jump from a fall
    /// (action 26): the original breaks there too.
    /// No triple jump: action 25 itself is not 24.
    ///
    /// Jump out of a box grab (slice 22): Up while
    /// holding a box (action 21) jumps (24) — the
    /// original's case 1 handler: `if (hA == 21)
    /// { s(24); break; }`.
    pub fn jump(&mut self, anim: &AnimSet) -> bool {
        // Ledge mount (slice 23): Up while hanging
        // (action 37) vaults onto the ledge (38) — the
        // `g()` top-out `s(38)` after a `s(37)` hang.
        if self.action == 37 {
            self.set_action(anim, 38);
            self.h_k = 0;
            self.h_l = 0;
            return true;
        }
        if self.action == 21 {
            self.action = 24;
            self.slot = AnimSlot::new(anim, 24);
            self.h_k = 0;
            self.h_l = 0;
            self.grabbed_box = None;
            return true;
        }
        if self.action == 24 {
            self.action = 25;
            self.slot = AnimSlot::new(anim, 25);
            self.h_k = 0;
            self.h_l = 0;
            return true;
        }
        if self.action != 0 && self.action != 9 {
            return false;
        }
        self.action = 24;
        self.slot = AnimSlot::new(anim, 24);
        self.h_k = 0;
        self.h_l = 0;
        true
    }

    /// On a climbable wall `gn` (slice 21): the action
    /// is 33 (hold), 34 (down), 35 (up) or 36 (attack).
    pub fn climbing(&self) -> bool {
        matches!(self.action, 33 | 34 | 35 | 36)
    }

    /// The wall column ahead, in the facing direction.
    fn wall_col(&self) -> i32 {
        if self.facing_right {
            self.tile_x() + 1
        } else {
            self.tile_x() - 1
        }
    }

    /// True when a box (flag 512) is still adjacent in
    /// the facing direction (slice 22): the tile ahead
    /// at the walk row or the two rows above it. Used
    /// when a push/pull ends, to decide whether the
    /// player holds the box again (21) or idles (0).
    fn box_ahead(&self, flags: &TileFlags) -> bool {
        let gy = self.walk_tile_y();
        let ahead = self.wall_col();
        flags.has(ahead, gy, 512) || flags.has(ahead, gy - 1, 512) || flags.has(ahead, gy - 2, 512)
    }

    /// Attach to a climbable wall (slice 21): the `f()`
    /// wall-stop snap plus `s(33)`. `wall_col` is the
    /// tile column being moved into. Facing right the
    /// player snaps to `wall*16 - hx`; facing left to
    /// `(wall+1)*16 + hx` (the original's `- hy`, with
    /// `hy = -hx`).
    pub fn attach_climb(&mut self, anim: &AnimSet, wall_col: i32) -> bool {
        if self.action == 38 {
            return false;
        }
        self.ga = if self.facing_right {
            wall_col * 16 - PLAYER_HX
        } else {
            (wall_col + 1) * 16 + PLAYER_HX
        };
        self.action = 33;
        self.slot = AnimSlot::new(anim, 33);
        self.h_k = 0;
        self.h_l = 0;
        self.climbing_top = false;
        true
    }

    /// Whether the wall can be attached to from a jump
    /// (slice 21): the `f()` attach check. `dy` is the
    /// current frame's vertical root motion (the `b`
    /// sign): rising checks the two flag-8 tiles below
    /// the feet row, falling checks the two above.
    /// Both must be flag 8 (`hg = 2`).
    pub fn can_attach_climb(&self, flags: &TileFlags, wall_col: i32, dy: i32) -> bool {
        if dy == 0 || self.action == 38 {
            return false;
        }
        let gy = self.walk_tile_y();
        let (r1, r2) = if dy > 0 {
            (gy - 1, gy - PLAYER_HG)
        } else {
            (gy + 1, gy + PLAYER_HG)
        };
        flags.has(wall_col, r1, 8) && flags.has(wall_col, r2, 8)
    }

    /// Ledge grab (slice 23): find the ledge row in the
    /// wall column ahead, from `g()`'s top-out block.
    /// Returns `Some(row)` when the column has a solid
    /// tile at `row` with the tile above clear (a ledge
    /// top), the player's feet are within reach of that
    /// top, and the approach-side tiles are clear (the
    /// `g()` guard's beside checks: the tile at the
    /// ledge row, one above, and `hg` below, all on the
    /// player's side of the wall). The hang snap is
    /// `gb = row*16 + gD`.
    fn ledge_row(&self, flags: &TileFlags) -> Option<i32> {
        let ahead = self.wall_col();
        // The approach side is the column behind the
        // wall (the player's side): `n5` in `g()` is -1
        // facing right, +1 facing left.
        let approach = if self.facing_right { ahead - 1 } else { ahead + 1 };
        let feet_row = self.gb / 16;
        for r in (feet_row - 3)..=(feet_row + 1) {
            if r < 1 {
                continue;
            }
            if !flags.solid(ahead, r) {
                continue;
            }
            if flags.solid(ahead, r - 1) {
                continue;
            }
            if flags.solid(approach, r)
                || flags.solid(approach, r - 1)
                || flags.solid(approach, r + PLAYER_HG)
            {
                continue;
            }
            let top = r * 16;
            if self.gb >= top - 16 && self.gb <= top + PLAYER_GD + 16 {
                return Some(r);
            }
        }
        None
    }

    /// Drop from a ledge hang (slice 23): Down while
    /// hanging (37) lets go into a fall (26).
    pub fn drop_from_hang(&mut self, anim: &AnimSet) -> bool {
        if self.action != 37 {
            return false;
        }
        self.action = 26;
        self.slot = AnimSlot::new(anim, 26);
        self.h_k = 0;
        self.h_l = 0;
        true
    }

    /// Drop through a one-way platform (slice 23):
    /// pressed-Down case 6, `if (!gz) break; s(26)`,
    /// where `gz` is flag 8192 (type-0 runs) under the
    /// player's feet. Only from grounded actions
    /// (0/9) and only when the tile below is a
    /// one-way platform (8192 without flag 1 — a
    /// solid tile is real ground, not a platform).
    /// Sets `drop_platform_row` so the fall's
    /// landing check ignores that row and the player
    /// goes through instead of landing back on it.
    /// Returns false when there is no platform to
    /// drop through.
    pub fn drop_through(&mut self, anim: &AnimSet, flags: &TileFlags) -> bool {
        if self.action != 0 && self.action != 9 {
            return false;
        }
        let row = self.walk_tile_y() + 1;
        let f = flags.get(self.tile_x(), row);
        if f & 8192 == 0 || f & 1 != 0 {
            return false;
        }
        self.action = 26;
        self.slot = AnimSlot::new(anim, 26);
        self.h_k = 0;
        self.h_l = 0;
        self.drop_platform_row = Some(row);
        true
    }

    /// Snap to a ledge hang (slice 23): `s(37)`. The
    /// player hangs from the ledge top with their feet
    /// `gD` below it. Returns false when no ledge is
    /// in reach.
    pub fn grab_ledge(&mut self, anim: &AnimSet, flags: &TileFlags) -> bool {
        let Some(row) = self.ledge_row(flags) else {
            return false;
        };
        self.gb = row * 16 + PLAYER_GD;
        self.action = 37;
        self.slot = AnimSlot::new(anim, 37);
        self.h_k = 0;
        self.h_l = 0;
        self.climbing_top = false;
        true
    }

    /// Climb up one cycle (slice 21): Up while holding
    /// (action 33), from the held-key case 1. Blocked
    /// only when the tile above the player's top
    /// (wall column, row `gx - hg - 1`) is solid AND
    /// not climbable. Returns false when blocked.
    pub fn climb_up(&mut self, anim: &AnimSet, flags: &TileFlags) -> bool {
        if self.action != 33 {
            return false;
        }
        // Climb top-out (slice 23): at the top of the
        // column (`gu`, set by the 35 wall-out) Up
        // vaults onto the ledge (38) instead of
        // another climb cycle — the `g()` branch
        // `gu && b < 0 -> s(38)`.
        if self.climbing_top {
            self.set_action(anim, 38);
            self.climbing_top = false;
            return true;
        }
        let above = self.walk_tile_y() - PLAYER_HG - 1;
        let tile = flags.get(self.wall_col(), above);
        if tile & 8 == 0 && tile & 1 != 0 {
            return false;
        }
        self.action = 35;
        self.slot = AnimSlot::new(anim, 35);
        true
    }

    /// Climb down one cycle (slice 21): Down while
    /// holding (action 33), from the held-key case 6.
    /// When flag 8 continues below (wall column, row
    /// `gx + 1`) the player climbs down (34); otherwise
    /// they drop (26). Returns true either way: the
    /// press is consumed as a climb action.
    pub fn climb_down(&mut self, anim: &AnimSet, flags: &TileFlags) -> bool {
        if self.action != 33 {
            return false;
        }
        if flags.has(self.wall_col(), self.walk_tile_y() + 1, 8) {
            self.action = 34;
            self.slot = AnimSlot::new(anim, 34);
        } else {
            self.action = 26;
            self.slot = AnimSlot::new(anim, 26);
            self.h_k = 0;
            self.h_l = 0;
        }
        true
    }

    /// Climb turn (slice 21): Left/Right pressed
    /// opposite to the facing while holding (action
    /// 33), from the pressed-key cases 5/2: the
    /// facing flips and action 44 plays (8 frames,
    /// 800 ms, dx to 51, dy to -40 and back). Returns
    /// false when not turning (same direction, or
    /// not holding).
    pub fn climb_turn(&mut self, anim: &AnimSet, dir: i32) -> bool {
        if self.action != 33 || dir == 0 {
            return false;
        }
        let want_right = dir > 0;
        if want_right == self.facing_right {
            return false;
        }
        self.facing_right = want_right;
        self.action = 44;
        self.slot = AnimSlot::new(anim, 44);
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
        // Climb attack (slice 21): fire while on the
        // wall (`A(); s(36)` in the pressed-key case 8)
        // unequips the weapon and plays action 36
        // (10 frames, repeat 1, no motion). Only from
        // the hold / climb actions, not mid-attack.
        if self.climbing() && !matches!(self.action, 36) {
            self.unequip_all();
            self.action = 36;
            self.slot = AnimSlot::new(anim, 36);
            return true;
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
            // Magic modes (Phase 3): start the cast
            // action; the caller (Gameplay / --play)
            // applies the costs and spawns the orbs.
            // Mode 2 (hc): action 94, channeled
            // (loops while attack is pressed, drains
            // 12 magic/tick). Mode 3 (hd): action 95,
            // one-shot burst (spawns damage orbs).
            if self.action == 94 || self.action == 95 {
                return false;
            }
            if self.weapon_mode == 2 {
                self.start_magic_cast_94(anim);
            } else {
                self.start_magic_cast_95(anim);
            }
            return true;
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

    /// Start the block stance (slice 18, Phase 6):
    /// Down pressed while grounded and idle/walking,
    /// as in the original's case 6 (`hw = true;
    /// s(27)` — the ENTER pose; the hold pose 3
    /// follows when 27 ends while Down is still
    /// held, e.java:5216). Returns false otherwise
    /// (attacking, airborne, or already blocking).
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
        self.set_action(anim, 27);
        self.blocking = true;
        true
    }

    /// End the block stance (slice 18, Phase 6):
    /// Down released (`t != 6` -> `hw = false`)
    /// plays the EXIT pose 28 (e.java:5324:
    /// `hA == 3 && !hw -> s(28)`); the tick
    /// returns to idle when 28 ends.
    pub fn stop_block(&mut self, anim: &AnimSet) {
        if self.blocking {
            self.action = 28;
            self.slot = AnimSlot::new(anim, 28);
            self.blocking = false;
        }
    }

    /// Hurt reaction (Phase 6, e.java:5378-5382):
    /// the player took `damage` from an attacker
    /// facing `attacker_facing_right` (the
    /// original's `gf = !ej`, i.e. the direction
    /// the player must face to look at the
    /// attacker). Damage > 50 turns the player
    /// to face the attacker and plays the big
    /// hurt (7); facing the attacker already
    /// plays the small hurt (5); facing away
    /// plays the from-behind hurt (100). The
    /// caller checks the guards (grounded, not
    /// blocking, not in 27/28/29/60/77, alive).
    pub fn hurt_reaction(&mut self, anim: &AnimSet, damage: i32, attacker_facing_right: bool) {
        if damage > 50 {
            self.facing_right = attacker_facing_right;
            self.action = 7;
        } else if self.facing_right == attacker_facing_right {
            self.action = 5;
        } else {
            self.action = 100;
        }
        self.slot = AnimSlot::new(anim, Self::anim_action(self.action, self.weapon_mode));
    }

    /// One 40 ms tick. `dir`: -1 left, 0 none, +1 right.
    /// Returns the root-motion delta applied (px, signed).
    pub fn tick(&mut self, anim: &AnimSet, flags: &TileFlags, level_w: i32, level_h: i32, dir: i32) -> i32 {
        let mut applied = 0;
        // Death trigger (Phase 7, e.java:5327-5333,
        // the `z()` tail after the action-end chain):
        // health <= 0 (and not grappling — not
        // ported) starts the death. `gy` is the
        // GROUNDED flag (the tile below the feet is
        // solid): grounded and not already dead
        // (52/53) plays the death action 52 at once
        // (and pins health to 0); airborne and not
        // already falling (26) plays the fall.
        // Pushing/pulling a box (22/23) defers it
        // (the original's `gk && hB != 21` guard);
        // holding a box (21) does not.
        if self.health <= 0
            && !matches!(self.action, 52 | 53 | 22 | 23)
        {
            let grounded = flags.landing_solid(
                self.tile_x(),
                self.walk_tile_y() + 1,
                self.drop_platform_row,
            );
            if grounded {
                self.action = 52;
                self.slot = AnimSlot::new(anim, 52);
                self.health = 0;
            } else if self.action != 26 {
                self.action = 26;
                self.slot = AnimSlot::new(anim, 26);
                self.h_k = 0;
                self.h_l = 0;
            }
        }
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
        if self.action == 27 {
            // Block ENTER pose (Phase 6, e.java:5216):
            // a one-shot pose; when it ends, the
            // hold pose 3 follows while Down is
            // still held, else the exit pose 28.
            self.slot.advance(anim, 40);
            if self.slot.repeat == 0 {
                if self.blocking {
                    self.set_action(anim, 3);
                } else {
                    self.action = 28;
                    self.slot = AnimSlot::new(anim, 28);
                }
            }
            let target_x = self.ga - 120 + if self.facing_right { 60 } else { -60 };
            let target_y = self.gb - 266;
            self.update_camera(target_x, target_y, level_w, level_h);
            return 0;
        }
        if self.action == 28 {
            // Block EXIT pose (Phase 6, e.java:5324):
            // one-shot; returns to idle when it ends.
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
        if self.action == 3 {
            // Block stance (slice 18): a single
            // 100 ms pose, re-started when it ends
            // so the stance holds while Down is held
            // (the original's `hw && hA == 3 -> s(3)`
            // re-affirmation). If Down was released
            // mid-pose, the exit pose 28 follows
            // instead. No walk, no root motion.
            self.slot.advance(anim, 40);
            if self.slot.repeat == 0 {
                if self.blocking {
                    self.slot = AnimSlot::new(anim, Self::anim_action(3, self.weapon_mode));
                } else {
                    self.action = 28;
                    self.slot = AnimSlot::new(anim, 28);
                }
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
        if self.action == 5 || self.action == 7 || self.action == 100 {
            // Hurt reactions (Phase 6): one-shot
            // poses (5 small, 7 big, 100 from
            // behind) with the animation's root
            // motion (the knockback slide), then
            // idle. The dx is authored backward
            // (5/7 total -32, 100 total +46, set
            // 28672), so it is applied RAW with the
            // facing sign — not dx.abs() like the
            // forward-moving swings (the pull, 23,
            // uses the same raw convention).
            let (dx, _dy) = self.slot.advance(anim, 40);
            self.ga += if self.facing_right { dx } else { -dx };
            if self.slot.repeat == 0 {
                self.action = 0;
                self.slot = AnimSlot::new(anim, 0);
            }
            let target_x = self.ga - 120 + if self.facing_right { 60 } else { -60 };
            let target_y = self.gb - 266;
            self.update_camera(target_x, target_y, level_w, level_h);
            return dx;
        }
        if matches!(self.action, 12 | 88 | 91 | 113 | 114 | 117) {
            // Grapple / finisher / throw (Phase 8):
            // one-shot poses with root motion. The
            // grapple actions (88/91) do NOT return
            // to idle here — Gameplay resolves the
            // mini-game (success 89/92, fail 90/93)
            // when the animation ends. The finisher
            // (12) and throws (113/114/117) return
            // to idle when they end.
            let (dx, _dy) = self.slot.advance(anim, 40);
            self.ga += if self.facing_right { dx } else { -dx };
            if self.slot.repeat == 0 && !matches!(self.action, 88 | 91) {
                self.action = 0;
                self.slot = AnimSlot::new(anim, 0);
            }
            let target_x = self.ga - 120 + if self.facing_right { 60 } else { -60 };
            let target_y = self.gb - 266;
            self.update_camera(target_x, target_y, level_w, level_h);
            return dx;
        }
        if self.action == 52 {
            // Death (Phase 7): an 18-frame one-shot
            // (set 28672, repeat 1, ~1.6 s, no root
            // motion). When it ends, `death_finished`
            // tells the caller to show the death
            // screen (state 108, e.java:5363:
            // `hA == 52 && !bl3` — the animation's
            // repeats are spent). The player stays in
            // the dead pose until the caller respawns
            // or quits.
            self.slot.advance(anim, 40);
            if self.slot.repeat == 0 {
                self.death_finished = true;
            }
            let target_x = self.ga - 120 + if self.facing_right { 60 } else { -60 };
            let target_y = self.gb - 266;
            self.update_camera(target_x, target_y, level_w, level_h);
            return 0;
        }
        if self.action == 95 {
            // Mode-3 burst cast (Phase 3): a one-shot
            // 17-frame animation (repeat 1). The
            // orbs were spawned and the weapon reset
            // to blades at fire time; when the
            // animation finishes, return to idle.
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
        if self.action == 15 || self.action == 17 || self.action == 31 {
            // Lever pull (Phase 5): one-shot pull
            // (15, 13 frames) / un-pull (17, 7 frames)
            // animations. The doors already toggled
            // at pull time; when the animation
            // finishes, return to idle.
            // Pickup pose (Phase 5, action 31): plays
            // when a type-4 collectible is touched;
            // the orbs / upgrade were granted at once.
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
        if self.action == 94 {
            // Mode-2 channeled cast (Phase 3): a
            // looping 16-frame animation (repeat 63).
            // It plays until the caller ends it (attack
            // released or magic spent); the drain is
            // the caller's job (Gameplay, 12/tick).
            self.slot.advance(anim, 40);
            let target_x = self.ga - 120 + if self.facing_right { 60 } else { -60 };
            let target_y = self.gb - 266;
            self.update_camera(target_x, target_y, level_w, level_h);
            return 0;
        }
        if self.action == 21 {
            // Box grab hold (slice 22): a looping 3-frame
            // pose (repeat 63), no motion, no gravity. The
            // player holds until a press changes the action
            // (push 22 / pull 23 / jump 24, via the caller).
            self.slot.advance(anim, 40);
            let target_x = self.ga - 120 + if self.facing_right { 60 } else { -60 };
            let target_y = self.gb - 266;
            self.update_camera(target_x, target_y, level_w, level_h);
            return 0;
        }
        if self.action == 22 || self.action == 23 {
            // Box push (22) / pull (23), slice 22: the
            // box itself moved at the press (u()); the
            // player's action just plays out. The push
            // has no root motion (dx 0); the pull steps
            // back 16 px (dx 0 -> -16, applied raw with
            // the facing sign, not dx.abs()). When the
            // action finishes, the player holds the box
            // again (21) if still adjacent to it, else
            // returns to idle.
            let (dx, _dy) = self.slot.advance(anim, 40);
            if self.action == 23 {
                let gy = self.walk_tile_y();
                let behind = if self.facing_right {
                    self.tile_x() - 1
                } else {
                    self.tile_x() + 1
                };
                let blocked = flags.solid(behind, gy)
                    || flags.solid(behind, gy - 1)
                    || flags.solid(behind, gy - 2);
                if !blocked {
                    self.ga += if self.facing_right { dx } else { -dx };
                }
            }
            if self.slot.repeat == 0 {
                if self.box_ahead(flags) {
                    self.action = 21;
                    self.slot = AnimSlot::new(anim, 21);
                } else {
                    self.action = 0;
                    self.slot = AnimSlot::new(anim, 0);
                    self.grabbed_box = None;
                }
            }
            let target_x = self.ga - 120 + if self.facing_right { 60 } else { -60 };
            let target_y = self.gb - 266;
            self.update_camera(target_x, target_y, level_w, level_h);
            return dx;
        }
        if self.action == 33 {
            // Climb hold (slice 21): a looping 3-frame
            // pose (repeat 63), no motion, no gravity.
            // The player hangs until a press changes
            // the action (climb_up / climb_down /
            // climb_turn / attack, via the caller) or
            // the ledge mount (slice 23) fires. The
            // camera target sits 80 px lower while
            // climbing (`gn ? 80 : 0` in `e.o()`).
            self.slot.advance(anim, 40);
            let target_x = self.ga - 120 + if self.facing_right { 60 } else { -60 };
            let target_y = self.gb - 266 + 80;
            self.update_camera(target_x, target_y, level_w, level_h);
            return 0;
        }
        if self.action == 34 || self.action == 35 {
            // Climb cycles (slice 21): one 8-frame
            // cycle (600 ms, 34 down +20 px, 35 up
            // -19 px), then back to the hold pose
            // (33). The original loops the action
            // (repeat 63); the port runs one cycle
            // per press, matching its press-based
            // input. The frame wrap (7 -> 0) ends
            // the cycle; the advance's wrap handling
            // keeps the root motion from snapping back.
            let prev_frame = self.slot.frame;
            let (_dx, dy) = self.slot.advance(anim, 40);
            self.gb += dy;
            let wall = self.wall_col();
            if self.action == 34 {
                // Landing cancels the descent.
                if dy > 0 && self.gb >= 0 && flags.landing_solid(self.tile_x(), self.gb / 16, self.drop_platform_row) {
                    self.gb = (self.gb / 16) * 16;
                    self.action = 0;
                    self.slot = AnimSlot::new(anim, 0);
                } else if !flags.has(wall, self.walk_tile_y(), 8) {
                    // Wall ran out below: the `f()`
                    // climb branch drops the player
                    // (`hB == 34 -> s(26)`).
                    self.gb = self.walk_tile_y() * 16;
                    self.action = 26;
                    self.slot = AnimSlot::new(anim, 26);
                    self.h_k = 0;
                    self.h_l = 0;
                } else if prev_frame != 0 && self.slot.frame == 0 {
                    self.action = 33;
                    self.slot = AnimSlot::new(anim, 33);
                }
            } else {
                // Climbing up: when the wall runs out
                // above, the `f()` branch sets `gu`
                // (the top-out flag) and the player
                // holds at the top; the ledge mount
                // (slice 23) consumes `gu`.
                if !flags.has(wall, self.walk_tile_y(), 8) {
                    self.gb = self.walk_tile_y() * 16;
                    self.climbing_top = true;
                    self.action = 33;
                    self.slot = AnimSlot::new(anim, 33);
                } else if prev_frame != 0 && self.slot.frame == 0 {
                    self.action = 33;
                    self.slot = AnimSlot::new(anim, 33);
                }
            }
            let target_x = self.ga - 120 + if self.facing_right { 60 } else { -60 };
            let target_y = self.gb - 266 + 80;
            self.update_camera(target_x, target_y, level_w, level_h);
            return dy;
        }
        if self.action == 36 {
            // Climb attack (slice 21): 10 frames,
            // repeat 1, no motion; back to the hold
            // pose when it finishes.
            self.slot.advance(anim, 40);
            if self.slot.repeat == 0 {
                self.action = 33;
                self.slot = AnimSlot::new(anim, 33);
            }
            let target_x = self.ga - 120 + if self.facing_right { 60 } else { -60 };
            let target_y = self.gb - 266 + 80;
            self.update_camera(target_x, target_y, level_w, level_h);
            return 0;
        }
        if self.action == 44 {
            // Climb turn (slice 21): 8 frames, repeat
            // 1, dx to 51 in the new facing direction
            // (the facing flipped before `s(44)`),
            // dy to -40 and back; back to the hold
            // pose when it finishes. The root motion
            // is applied without a wall check (the
            // turn moves away from the old wall face).
            let (dx, dy) = self.slot.advance(anim, 40);
            self.ga += if self.facing_right { dx.abs() } else { -dx.abs() };
            self.gb += dy;
            if self.slot.repeat == 0 {
                self.action = 33;
                self.slot = AnimSlot::new(anim, 33);
            }
            let target_x = self.ga - 120 + if self.facing_right { 60 } else { -60 };
            let target_y = self.gb - 266 + 80;
            self.update_camera(target_x, target_y, level_w, level_h);
            return dx;
        }
        if self.action == 37 {
            // Ledge hang (slice 23): a looping 6-frame
            // pose (repeat 63), no motion, no gravity.
            // The player hangs from the ledge until Up
            // mounts (38, via `jump()`) or the caller
            // drops them (26). The camera target sits
            // 80 px lower, as while climbing.
            self.slot.advance(anim, 40);
            let target_x = self.ga - 120 + if self.facing_right { 60 } else { -60 };
            let target_y = self.gb - 266 + 80;
            self.update_camera(target_x, target_y, level_w, level_h);
            return 0;
        }
        if self.action == 38 {
            // Ledge mount / top-out vault (slice 23):
            // 11 frames, repeat 1, dx to 30 forward,
            // dy to -42 and back to -34 net. The root
            // motion is applied without a wall check
            // (a scripted mount: the hang snap put the
            // feet `gD` below the ledge, the vault nets
            // -`gD`, so it ends exactly on the ledge
            // top). When it finishes the player stands
            // (0), snapped to the solid row's top.
            let (dx, dy) = self.slot.advance(anim, 40);
            self.ga += if self.facing_right { dx.abs() } else { -dx.abs() };
            self.gb += dy;
            if self.slot.repeat == 0 {
                if self.gb >= 0 && flags.landing_solid(self.tile_x(), self.gb / 16, self.drop_platform_row) {
                    self.gb = (self.gb / 16) * 16;
                }
                self.action = 0;
                self.slot = AnimSlot::new(anim, 0);
                self.h_k = 0;
                self.h_l = 0;
            }
            let target_x = self.ga - 120 + if self.facing_right { 60 } else { -60 };
            let target_y = self.gb - 266;
            self.update_camera(target_x, target_y, level_w, level_h);
            return dx;
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
            // Ledge grab (slice 23): reaching a ledge
            // top while airborne snaps to the hang pose
            // (37) instead of landing or falling on.
            // Only while moving vertically; the
            // `ledge_row` range + approach clearance
            // keep ordinary steps (whose approach side
            // is solid at `hg` below) from grabbing.
            if dy != 0 && self.grab_ledge(anim, flags) {
                let target_x = self.ga - 120 + if self.facing_right { 60 } else { -60 };
                let target_y = self.gb - 266 + 80;
                self.update_camera(target_x, target_y, level_w, level_h);
                return dx;
            }
            if dy > 0 && self.gb >= 0 && flags.landing_solid(self.tile_x(), self.gb / 16, self.drop_platform_row) {
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
        if self.action == 24 || self.action == 25 {
            // Jump arcs (24 first jump, 25 double jump,
            // slice 19): root motion from the animation.
            // Horizontal step uses the facing sign (as
            // walk/attack do); the walk blocked check stops
            // horizontal motion at walls but never stops the
            // vertical arc. Landing is checked only while
            // descending: feet entering a solid tile row
            // snap to that row's top and return to idle, as
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
            if blocked && self.can_attach_climb(flags, ahead, dy) {
                // Wall attach (slice 21): jumping at a
                // flag-8 wall snaps the player to its
                // face and starts the hold pose (33)
                // instead of stopping the arc.
                self.attach_climb(anim, ahead);
                let target_x = self.ga - 120 + if self.facing_right { 60 } else { -60 };
                let target_y = self.gb - 266 + 80;
                self.update_camera(target_x, target_y, level_w, level_h);
                return dx;
            }
            if !blocked {
                self.ga += if self.facing_right { step } else { -step };
            }
            self.gb += dy;
            // Ledge grab (slice 23): reaching a ledge
            // top while airborne snaps to the hang pose
            // (37) instead of landing or falling on.
            // Only while moving vertically; the
            // `ledge_row` range + approach clearance
            // keep ordinary steps (whose approach side
            // is solid at `hg` below) from grabbing.
            if dy != 0 && self.grab_ledge(anim, flags) {
                let target_x = self.ga - 120 + if self.facing_right { 60 } else { -60 };
                let target_y = self.gb - 266 + 80;
                self.update_camera(target_x, target_y, level_w, level_h);
                return dx;
            }
            if dy > 0 && self.gb >= 0 && flags.landing_solid(self.tile_x(), self.gb / 16, self.drop_platform_row) {
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
            // Ledge grab (slice 23): falling past a
            // ledge top snaps to the hang pose (37).
            if self.h_k != 0 && self.grab_ledge(anim, flags) {
                let target_x = self.ga - 120 + if self.facing_right { 60 } else { -60 };
                let target_y = self.gb - 266 + 80;
                self.update_camera(target_x, target_y, level_w, level_h);
                return 0;
            }
            // One-way drop-through (slice 23): once
            // the player has fallen below the platform
            // row they dropped through, the row is no
            // longer excluded from the landing check.
            if let Some(r) = self.drop_platform_row {
                if self.gb / 16 > r {
                    self.drop_platform_row = None;
                }
            }
            if self.gb >= 0 && flags.landing_solid(self.tile_x(), self.gb / 16, self.drop_platform_row) {
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
                // Walked off a ledge (slice 23): when
                // the row below the walk row is no
                // longer solid, the player falls (26)
                // instead of floating — the original's
                // `gy` ground flag clears and `f()`
                // drops them. The fall branch then
                // lands them on the next solid row.
                if !flags.landing_solid(self.tile_x(), self.walk_tile_y() + 1, self.drop_platform_row) {
                    self.action = 26;
                    self.slot = AnimSlot::new(anim, 26);
                    self.h_k = 0;
                    self.h_l = 0;
                }
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

    /// Snap the camera to a target (Phase 7, the
    /// death respawn recentre): sets both the final
    /// camera and the fixed-point base, so the next
    /// `update_camera` continues from here instead
    /// of sliding back.
    pub fn snap_camera(&mut self, x: i32, y: i32) {
        self.cam_x = x;
        self.cam_y = y;
        self.cam_fx = x << 8;
        self.cam_fy = y << 8;
    }

    fn update_camera(&mut self, target_x: i32, target_y: i32, level_w: i32, level_h: i32) {
        // The fixed point (`cam_fx`/`cam_fy`) tracks
        // the BASE camera (before the zone offset);
        // `cam_x`/`cam_y` are the final camera (base
        // + zone offset). The delta is measured from
        // the BASE camera — `o()` computes
        // `(n6 - iQ + aI) * L`, i.e. the target minus
        // the base camera `(iQ - aI)`, not minus the
        // final camera. Measuring from the final
        // camera is a bug: the base camera then
        // compensates for the zone offset, so it sits
        // `zone_j` above the target and the zone
        // points end up on the very edge of the base
        // viewport — the zone flickers in and out as
        // the base wobbles across the edge, pumping
        // the offset up and letting it decay, which
        // shakes the whole screen up and down
        // (9217: cam_y swung 273..304 on an 8-tick
        // cycle while the player stood still).
        let base_x = self.cam_fx >> 8;
        let base_y = self.cam_fy >> 8;
        let cap = 40 << 8;
        let mut nx = (target_x - base_x) * 40;
        if nx > cap {
            nx = cap;
        } else if nx < -cap {
            nx = -cap;
        }
        let mut ny = (target_y - base_y) * 40;
        if ny > cap {
            ny = cap;
        } else if ny < -cap {
            ny = -cap;
        }
        self.cam_fx += nx;
        self.cam_fy += ny;
        let max_x = level_w - 240;
        let max_y = level_h - 320;
        let mut bx = self.cam_fx >> 8;
        let mut by = self.cam_fy >> 8;
        bx = bx.clamp(0, max_x.max(0));
        by = by.clamp(0, max_y.max(0));
        self.cam_fx = bx << 8;
        self.cam_fy = by << 8;
        // Camera zones (slice 23): the `o()` zone
        // loop, run once per frame after the base
        // camera settles. A zone point inside the
        // viewport shifts the offset (`aI`/`aJ`) by
        // the step each tick, snapping so the point
        // stays in view; with no zone in view the
        // offsets decay toward 0 by the step.
        // Type-7 zones are not ported (their object
        // data is not loaded here).
        self.cam_x = bx;
        self.cam_y = by;
        self.update_camera_zones();
        self.cam_x = (bx + self.cam_zone_i).clamp(0, max_x.max(0));
        self.cam_y = (by + self.cam_zone_j).clamp(0, max_y.max(0));
    }

    /// The `o()` zone loop (slice 23): adjust
    /// `cam_zone_i`/`cam_zone_j` from the type-3
    /// zones in view. The viewport test uses the
    /// base camera (before the zone offset).
    fn update_camera_zones(&mut self) {
        let step = CAMERA_ZONE_STEP;
        let mut horiz = false;
        let mut vert = false;
        for zone in &self.camera_zones {
            let in_view = zone.x >= self.cam_x
                && zone.x < self.cam_x + 240
                && zone.y >= self.cam_y
                && zone.y < self.cam_y + 320;
            if !in_view {
                continue;
            }
            if !zone.vertical {
                horiz = true;
                if zone.negative {
                    self.cam_zone_i -= step;
                } else {
                    self.cam_zone_i += step;
                }
                // Snap so the point stays in view.
                let px = zone.x;
                let in_view2 = px >= self.cam_x + self.cam_zone_i
                    && px < self.cam_x + self.cam_zone_i + 240;
                if !in_view2 {
                    self.cam_zone_i = px - self.cam_x - if zone.negative { 240 } else { 0 };
                }
            } else {
                vert = true;
                if zone.negative {
                    self.cam_zone_j -= step;
                } else {
                    self.cam_zone_j += step;
                }
                let py = zone.y;
                let in_view2 = py >= self.cam_y + self.cam_zone_j
                    && py < self.cam_y + self.cam_zone_j + 320;
                if !in_view2 {
                    self.cam_zone_j = py - self.cam_y - if zone.negative { 320 } else { 0 };
                }
            }
        }
        // Type-7 zones (Phase 8, e.java:1588-1599):
        // horizontal-only. When enabled and the
        // point `(x + half_w if negative, y)` is in
        // the viewport, the horizontal offset steps
        // toward `negative` and snaps so the point
        // stays in view. An active zone also holds
        // the offset (sets `horiz`, so the decay
        // below does not run).
        for zone in &self.camera_zones_7 {
            if !zone.enabled {
                continue;
            }
            let px = zone.x + if zone.negative { zone.half_w } else { 0 };
            let in_view = px >= self.cam_x
                && px < self.cam_x + 240
                && zone.y >= self.cam_y
                && zone.y < self.cam_y + 320;
            if !in_view {
                continue;
            }
            horiz = true;
            if zone.negative {
                self.cam_zone_i -= step;
            } else {
                self.cam_zone_i += step;
            }
            let in_view2 = px >= self.cam_x + self.cam_zone_i
                && px < self.cam_x + self.cam_zone_i + 240;
            if !in_view2 {
                self.cam_zone_i = px - self.cam_x - if zone.negative { 240 } else { 0 };
            }
        }
        if !horiz && self.cam_zone_i != 0 {
            if self.cam_zone_i > 0 {
                self.cam_zone_i -= step;
                if self.cam_zone_i < 0 {
                    self.cam_zone_i = 0;
                }
            } else {
                self.cam_zone_i += step;
                if self.cam_zone_i > 0 {
                    self.cam_zone_i = 0;
                }
            }
        }
        if !vert && self.cam_zone_j != 0 {
            if self.cam_zone_j > 0 {
                self.cam_zone_j -= step;
                if self.cam_zone_j < 0 {
                    self.cam_zone_j = 0;
                }
            } else {
                self.cam_zone_j += step;
                if self.cam_zone_j > 0 {
                    self.cam_zone_j = 0;
                }
            }
        }
    }
}


#[cfg(test)]
mod tests {
    #[test]
    /// Slice 23: walking off a ledge falls (26)
    /// instead of floating. The player walks right
    /// on the roof (gb=416); a 1-tile hanging pillar
    /// at x=151 (row 25) blocks the walk, so they
    /// jump over it, then fall off the roof edge to
    /// the temple floor (gb=528).
    #[test]
    fn walk_off_roof_edge_falls_to_temple_floor() {
        let (_parsed, flags, anim) = level_9217();
        let (w, h) = (3712, 624);
        // Start on the roof just left of the pillar
        // (x=141) is blocked, so start past it at
        // tile 145, on the roof (gb=416).
        let mut pl = super::Player::new(&anim, (145 * 16, 416), true, 0, 294);
        let mut fell = false;
        let mut landed = false;
        for _ in 0..200 {
            if pl.action == 0 && pl.gb == 416 {
                pl.jump(&anim);
            } else if pl.action == 37 {
                // Mount a grabbed ledge (38) to get
                // over the pillar instead of hanging.
                pl.jump(&anim);
            }
            pl.tick(&anim, &flags, w, h, 1);
            if pl.action == 26 {
                fell = true;
            }
            if fell && pl.action == 0 && pl.gb == 528 {
                landed = true;
                break;
            }
        }
        assert!(fell, "never fell off the roof edge: ga={} gb={} action={}", pl.ga, pl.gb, pl.action);
        assert!(landed, "never landed on the temple floor: ga={} gb={} action={}", pl.ga, pl.gb, pl.action);
        assert!(pl.ga > 151 * 16, "should be past the roof edge: ga={}", pl.ga);
    }

    /// Slice 23: type-3 camera zones load from the
    /// level (9217 has 31): a horizontal row at y=26
    /// and a vertical column at x=224.
    #[test]
    fn camera_zones_load_for_level_9217() {
        let data = std::fs::read("../extracted/RP9").unwrap();
        let pack = crate::rp::parse_pack(&data).unwrap();
        let chunk = pack.chunk(1).unwrap();
        let parsed = crate::entities::parse_level_entities(9217, &chunk).unwrap();
        let zones = crate::entities::camera_zones(&parsed);
        assert_eq!(zones.len(), 31, "type-3 count {}", zones.len());
        // The first items are the column at tile
        // x=224 (extras [aH=0, aG=1]): a horizontal
        // zone (`aH` false -> `aI`), negative.
        // (`aH` names the zone's effect, not the
        // points' arrangement: the column's points
        // are vertical, the offset is horizontal.)
        assert!(!zones[0].vertical, "zone 0 should be horizontal (aH=0)");
        assert!(zones[0].negative, "zone 0 should be negative (aG=1)");
        assert_eq!(zones[0].x, 224 * 16);
        // A row item (extras [aH=1, aG=0]): a
        // vertical zone (`aH` true -> `aJ`), positive.
        // The row runs at y=24..26; the first
        // vertical item is at (219,6).
        let vert = zones.iter().find(|z| z.vertical).expect("a vertical zone");
        assert!(!vert.negative, "row zones are positive (aG=0)");
        let row26 = zones.iter().find(|z| z.vertical && z.y == 26 * 16).expect("a vertical zone on row 26");
        assert!(!row26.negative, "row-26 zones are positive");
    }

    /// Phase 8: type-7 camera zones load from the
    /// level. Most zones are disabled (`fj` = 0);
    /// 24576 has one enabled, negative zone at
    /// (992,256) with extras `[fa=3, fj=1, fk=1]`.
    /// The half width comes from the zone sprite
    /// (anim set 2048 action `fa`, sprite set
    /// 2051) — here it is a real frame, so the
    /// half width is positive.
    #[test]
    fn camera_zones_7_load_for_level_24576() {
        let data = std::fs::read("../extracted/RP24").unwrap();
        let pack = crate::rp::parse_pack(&data).unwrap();
        let chunk = pack.chunk(0).unwrap();
        let parsed = crate::entities::parse_level_entities(24576, &chunk).unwrap();
        let zones = crate::entities::camera_zones_7(&parsed);
        assert_eq!(zones.len(), 1, "type-7 count {}", zones.len());
        let (x, y, action, enabled, negative) = zones[0];
        assert_eq!((x, y), (992, 256), "type-7 point");
        assert_eq!(action, 3, "fa is the anim action");
        assert!(enabled, "fj is set for the 24576 zone");
        assert!(negative, "fk is set for the 24576 zone");
        // The zone's sprite frame (anim 2048 action
        // `fa`, sprite set 2051) has a real width.
        let anim_data = std::fs::read("../extracted/RP2").unwrap();
        let anim_pack = crate::rp::parse_pack(&anim_data).unwrap();
        let anim_set =
            crate::anim::load_anim_set(&anim_pack.chunk(0).unwrap()).unwrap();
        let mut res = crate::resources::Resources::new("../extracted");
        let sprites = res.sprite_set(2051, None).unwrap();
        let frames = crate::anim::frames_for_timeline_frame(&anim_set, action, 0, 0)
            .unwrap();
        let fr = frames.first().unwrap();
        let half_w = sprites.frames[fr.sprite_idx].w / 2;
        assert!(half_w > 0, "zone sprite has a width");
        // Wiring it into the player moves the
        // camera offset while the zone is in view.
        let mut pl = super::Player::new(
            &crate::anim::load_anim_set(
                &crate::rp::parse_pack(
                    &std::fs::read("../extracted/RP28").unwrap(),
                )
                .unwrap()
                .chunk(0)
                .unwrap(),
            )
            .unwrap(),
            (992, 256),
            true,
            872,
            0,
        );
        pl.set_camera_zones_7(vec![super::CameraZone7 {
            x,
            y,
            half_w,
            enabled,
            negative,
        }]);
        pl.update_camera_zones();
        assert_ne!(pl.cam_zone_i, 0, "type-7 zone shifts the camera offset");
    }

    /// Slice 23: a horizontal zone in view shifts the
    /// camera offset (`aI`) by the step (7 px/tick)
    /// in its direction, and the offset decays back
    /// toward 0 once the zone leaves the view.
    #[test]
    fn camera_zone_shifts_offset_and_decays() {
        let (_parsed, _flags, anim) = level_9217();
        let (w, h) = (3712, 624);
        let mut pl = super::Player::new(&anim, (48, 560), true, 0, 294);
        // A horizontal zone point inside the spawn
        // viewport (0..240 x 294..614), positive
        // direction: the offset grows +7 per tick.
        pl.set_camera_zones(vec![super::CameraZone {
            x: 120,
            y: 400,
            vertical: false,
            negative: false,
        }]);
        pl.tick(&anim, &_flags, w, h, 0);
        assert_eq!(pl.cam_zone_i, 7, "offset should grow +7, got {}", pl.cam_zone_i);
        pl.tick(&anim, &_flags, w, h, 0);
        assert_eq!(pl.cam_zone_i, 14, "offset should grow +7 again, got {}", pl.cam_zone_i);
        // Move the zone out of view: the offset
        // decays back toward 0 by 7 per tick.
        pl.set_camera_zones(vec![super::CameraZone {
            x: 3000,
            y: 400,
            vertical: false,
            negative: false,
        }]);
        pl.tick(&anim, &_flags, w, h, 0);
        assert_eq!(pl.cam_zone_i, 7, "offset should decay to 7, got {}", pl.cam_zone_i);
        pl.tick(&anim, &_flags, w, h, 0);
        assert_eq!(pl.cam_zone_i, 0, "offset should decay to 0, got {}", pl.cam_zone_i);
    }

    /// Slice 23: drop through a one-way platform
    /// (level 20480 has 11 type-0 items). The
    /// platform at tile (63,22) (len 4) holds the
    /// player from above; Down drops through it and
    /// the player falls past it instead of landing
    /// back on it.
    #[test]
    fn drop_through_one_way_platform() {
        let mut res = crate::resources::Resources::new("../extracted");
        let chunk = res.chunk(20480).expect("level 20480 chunk");
        let parsed = crate::entities::parse_level_entities(20480, &chunk).unwrap();
        let footprints = crate::entities::load_footprints(&mut res, &parsed).unwrap();
        let flags = crate::player::TileFlags::build(&parsed, &footprints);
        // The platform at (63,22) is flag 8192 only
        // (not solid), with open space below.
        assert_eq!(flags.get(63, 22) & 8192, 8192, "platform flag 8192 at (63,22)");
        assert_eq!(flags.get(63, 22) & 1, 0, "platform is not solid");
        assert_eq!(flags.get(63, 23), 0, "open space below the platform");
        let (_parsed2, _flags2, anim) = level_9217();
        let (w, h) = (parsed.level.pixel_width() as i32, parsed.level.pixel_height() as i32);
        // Stand on the platform: feet at the top of
        // row 22 (gb=352), tile x=63.
        let mut pl = super::Player::new(&anim, (63 * 16 + 8, 22 * 16), true, 0, 0);
        // The platform holds the player (no fall).
        let dy = pl.tick(&anim, &flags, w, h, 0);
        assert_eq!(pl.action, 0, "player stands on the one-way platform");
        assert_eq!(pl.gb, 352, "feet stay on the platform top");
        // Down drops through it.
        assert!(pl.drop_through(&anim, &flags), "drop_through should fire");
        assert_eq!(pl.action, 26, "player falls (action 26)");
        assert_eq!(pl.drop_platform_row, Some(22));
        // Fall past the platform row: the landing
        // check must not put the player back on it.
        for _ in 0..30 {
            pl.tick(&anim, &flags, w, h, 0);
            if pl.action == 0 {
                break;
            }
        }
        assert!(pl.gb > 22 * 16 + 16, "player fell past the platform row, gb={}", pl.gb);
        assert_eq!(pl.drop_platform_row, None, "drop row clears once below it");
        let _ = dy;
    }

    fn walk_right_advances_and_camera_follows() {
        let data = std::fs::read("../extracted/RP9").unwrap();
        let pack = crate::rp::parse_pack(&data).unwrap();
        let chunk = pack.chunk(1).unwrap();
        let parsed = crate::entities::parse_level_entities(9217, &chunk).unwrap();
        let mut res = crate::resources::Resources::new("../extracted");
        let footprints = crate::entities::load_footprints(&mut res, &parsed).unwrap();
        let flags = super::TileFlags::build(&parsed, &footprints);
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
        let mut res = crate::resources::Resources::new("../extracted");
        let footprints = crate::entities::load_footprints(&mut res, &parsed).unwrap();
        let flags = super::TileFlags::build(&parsed, &footprints);
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

    /// Double jump (slice 19): Up mid first-arc starts
    /// action 25 and rises a second 48 px; the combined
    /// rise is ~96 px over the jump start.
    #[test]
    fn double_jump_rises_higher_than_single_jump() {
        let (parsed, flags, anim) = level_9217();
        let spawn = parsed.group(26).unwrap().positions[0];
        let (cx, cy) = crate::entities::start_camera(&parsed.level, spawn);
        let w = parsed.level.pixel_width() as i32;
        let h = parsed.level.pixel_height() as i32;
        // Single jump: min gb is 512 (560 - 48).
        let mut single = super::Player::new(&anim, spawn, true, cx, cy);
        assert!(single.jump(&anim));
        let mut min_single = single.gb;
        for _ in 0..120 {
            single.tick(&anim, &flags, w, h, 0);
            min_single = min_single.min(single.gb);
            if single.action == 0 {
                break;
            }
        }
        assert_eq!(min_single, 512, "single jump peak: {min_single}");
        // Double jump: second Up at the first peak
        // (frame durations are ~90-100 ms, so the
        // peak is ~12 ticks in, not 5).
        let mut pl = super::Player::new(&anim, spawn, true, cx, cy);
        assert!(pl.jump(&anim));
        for _ in 0..15 {
            if pl.gb == 512 {
                break;
            }
            pl.tick(&anim, &flags, w, h, 0);
        }
        assert_eq!(pl.gb, 512, "first jump never peaked: gb {}", pl.gb);
        assert_eq!(pl.action, 24);
        assert!(pl.jump(&anim), "double jump refused mid-arc");
        assert_eq!(pl.action, 25);
        assert!(!pl.jump(&anim), "triple jump must fail");
        let mut min_double = pl.gb;
        let mut landed = false;
        for _ in 0..120 {
            pl.tick(&anim, &flags, w, h, 0);
            min_double = min_double.min(pl.gb);
            if pl.action == 0 {
                landed = true;
                break;
            }
        }
        assert!(landed, "double jump never landed: action {}", pl.action);
        assert_eq!(pl.gb, spawn.1, "double jump landing: gb {}", pl.gb);
        assert!(
            min_double <= 470,
            "double jump peak too low: min gb {min_double} (single {min_single})"
        );
    }

    /// No double jump from a fall (action 26): the
    /// original's case-1 handler breaks there.
    #[test]
    fn double_jump_from_fall_fails() {
        let (parsed, _flags, anim) = level_9217();
        let spawn = parsed.group(26).unwrap().positions[0];
        let (cx, cy) = crate::entities::start_camera(&parsed.level, spawn);
        let mut pl = super::Player::new(&anim, spawn, true, cx, cy);
        pl.gb = 400;
        pl.action = 26;
        pl.slot = crate::anim::AnimSlot::new(&anim, 26);
        assert!(!pl.jump(&anim));
        assert_eq!(pl.action, 26);
    }

    /// The 32 px step at tile 44 (ga=690 stall in the
    /// probe) is cleared by a single jump: the player
    /// lands on the step and walks past it.
    #[test]
    fn jump_clears_the_690_step() {
        let (parsed, flags, anim) = level_9217();
        let spawn = parsed.group(26).unwrap().positions[0];
        let (cx, cy) = crate::entities::start_camera(&parsed.level, spawn);
        let mut pl = super::Player::new(&anim, spawn, true, cx, cy);
        pl.ga = 650;
        pl.gb = 560;
        let w = parsed.level.pixel_width() as i32;
        let h = parsed.level.pixel_height() as i32;
        assert!(pl.jump(&anim));
        for _ in 0..120 {
            pl.tick(&anim, &flags, w, h, 1);
        }
        assert!(
            pl.ga > 720,
            "jump did not clear the step: ga {} gb {} action {}",
            pl.ga, pl.gb, pl.action
        );
    }

    /// Slice 20: climb columns (type 5, flag 8) are where
    /// the data puts them — tiles (156, 14..29) and (222,
    /// 10..25) — and NOT at the ga=2161 wall (tile 136),
    /// which is why climbing cannot attach there.
    #[test]
    fn full_flags_climb_columns() {
        let (_parsed, flags, _anim) = level_9217();
        for y in 14..=29 {
            assert!(flags.has(156, y, 8), "no flag 8 at (156, {y})");
        }
        for y in 10..=25 {
            assert!(flags.has(222, y, 8), "no flag 8 at (222, {y})");
        }
        assert!(!flags.has(156, 30, 8), "flag 8 leaks past (156, 29)");
        assert!(!flags.has(222, 26, 8), "flag 8 leaks past (222, 25)");
        for y in 26..=35 {
            assert!(!flags.has(136, y, 8), "wall tile (136, {y}) must not be climbable");
            assert!(flags.solid(136, y), "wall tile (136, {y}) must stay solid");
        }
    }

    /// Slice 20: box footprints (type 27) carry flags 1
    /// and 512; the tall object at tile (188, 33) reaches
    /// 6 tiles up.
    #[test]
    fn full_flags_box_footprints() {
        let (_parsed, flags, _anim) = level_9217();
        assert!(flags.has(128, 33, 512), "box top-left must be flag 512");
        assert!(flags.has(129, 34, 512), "box bottom-right must be flag 512");
        assert!(flags.solid(128, 33), "box tiles must be solid");
        assert!(flags.solid(129, 34), "box tiles must be solid");
        assert!(flags.has(188, 27, 512), "tall object top must be flag 512");
        assert!(flags.has(189, 32, 512), "tall object base row must be flag 512");
        assert!(!flags.has(128, 32, 512), "no flag 512 above the box");
    }

    /// Slice 20: the remaining load-time flag sources.
    /// Positions come from the groups themselves; the
    /// test pins the flag VALUE each source writes.
    #[test]
    fn full_flags_runs_and_singles() {
        let (parsed, flags, _anim) = level_9217();
        // Type 8 triggers: flag 128 on the spawn tile.
        assert!(flags.has(3, 35, 128), "spawn tile must be a trigger (flag 128)");
        // Type 25 checkpoints: flag 1024.
        let g25 = parsed.group(25).unwrap();
        let (x, y) = g25.positions[0];
        assert!(flags.has(x / 16, y / 16, 1024), "type-25 tile must be flag 1024");
        // Type 9 switch column: flag 256.
        let g9 = parsed.group(9).unwrap();
        let (x, y) = g9.positions[0];
        assert!(flags.has(x / 16, y / 16, 256), "type-9 tile must be flag 256");
        assert!(flags.has(x / 16, y / 16 + g9.extra[0] as i32, 256), "type-9 column end must be flag 256");
        // Type 29 run: flag 16384.
        let g29 = parsed.group(29).unwrap();
        let (x, y) = g29.positions[0];
        assert!(flags.has(x / 16, y / 16, 16384), "type-29 tile must be flag 16384");
        // Type 18 run: flag 16. Type 10 run: flag 64.
        let g18 = parsed.group(18).unwrap();
        let (x, y) = g18.positions[0];
        assert!(flags.has(x / 16, y / 16, 16), "type-18 tile must be flag 16");
        let g10 = parsed.group(10).unwrap();
        let (x, y) = g10.positions[0];
        assert!(flags.has(x / 16, y / 16, 64), "type-10 tile must be flag 64");
        // Type 14: vertical run flag 4.
        let g14 = parsed.group(14).unwrap();
        let (x, y) = g14.positions[0];
        assert!(flags.has(x / 16, y / 16, 4), "type-14 tile must be flag 4");
        // Type 28: rectangle flag 32.
        let g28 = parsed.group(28).unwrap();
        let (x, y) = g28.positions[0];
        assert!(flags.has(x / 16, y / 16, 32), "type-28 tile must be flag 32");
        assert!(
            flags.has(x / 16 + g28.extra[0] as i32, y / 16 + g28.extra[1] as i32, 32),
            "type-28 rectangle corner must be flag 32"
        );
        // Type 30: flag 2048 or 4096 by its bit.
        let g30 = parsed.group(30).unwrap();
        for (p, &(x, y)) in g30.positions.iter().enumerate() {
            let want = if g30.extra[p] != 0 { 2048 } else { 4096 };
            assert!(flags.has(x / 16, y / 16, want), "type-30 tile {p} must be flag {want}");
        }
        // Type 35 with cV: the (217, 24) doorway clears
        // flag 1 on its footprint.
        let g35 = parsed.group(35).unwrap();
        let (x, y) = g35.positions[1];
        assert_eq!(g35.extra[1 * 4 + 2], 1, "second type-35 item must be the cV one");
        assert!(!flags.has(x / 16, y / 16, 1), "cV footprint must clear flag 1");
    }

    /// Slice 20's key behaviour change: the box at tile
    /// 128 is solid now, so walking right from ga=1900
    /// stops at its face (ga <= 2048) instead of passing
    /// through it. (Getting past it is slice 22's grab /
    /// push; jumping over it also works — 32 px < 48 px.)
    #[test]
    fn box_footprint_blocks_walk() {
        let (parsed, flags, anim) = level_9217();
        let spawn = parsed.group(26).unwrap().positions[0];
        let (cx, cy) = crate::entities::start_camera(&parsed.level, spawn);
        let mut pl = super::Player::new(&anim, spawn, true, cx, cy);
        pl.ga = 1900;
        pl.gb = 560;
        let w = parsed.level.pixel_width() as i32;
        let h = parsed.level.pixel_height() as i32;
        for _ in 0..200 {
            pl.tick(&anim, &flags, w, h, 1);
        }
        assert!(
            pl.ga > 1900 && pl.ga <= 2048,
            "walk must stop at the box face: ga {}",
            pl.ga
        );
    }

    /// Slice 21 helper: teleport the player to the
    /// temple floor left of the climb column at tile
    /// 156 (x=2496, rows 14..29, flags 1+8) and double
    /// jump at it. The first jump's rise stalls
    /// against the column (blocked, no attach — the
    /// flag-8 check needs gy<=27); the double jump
    /// rises to gy=26/27 and attaches (action 33).
    fn attach_at_column_156() -> (super::Player, super::TileFlags, crate::anim::AnimSet, (i32, i32)) {
        let (parsed, flags, anim) = level_9217();
        let spawn = parsed.group(26).unwrap().positions[0];
        let (cx, cy) = crate::entities::start_camera(&parsed.level, spawn);
        let mut pl = super::Player::new(&anim, spawn, true, cx, cy);
        pl.ga = 2440;
        pl.gb = 528;
        let w = parsed.level.pixel_width() as i32;
        let h = parsed.level.pixel_height() as i32;
        assert!(pl.jump(&anim), "first jump must start");
        for _ in 0..12 {
            pl.tick(&anim, &flags, w, h, 1);
        }
        assert_eq!(pl.action, 24, "still in the first arc");
        assert!(pl.jump(&anim), "double jump must start");
        let mut attached = false;
        for _ in 0..60 {
            pl.tick(&anim, &flags, w, h, 1);
            if pl.action == 33 {
                attached = true;
                break;
            }
        }
        assert!(attached, "never attached: action {} ga {} gb {}", pl.action, pl.ga, pl.gb);
        (pl, flags, anim, (w, h))
    }

    /// Slice 21: double-jumping at the type-5 column
    /// attaches the player to the wall (action 33) and
    /// snaps ga to the wall face (156*16 - hx = 2486).
    #[test]
    fn climb_attach_from_double_jump() {
        let (pl, _flags, _anim, _dims) = attach_at_column_156();
        assert_eq!(pl.action, 33);
        assert_eq!(pl.ga, 156 * 16 - super::PLAYER_HX, "ga {} should snap to the wall face", pl.ga);
        assert!(pl.climbing());
    }

    /// Slice 23: double-jumping from the box top
    /// (gb=528) at the wall (tile 136) grabs the roof
    /// ledge (row 26, top 416): the hang pose (37)
    /// snaps the feet to `416 + gD = 450`. The player
    /// starts on the box pushed right against the
    /// wall (ga=2144, tile 134) — from the box's
    /// original spot (128) an overhead structure at
    /// tile 131 blocks the jump.
    #[test]
    fn ledge_grab_from_double_jump_at_wall() {
        let (_parsed, flags, anim) = level_9217();
        let (w, h) = (3712, 624);
        let mut pl = super::Player::new(&anim, (2144, 528), true, 0, 294);
        assert!(pl.jump(&anim), "first jump refused");
        for _ in 0..12 {
            pl.tick(&anim, &flags, w, h, 1);
        }
        assert!(pl.jump(&anim), "double jump refused: action {}", pl.action);
        let mut grabbed = false;
        for _ in 0..60 {
            pl.tick(&anim, &flags, w, h, 1);
            if pl.action == 37 {
                grabbed = true;
                break;
            }
        }
        assert!(grabbed, "never grabbed the ledge: ga={} gb={} action={}", pl.ga, pl.gb, pl.action);
        assert_eq!(pl.gb, 26 * 16 + super::PLAYER_GD, "hang gb {} want 450", pl.gb);
        assert!(pl.ga < 136 * 16, "ga {} should be left of the wall face", pl.ga);
    }

    /// Slice 23: Up from the hang (37) vaults (38)
    /// onto the roof: the vault nets -gD, so the
    /// player ends standing (0) on the roof top (416).
    #[test]
    fn ledge_mount_vaults_onto_roof() {
        let (_parsed, flags, anim) = level_9217();
        let (w, h) = (3712, 624);
        let mut pl = super::Player::new(&anim, (2144, 528), true, 0, 294);
        pl.jump(&anim);
        for _ in 0..12 {
            pl.tick(&anim, &flags, w, h, 1);
        }
        pl.jump(&anim);
        for _ in 0..60 {
            pl.tick(&anim, &flags, w, h, 1);
            if pl.action == 37 {
                break;
            }
        }
        assert_eq!(pl.action, 37, "setup: never grabbed, action {}", pl.action);
        assert!(pl.jump(&anim), "mount refused from hang");
        assert_eq!(pl.action, 38);
        for _ in 0..40 {
            pl.tick(&anim, &flags, w, h, 0);
            if pl.action == 0 {
                break;
            }
        }
        assert_eq!(pl.action, 0, "vault never finished: action {}", pl.action);
        assert_eq!(pl.gb, 26 * 16, "mount gb {} want roof top 416", pl.gb);
        assert!(pl.ga > 136 * 16, "mount ga {} should be past the wall", pl.ga);
    }

    /// Slice 23: jumping the ga=690 step (a 32 px
    /// step, not a ledge) does NOT grab — the `g()`
    /// beside check blocks it because the approach
    /// side is solid `hg` below the step top.
    #[test]
    fn ledge_grab_does_not_fire_on_step() {
        let (_parsed, flags, anim) = level_9217();
        let (w, h) = (3712, 624);
        let mut pl = super::Player::new(&anim, (650, 560), true, 0, 294);
        assert!(pl.jump(&anim), "jump refused");
        let mut grabbed = false;
        for _ in 0..60 {
            pl.tick(&anim, &flags, w, h, 1);
            if pl.action == 37 {
                grabbed = true;
                break;
            }
            if pl.action == 0 && pl.ga > 720 {
                break;
            }
        }
        assert!(!grabbed, "step grabbed as a ledge at ga={} gb={}", pl.ga, pl.gb);
        assert!(pl.ga > 720, "step not cleared: ga={}", pl.ga);
    }

    /// Slice 23: climbing to the top of column 156
    /// then pressing Up mounts (38) instead of
    /// another climb cycle — the `gu` top-out.
    #[test]
    fn climb_topout_mounts() {
        let (mut pl, flags, anim, (w, h)) = attach_at_column_156();
        // Climb up until the wall runs out above
        // (climbing_top set by the 35 wall-out).
        let mut topped = false;
        for _ in 0..40 {
            if pl.climbing_top {
                topped = true;
                break;
            }
            if !pl.climb_up(&anim, &flags) {
                break;
            }
            for _ in 0..30 {
                pl.tick(&anim, &flags, w, h, 0);
                if pl.action == 33 {
                    break;
                }
            }
        }
        assert!(topped, "never reached the column top: gb={} action={}", pl.gb, pl.action);
        assert_eq!(pl.action, 33);
        assert!(pl.climb_up(&anim, &flags), "top-out mount refused");
        assert_eq!(pl.action, 38, "top-out should vault (38), got {}", pl.action);
        for _ in 0..40 {
            pl.tick(&anim, &flags, w, h, 0);
            if pl.action == 0 {
                break;
            }
        }
        assert_eq!(pl.action, 0, "top-out vault never finished: action {}", pl.action);
    }

    /// Slice 21: one climb-up cycle (action 35) lifts
    /// the player 19 px, then returns to the hold pose.
    #[test]
    fn climb_up_moves_up_19px() {
        let (mut pl, flags, anim, (w, h)) = attach_at_column_156();
        let start_gb = pl.gb;
        assert!(pl.climb_up(&anim, &flags), "climb up refused");
        assert_eq!(pl.action, 35);
        let mut held = false;
        for _ in 0..30 {
            pl.tick(&anim, &flags, w, h, 0);
            if pl.action == 33 {
                held = true;
                break;
            }
        }
        assert!(held, "climb-up cycle never returned to hold: action {}", pl.action);
        assert_eq!(pl.gb, start_gb - 19, "climb-up moved {} px, want -19", pl.gb - start_gb);
    }

    /// Slice 21: one climb-down cycle (action 34) lowers
    /// the player 20 px, then returns to the hold pose.
    #[test]
    fn climb_down_moves_down_20px() {
        let (mut pl, flags, anim, (w, h)) = attach_at_column_156();
        let start_gb = pl.gb;
        assert!(pl.climb_down(&anim, &flags), "climb down refused");
        assert_eq!(pl.action, 34);
        let mut held = false;
        for _ in 0..30 {
            pl.tick(&anim, &flags, w, h, 0);
            if pl.action == 33 {
                held = true;
                break;
            }
        }
        assert!(held, "climb-down cycle never returned to hold: action {}", pl.action);
        assert_eq!(pl.gb, start_gb + 20, "climb-down moved {} px, want +20", pl.gb - start_gb);
    }

    /// Slice 21: climbing down off the bottom of a
    /// wall drops the player (action 26) instead of
    /// climbing.
    #[test]
    fn climb_down_off_the_bottom_drops() {
        let (mut pl, flags, anim, _dims) = attach_at_column_156();
        // Walk the player to the column's bottom row
        // (gy=29) by climbing down repeatedly.
        for _ in 0..30 {
            if pl.action == 33 {
                if !pl.climb_down(&anim, &flags) {
                    break;
                }
            }
            let w = 3712;
            let h = 624;
            pl.tick(&anim, &flags, w, h, 0);
            if pl.action == 26 {
                break;
            }
        }
        assert_eq!(pl.action, 26, "climbing off the bottom must drop: action {}", pl.action);
    }

    /// Slice 21: pressing Left while facing right on
    /// the wall turns the player (action 44) and flips
    /// the facing; the turn ends back in the hold pose.
    #[test]
    fn climb_turn_flips_facing() {
        let (mut pl, flags, anim, (w, h)) = attach_at_column_156();
        assert!(pl.facing_right);
        assert!(pl.climb_turn(&anim, -1), "turn refused");
        assert_eq!(pl.action, 44);
        assert!(!pl.facing_right);
        assert!(!pl.climb_turn(&anim, -1), "same-direction turn must be refused");
        let mut held = false;
        for _ in 0..30 {
            pl.tick(&anim, &flags, w, h, 0);
            if pl.action == 33 {
                held = true;
                break;
            }
        }
        assert!(held, "turn never returned to hold: action {}", pl.action);
        assert!(!pl.facing_right, "facing must stay left after the turn");
    }

    /// Slice 21: fire on the wall unequips the weapon
    /// and plays the climb attack (36), then holds.
    #[test]
    fn climb_attack_plays_36_and_unequips() {
        let (mut pl, flags, anim, (w, h)) = attach_at_column_156();
        pl.weapon_mode = 1;
        assert!(pl.attack(&anim), "climb attack refused");
        assert_eq!(pl.action, 36);
        assert_eq!(pl.weapon_mode, 0, "climb attack must unequip (A())");
        let mut held = false;
        for _ in 0..40 {
            pl.tick(&anim, &flags, w, h, 0);
            if pl.action == 33 {
                held = true;
                break;
            }
        }
        assert!(held, "climb attack never returned to hold: action {}", pl.action);
    }

    /// Slice 21: the climb-up gate — when the tile
    /// above the player's top is solid and not
    /// climbable, Up is refused; when it is empty
    /// (or climbable), Up is allowed. Synthetic
    /// flags: wall column x=5 (flag 8, rows 4..9),
    /// solid ceiling at (5, 3).
    #[test]
    fn climb_up_blocked_by_solid_ceiling() {
        let (_parsed, _flags, anim) = level_9217();
        let mut raw = vec![0i16; 100];
        // Wall column (5, 4..9): flag 8. Ceiling (5, 3): flag 1.
        for y in 4..=9 {
            raw[5 + 10 * y] = 8;
        }
        raw[5 + 10 * 3] = 1;
        let flags = super::TileFlags { flags: raw, width: 10, height: 10 };
        let mut pl = super::Player::new(&anim, (72, 111), true, 0, 0);
        // ga=72 (tile 4), gb=111 (gy=6): the above tile
        // is (5, 6-2-1=3), solid and not climbable.
        pl.action = 33;
        pl.slot = crate::anim::AnimSlot::new(&anim, 33);
        assert!(!pl.climb_up(&anim, &flags), "solid ceiling must block climb-up");
        assert_eq!(pl.action, 33);
        // Remove the ceiling: the same press is allowed.
        let mut raw2 = vec![0i16; 100];
        for y in 4..=9 {
            raw2[5 + 10 * y] = 8;
        }
        let flags2 = super::TileFlags { flags: raw2, width: 10, height: 10 };
        assert!(pl.climb_up(&anim, &flags2), "open sky must allow climb-up");
        assert_eq!(pl.action, 35);
    }

    /// Player bbox constants (slice 21): action 0 is
    /// 42x34, so gC = 21, gD = 34, hg = 34/16 = 2,
    /// hx = 21>>1 = 10. Pins the climb snap geometry.
    #[test]
    fn player_bbox_constants() {
        let mut res = crate::resources::Resources::new("../extracted");
        let anim_chunk = res.chunk(28672).unwrap();
        let anim = crate::anim::load_anim_set(&anim_chunk).unwrap();
        let sprites = res.sprite_set(28673, None).unwrap();
        let (w, h) = crate::entities::action_bbox(&anim, &sprites, 0);
        assert_eq!((w, h), (42, 34));
        assert_eq!(super::PLAYER_HX, w / 2 / 2);
        assert_eq!(super::PLAYER_HG, h / 16);
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

    /// Slice 22: Up while holding a box (action 21)
    /// jumps out of the grab (action 24) and clears
    /// the grabbed box.
    #[test]
    fn jump_out_of_grab() {
        let (_parsed, _flags, anim) = level_9217();
        let spawn = (48, 560);
        let mut pl = super::Player::new(&anim, spawn, true, 0, 0);
        pl.action = 21;
        pl.slot = crate::anim::AnimSlot::new(&anim, 21);
        pl.grabbed_box = Some(0);
        assert!(pl.jump(&anim), "jump out of grab refused");
        assert_eq!(pl.action, 24);
        assert_eq!(pl.grabbed_box, None);
    }

    #[test]
    fn hurt_reaction_picks_7_5_or_100() {
        // Phase 6 (e.java:5378-5382): damage > 50
        // turns to face the attacker and plays 7;
        // already facing the attacker plays 5;
        // facing away plays 100.
        let (_parsed, _flags, anim) = level_9217();
        let spawn = (48, 560);
        // Big hurt: facing right, attacker to the
        // right (gf = true) -> 7, facing unchanged.
        let mut pl = super::Player::new(&anim, spawn, true, 0, 0);
        pl.hurt_reaction(&anim, 60, true);
        assert_eq!(pl.action, 7);
        assert!(pl.facing_right);
        // Small hurt, facing the attacker -> 5.
        let mut pl = super::Player::new(&anim, spawn, true, 0, 0);
        pl.hurt_reaction(&anim, 10, true);
        assert_eq!(pl.action, 5);
        // Small hurt, facing away -> 100.
        let mut pl = super::Player::new(&anim, spawn, true, 0, 0);
        pl.hurt_reaction(&anim, 10, false);
        assert_eq!(pl.action, 100);
        // Big hurt from behind turns the player
        // to face the attacker.
        let mut pl = super::Player::new(&anim, spawn, true, 0, 0);
        pl.hurt_reaction(&anim, 60, false);
        assert_eq!(pl.action, 7);
        assert!(!pl.facing_right);
    }

    #[test]
    fn artemis_variants_map_while_action_stays_logical() {
        // Phase 8 (e.java `s()`): with the Blade of
        // Artemis (weapon_mode 1) the anim plays a
        // variant while `player.action` stays the
        // logical action. Blades (mode 0) play as-is.
        let a = super::Player::anim_action;
        assert_eq!(a(27, 0), 27);
        assert_eq!(a(3, 0), 3);
        assert_eq!(a(97, 0), 97);
        assert_eq!(a(27, 1), 29);
        assert_eq!(a(3, 1), 4);
        assert_eq!(a(97, 1), 98);
        assert_eq!(a(5, 1), 6);
        assert_eq!(a(8, 1), 9);
        assert_eq!(a(100, 1), 101);
        assert_eq!(a(38, 1), 39);
        assert_eq!(a(0, 1), 0);
        assert_eq!(a(60, 1), 60);
        // The slot uses the variant: block enter
        // with Artemis plays 29, action stays 27.
        let (_parsed, _flags, anim) = level_9217();
        let mut pl = super::Player::new(&anim, (48, 560), true, 0, 0);
        pl.weapon_mode = 1;
        assert!(pl.start_block(&anim));
        assert_eq!(pl.action, 27);
        assert_eq!(pl.slot.action, 29);
    }

    #[test]
    fn block_poses_27_then_3_then_28() {
        // Phase 6: start_block enters with 27;
        // 27 ends in the hold pose 3 while Down
        // is held; stop_block plays the exit
        // pose 28, which ends in idle.
        let (_parsed, flags, anim) = level_9217();
        let w = 232 * 16;
        let h = 39 * 16;
        let spawn = (48, 560);
        let mut pl = super::Player::new(&anim, spawn, true, 0, 0);
        assert!(pl.start_block(&anim));
        assert_eq!(pl.action, 27);
        assert!(pl.blocking);
        // 27 takes 2 ticks; the hold pose follows.
        pl.tick(&anim, &flags, w, h, 0);
        pl.tick(&anim, &flags, w, h, 0);
        assert_eq!(pl.action, 3, "27 should end in the hold pose 3");
        // Release: 28, then idle.
        pl.stop_block(&anim);
        assert_eq!(pl.action, 28);
        assert!(!pl.blocking);
        pl.tick(&anim, &flags, w, h, 0);
        pl.tick(&anim, &flags, w, h, 0);
        assert_eq!(pl.action, 0, "28 should end in idle");
    }
}









