//! Gameplay engine shared by `--play` and `--game` state 1
//! (wiring, 2026-11-04). Wraps the slice-3 modules: level
//! tiles, static sprites, player walk/camera, enemies,
//! combat (3d-3) and HUD (3e).
//!
//! `Gameplay::load` mirrors the `--play` setup: parse the
//! level chunk, build tile flags, spawn the player at the
//! type-26 spawn with the start camera, and spawn enemies.
//! `tick` is one 40 ms frame. `draw` renders tiles +
//! statics + enemies + player + HUD, in `d(Graphics)` order.

use crate::anim::{self, AnimSet};
use crate::boxes::{self, GameBox};
use crate::doors::{self, Door, Lever, Switch};
use crate::enemies::{self, EnemyState};
use crate::entities::{self, LevelEntities};
use crate::font::Font;
use crate::hud;
use crate::level;
use crate::midp::{self, Framebuffer, Graphics};
use crate::player::{Player, TileFlags};
use crate::resources::Resources;
use crate::resources::SpriteSet;
use crate::script::{ScriptEngine, ScriptEvent, ScriptTrigger};

/// Level ids in menu order (`bB` in e.java).
pub const LEVEL_IDS: [u32; 10] = [
    9217, 19456, 20480, 21504, 22528, 23552, 24576, 25600, 26624, 27648,
];
pub const ARENA_ID: u32 = 9216;
/// Palette-key selector `bG` in e.java, indexed by level index.
/// Tileset key = 2068 + bG[i], static-sprite key = 3084 + bG[i],
/// when bG[i] >= 0, else no key (base palette).
pub const BG: [i32; 10] = [-1, -1, -1, 2, -1, 1, 1, -1, 2, 2];

pub fn level_index(level_id: u32) -> usize {
    LEVEL_IDS.iter().position(|&id| id == level_id).unwrap_or(0)
}

pub fn palette_key(base: u32, level_index: usize) -> Option<u32> {
    let k = BG.get(level_index).copied().unwrap_or(-1);
    if k >= 0 {
        Some(base + k as u32)
    } else {
        None
    }
}

pub struct Gameplay {
    pub parsed: LevelEntities,
    pub flags: TileFlags,
    pub player: Player,
    pub player_anim: AnimSet,
    pub player_sprites: SpriteSet,
    pub enemy_states: Vec<EnemyState>,
    pub combo_hits: i32,
    combo_timer_ms: i32,
    pub arena: bool,
    pub level_index: usize,
    /// Orb pickups (slice 12) and the totals they feed.
    pub pickups: crate::pickups::Pickups,
    pub gold: i32,
    pub magic: i32,
    rng: crate::pickups::OrbRng,
    /// Script triggers and playback (slice 13).
    pub script: ScriptEngine,
    /// Set when a script's lone `*` fires: the index
    /// of the next level (level_index + 1; 10 means
    /// all levels done). The caller (Game, --play)
    /// consumes it and advances or returns to menu.
    pub level_complete: Option<usize>,
    /// Weakened-marker animation (slice 14): anim
    /// set 1027, sprite set 1040 (`aU`), slot for
    /// action 10, advanced once per tick. None when
    /// either set fails to load (markers disabled).
    bar: Option<BarState>,
    /// Boss bar (Phase 2): anim set 1027, sprite
    /// set 1040 (`aU`), slot 131. The slot is
    /// (re)set to the latched action `aT` when an
    /// `@` trigger latches; it advances (t(131))
    /// each tick while latched. None when either
    /// set fails to load (bar disabled).
    boss_bar: Option<BarState>,
    /// Level intro dialog (slice 15): the level
    /// header's Q string index, when Q > 0 (only
    /// 9217, 23552, 27648 have one). The text is
    /// resolved from the string table at load and
    /// re-resolved when the language changes.
    intro_index: Option<usize>,
    /// Intro text while it is showing; None once
    /// dismissed (or when the level has no intro).
    pub intro_text: Option<String>,
    /// Down held this frame (slice 18): set by
    /// Game from its held-down state before each
    /// tick; drives the block stance below.
    pub block_held: bool,
    /// Type-27 boxes (slice 22) and their anim /
    /// sprite sets (6144 / 6146).
    pub boxes: Vec<GameBox>,
    box_anim: AnimSet,
    box_sprites: SpriteSet,
    /// Type-9 switches and type-35 doors (slice 23):
    /// the switch columns toggle doors open/closed.
    pub switches: Vec<Switch>,
    pub doors: Vec<Door>,
    /// Type-34 levers (slice 23): Down near one
    /// toggles its doors (the (206,32) lever opens
    /// the pillar door before the level end).
    pub levers: Vec<Lever>,
    /// Down held on the previous tick (slice 23):
    /// a lever pull fires only on the press edge
    /// (block_held false -> true).
    prev_block_held: bool,
    /// Held direction on the previous tick (slice 22):
    /// a push/pull starts only on a fresh press edge
    /// (dir != prev_dir) while grabbing.
    prev_dir: i32,
}

/// Marker slot state (slice 14).
struct BarState {
    anim: AnimSet,
    sprites: SpriteSet,
    slot: crate::anim::AnimSlot,
}

impl Gameplay {
    pub fn load(res: &mut Resources, level_id: u32, arena: bool) -> Result<Gameplay, String> {
        let chunk = res.chunk(level_id).map_err(|e| format!("{e:?}"))?;
        let parsed = entities::parse_level_entities(level_id, &chunk).map_err(|e| e.to_string())?;
        let footprints = entities::load_footprints(res, &parsed)?;
        let flags = TileFlags::build(&parsed, &footprints);
        let player_anim_chunk = res.chunk(28672).map_err(|e| format!("{e:?}"))?;
        let player_anim = anim::load_anim_set(&player_anim_chunk).map_err(|e| e.to_string())?;
        let player_sprites = res.sprite_set(28673, None).map_err(|e| format!("{e:?}"))?;
        let spawn = parsed
            .group(26)
            .and_then(|g| g.positions.first().copied())
            .ok_or("no player spawn (type 26)")?;
        let (start_cam_x, start_cam_y) = entities::start_camera(&parsed.level, spawn);
        let mut player = Player::new(&player_anim, spawn, parsed.level.facing_right, start_cam_x, start_cam_y);
        player.health = 1024;
        player.max_health = 1024;
        // Type-3 camera zones (slice 23).
        player.set_camera_zones(entities::camera_zones(&parsed));
        let mut enemy_states = Vec::new();
        for e in enemies::enemies(&parsed) {
            if let Ok(anim_chunk) = res.chunk(e.anim_id) {
                if let Ok(anim_set) = anim::load_anim_set(&anim_chunk) {
                    let mut st = EnemyState::new(e.clone(), &anim_set);
                    // ef from the spawn-time bbox
                    // (initial action, timeline 0).
                    if let Ok(sprites) = res.sprite_set(e.sprite_id, None) {
                        st.ef = enemies::enemy_bbox_height(&anim_set, &sprites, e.action);
                    }
                    enemy_states.push(st);
                }
            }
        }
        // Weakened-marker sets (slice 14).
        let bar = match (
            res.chunk(1027).ok().and_then(|c| anim::load_anim_set(&c).ok()),
            res.sprite_set(1040, None).ok(),
        ) {
            (Some(anim_set), Some(sprites)) => {
                let slot = crate::anim::AnimSlot::new(&anim_set, 10);
                Some(BarState {
                    anim: anim_set,
                    sprites,
                    slot,
                })
            }
            _ => None,
        };
        // Boss-bar sets (Phase 2): same anim set
        // 1027 + sprite set 1040 as the marker, but
        // slot 131 with the latched action (set on
        // latch; initialised to action 2).
        let boss_bar = match (
            res.chunk(1027).ok().and_then(|c| anim::load_anim_set(&c).ok()),
            res.sprite_set(1040, None).ok(),
        ) {
            (Some(anim_set), Some(sprites)) => {
                let slot = crate::anim::AnimSlot::new(&anim_set, 2);
                Some(BarState {
                    anim: anim_set,
                    sprites,
                    slot,
                })
            }
            _ => None,
        };
        // Type-27 boxes (slice 22): anim set 6144 +
        // sprite set 6146 with the level palette key
        // (6150 + bG[i]).
        let level_idx = level_index(level_id);
        let box_anim_chunk = res.chunk(boxes::BOX_ANIM_SET).map_err(|e| format!("{e:?}"))?;
        let box_anim = anim::load_anim_set(&box_anim_chunk).map_err(|e| e.to_string())?;
        let box_sprites = res
            .sprite_set(
                boxes::BOX_SPRITE_SET,
                palette_key(boxes::BOX_SPRITE_KEY_BASE, level_idx),
            )
            .map_err(|e| format!("{e:?}"))?;
        let boxes = boxes::load_boxes(
            res,
            &parsed,
            palette_key(boxes::BOX_SPRITE_KEY_BASE, level_idx),
        )?;
        // Type-9 switches and type-35 doors (slice 23).
        let switches = doors::load_switches(&parsed);
        let doors = doors::load_doors(
            res,
            &parsed,
            palette_key(2062, level_idx),
        )?;
        // Type-34 levers (slice 23).
        let levers = doors::load_levers(&parsed);
        let mut triggers = Vec::new();
        if let Some(g8) = parsed.group(8) {
            for (i, &(px, py)) in g8.positions.iter().enumerate() {
                let cn = g8.extra.get(i * 2).copied().unwrap_or(0) as usize;
                let co = g8.extra.get(i * 2 + 1).copied().unwrap_or(0) as usize;
                triggers.push(ScriptTrigger {
                    tile_x: px / 16,
                    tile_y: py / 16,
                    start: cn,
                    len: co,
                });
            }
        }
        // English strings by default; Game overwrites
        // with the selected language after load.
        let strings = res.strings(10240).unwrap_or_default();
        // Level intro: Q > 0 means the level opens
        // with dialog string Q (state 1 -> 107 in the
        // original; Q <= 0 goes straight to gameplay).
        let intro_index = if parsed.level.q > 0 {
            Some(parsed.level.q as usize)
        } else {
            None
        };
        let intro_text = intro_index.and_then(|i| strings.get(i).cloned());
        Ok(Gameplay {
            parsed,
            flags,
            player,
            player_anim,
            player_sprites,
            enemy_states,
            combo_hits: 0,
            combo_timer_ms: 0,
            arena,
            level_index: level_index(level_id),
            pickups: crate::pickups::Pickups::default(),
            gold: 0,
            magic: 0,
            // Time-based seed; exact scatter does not
            // matter for gameplay, only for tests (which
            // seed their own rng).
            rng: crate::pickups::OrbRng(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.subsec_nanos() ^ (d.as_secs() as u32))
                    .unwrap_or(0x9E3779B9),
            ),
            script: ScriptEngine::new(triggers, strings),
            level_complete: None,
            bar,
            boss_bar,
            intro_index,
            intro_text,
            block_held: false,
            boxes,
            box_anim,
            box_sprites,
            switches,
            doors,
            levers,
            prev_block_held: false,
            prev_dir: 0,
        })
    }

    /// Re-resolve all string-table text after a
    /// language change: script strings and the
    /// intro dialog text (slice 15).
    pub fn set_strings(&mut self, strings: Vec<String>) {
        self.intro_text = self.intro_index.and_then(|i| strings.get(i).cloned());
        self.script.set_strings(strings);
    }

    /// True while the level intro dialog is showing.
    pub fn intro_active(&self) -> bool {
        self.intro_text.is_some()
    }

    /// Dismiss the intro dialog (Select/Exit key):
    /// gameplay starts on the next tick.
    pub fn dismiss_intro(&mut self) {
        self.intro_text = None;
    }

    /// Apply pickup events from a tick: gold/health/
    /// magic totals, and damage-orbs hitting enemies
    /// (which can kill and drop in turn).
    fn apply_pickup_events(&mut self, events: Vec<crate::pickups::PickupEvent>, res: &mut Resources) {
        for ev in events {
            match ev {
                crate::pickups::PickupEvent::Collected { kind, value } => match kind {
                    crate::pickups::GOLD => self.gold += value,
                    crate::pickups::HEALTH => {
                        self.player.health =
                            (self.player.health + value).min(self.player.max_health);
                    }
                    crate::pickups::MAGIC => {
                        self.magic = (self.magic + value).min(1024);
                    }
                    _ => {}
                },
                crate::pickups::PickupEvent::HitEnemy { index, value } => {
                    // Scope the enemy borrow so the drop
                    // spawn below can borrow self.
                    let killed_drop = {
                        let Some(st) = self.enemy_states.get_mut(index) else {
                            continue;
                        };
                        if st.dead || st.dying {
                            continue;
                        }
                        let killed = if let Ok(anim_chunk) = res.chunk(st.enemy.anim_id) {
                            if let Ok(anim_set) = anim::load_anim_set(&anim_chunk) {
                                st.apply_damage(&anim_set, value)
                            } else {
                                false
                            }
                        } else {
                            false
                        };
                        if killed {
                            Some((st.x, st.y, st.enemy.kind, st.enemy.em))
                        } else {
                            None
                        }
                    };
                    if let Some((x, y, kind, em)) = killed_drop {
                        let drop_kind = match kind {
                            12 => crate::pickups::MAGIC,
                            _ => crate::pickups::GOLD,
                        };
                        self.pickups.spawn(x, y - 16, drop_kind, em, &mut self.rng);
                    }
                }
            }
        }
    }

    /// Start a jump (Up key) if the player is grounded.
    /// The caller advances a frame afterwards, as for walk.
    /// Up pressed (slice 21): while holding on a wall
    /// (action 33) it climbs up one cycle; otherwise
    /// it jumps (or double-jumps, slice 19).
    pub fn jump(&mut self) -> bool {
        if self.player.action == 33 {
            return self.player.climb_up(&self.player_anim, &self.flags);
        }
        self.player.jump(&self.player_anim)
    }

    /// Advance the active script one step (Select key).
    /// A `*` step sets `level_complete` for the caller.
    pub fn script_advance(&mut self) {
        let ev = self.script.advance();
        self.handle_script_event(ev);
        self.apply_weapon_switch();
    }

    /// Apply a weapon switch recorded by the most
    /// recently shown `^` step (slice 17).
    fn apply_weapon_switch(&mut self) {
        if let Some(mode) = self.script.take_weapon_switch() {
            self.player.weapon_mode = mode;
        }
    }

    /// Cycle weapons (slice 17): left softkey in
    /// gameplay (Select when no dialog shows).
    pub fn cycle_weapon(&mut self) -> bool {
        self.player.cycle_weapon()
    }

    /// Start a box push/pull (slice 22): the player is
    /// grabbing (21) and pressed Left/Right. Pressing
    /// the facing direction pushes (22, gl = 1),
    /// pressing the opposite pulls (23, gl = 0). `u()`
    /// runs at once; on failure the player returns to
    /// the grab pose.
    fn start_box_push(&mut self, dir: i32) {
        let Some(gd) = self.player.grabbed_box else {
            return;
        };
        let push = (dir > 0) == self.player.facing_right;
        let action = if push { 22 } else { 23 };
        self.player.push_pull = if push { 1 } else { 0 };
        self.player.action = action;
        self.player.slot = anim::AnimSlot::new(&self.player_anim, action);
        let (gw, gx) = (self.player.tile_x(), self.player.walk_tile_y());
        let facing = self.player.facing_right;
        if !boxes::try_push(
            &mut self.boxes,
            gd,
            &mut self.flags,
            facing,
            action,
            gw,
            gx,
            &self.box_anim,
        ) {
            self.player.action = 21;
            self.player.slot = anim::AnimSlot::new(&self.player_anim, 21);
        }
    }

    /// Cancel the active script (Exit key).
    pub fn script_cancel(&mut self) {
        self.script.cancel();
    }

    fn handle_script_event(&mut self, ev: ScriptEvent) {
        if ev == ScriptEvent::LevelComplete {
            self.level_complete = Some(self.level_index + 1);
        }
        if ev == ScriptEvent::BossBarLatched {
            // Phase 2: an `@` trigger latched — (re)set
            // the boss-bar slot (131) to the latched
            // action `aT` (fresh slot, like `a(131,
            // 1027, aT)`).
            if let Some(bb) = &mut self.boss_bar {
                let action = self.script.boss_action();
                bb.slot = crate::anim::AnimSlot::new(&bb.anim, action as usize);
            }
        }
    }

    /// Mode-3 burst fire (Phase 3, `hd`): gate is
    /// magic > 256 and fewer than 10 active damage
    /// orbs (`fG < 10`). On fire: action 95, spawn
    /// type-3 orbs totalling `fL[gW]` from
    /// `(ga, gb - gD + gD/4)`, flat 256 magic cost,
    /// reset to blades (`A()`). On a failed gate:
    /// reset to blades and take a normal swing
    /// (the original's fall-through to `s(54)`).
    fn try_fire_mode3(&mut self) {
        if self.magic > crate::player::Player::MODE3_COST
            && self.pickups.damage_orb_count() < 10
        {
            self.player.attack(&self.player_anim);
            self.spawn_mode3_orbs();
            self.magic = (self.magic - crate::player::Player::MODE3_COST).max(0);
            self.player.unequip_all();
        } else {
            self.player.unequip_all();
            self.player.attack(&self.player_anim);
        }
    }

    /// Spawn the mode-3 damage orbs (Phase 3):
    /// `b(ga, gb - gD + gD/4, 3, fL[gW])`. The
    /// orbs round-robin their targets over the
    /// live enemies in the viewport (the
    /// original's `P[]`), like `b()` does.
    fn spawn_mode3_orbs(&mut self) {
        let g_w = self.player.magic_level_mode3.clamp(0, 4) as usize;
        let value = crate::player::Player::F_L[g_w];
        let x = self.player.ga;
        let y = self.player.gb - 34 + 8;
        // Viewport enemies (the original's rect
        // is iQ-64, iR-64, 368x448 around the
        // camera; the port uses the camera rect
        // expanded by 64 px).
        let cam_x = self.player.cam_x;
        let cam_y = self.player.cam_y;
        let targets: Vec<usize> = self
            .enemy_states
            .iter()
            .enumerate()
            .filter(|(_, st)| {
                !st.dead
                    && st.x >= cam_x - 64
                    && st.x < cam_x + 240 + 64
                    && st.y >= cam_y - 64
                    && st.y < cam_y + 320 + 64
            })
            .map(|(i, _)| i)
            .collect();
        self.pickups
            .spawn(x, y, crate::pickups::DAMAGE, value, &mut self.rng);
        self.pickups.assign_damage_targets(&targets);
    }

    /// One 40 ms tick. `dir`: -1 left, 0 none, +1 right.
    /// `attack`: fire key pressed this frame.
    pub fn tick(&mut self, res: &mut Resources, dir: i32, attack: bool) {
        // A showing intro dialog freezes the world
        // (the original's state 1 -> 107 path shows
        // the dialog before gameplay state 100).
        if self.intro_text.is_some() {
            return;
        }
        // A showing script freezes the world (the
        // original's dialog states do not run z()).
        if self.script.active() {
            return;
        }
        // Climb turn (slice 21): Left/Right pressed
        // opposite to the facing while holding (33)
        // flips the player around the wall (44).
        if self.player.action == 33 && dir != 0 {
            self.player.climb_turn(&self.player_anim, dir);
        }
        // Block stance (slice 18): Down held starts
        // it from idle/walk (or resumes it after a
        // blocked-hit reaction ends in idle);
        // releasing Down ends it. Attacking or
        // airborne presses do not start it.
        // Down held while holding on a wall (slice
        // 21) climbs down (34) or drops (26) instead.
        if self.block_held {
            // Lever pull (slice 23): Down pressed near
            // a type-34 lever toggles its doors. Fires
            // only on the press edge, while grounded.
            let down_edge = !self.prev_block_held;
            if down_edge && (self.player.action == 0 || self.player.action == 9) {
                if let Some(lx) = doors::check_lever(
                    &mut self.levers,
                    &mut self.doors,
                    &mut self.flags,
                    self.player.ga,
                    self.player.gb,
                ) {
                    self.player.ga = lx;
                }
            }
            if self.player.action == 37 {
                // Drop from a ledge hang (slice 23):
                // Down while hanging (37) lets go (26).
                self.player.drop_from_hang(&self.player_anim);
            } else if self.player.action == 33 {
                self.player.climb_down(&self.player_anim, &self.flags);
            } else if self.player.drop_through(&self.player_anim, &self.flags) {
                // Drop through a one-way platform
                // (slice 23): Down on a flag-8192 tile
                // falls through instead of blocking.
            } else if !self.player.blocking || self.player.action == 0 {
                self.player.start_block(&self.player_anim);
            }
        } else if self.player.blocking {
            self.player.stop_block(&self.player_anim);
        }
        self.prev_block_held = self.block_held;
        if attack && !self.player.blocking {
            // `A()` (slice 17): Artemis with no magic
            // left unequips instead of swinging.
            if self.player.is_artemis() && self.magic <= 0 {
                self.player.unequip_all();
            } else if self.player.is_magic_mode3() {
                // Phase 3: burst magic (hd). Gate:
                // magic > 256 and fewer than 10 active
                // damage orbs (fG < 10).
                self.try_fire_mode3();
            } else if self.player.is_magic_mode2() {
                // Phase 3: channeled magic (hc). Gate:
                // magic > (gV+1)*4. Starts action 94;
                // the channel drains below.
                if self.magic > self.player.mode2_start_gate() {
                    self.player.attack(&self.player_anim);
                }
            } else {
                self.player.attack(&self.player_anim);
            }
        }
        // Mode-2 channel (Phase 3): while in action
        // 94, drain 12 magic per tick while attack
        // is pressed; end the cast (idle + blades)
        // when attack is released or magic runs out.
        // Approximation: the port's attack is a
        // per-tick press, not a held key, so the
        // channel continues only while attack presses
        // keep arriving each tick.
        if self.player.action == 94 {
            if attack && self.magic > 0 {
                self.magic = (self.magic
                    - crate::player::Player::MODE2_DRAIN_PER_TICK)
                    .max(0);
            } else {
                self.player.end_magic_cast(&self.player_anim);
            }
        }
        if self.combo_timer_ms > 0 {
            self.combo_timer_ms -= 40;
            if self.combo_timer_ms <= 0 {
                self.combo_hits = 0;
            }
        }
        // Enemies tick first (as in --play), then player attack hit.
        let (px, py) = (self.player.ga, self.player.gb);
        for st in &mut self.enemy_states {
            if let Ok(anim_chunk) = res.chunk(st.enemy.anim_id) {
                if let Ok(anim_set) = anim::load_anim_set(&anim_chunk) {
                    let dmg = st.tick(&anim_set, &self.flags, px, py);
                    if dmg > 0 {
                        if self.player.blocking {
                            // Blocked hit (slice 18):
                            // no damage; play the
                            // block-impact reaction
                            // (action 97) facing the
                            // attacker, as in e.java
                            // (`gi = true`, then
                            // `hu = gf; s(97)`).
                            self.player.facing_right = !st.facing_right;
                            self.player.action = 97;
                            self.player.slot =
                                anim::AnimSlot::new(&self.player_anim, 97);
                        } else {
                            self.player.health -= dmg;
                        }
                    }
                }
            }
        }
        // Collect death drops first: the hit loop holds
        // a borrow of enemy_states, so drops spawn after.
        let mut killed_drops: Vec<(i32, i32, u32, i32)> = Vec::new();
        // Hit once per swing, at frame >= 2, for any
        // ground or air swing (slice 16; the original's
        // ht[] hitboxes are still approximated by the
        // front-32px rule below).
        if (crate::player::Player::is_ground_attack(self.player.action)
            || crate::player::Player::is_air_attack(self.player.action))
            && self.player.slot.frame >= 2
            && !self.player.attack_hit_done
        {
            self.player.attack_hit_done = true;
            let swing_damage = self.player.swing_damage;
            for st in &mut self.enemy_states {
                if st.dead || st.dying {
                    continue;
                }
                let in_front = if self.player.facing_right {
                    st.x - self.player.ga >= 0 && st.x - self.player.ga <= 32
                } else {
                    self.player.ga - st.x >= 0 && self.player.ga - st.x <= 32
                };
                if in_front && (st.y - self.player.gb).abs() < 16 {
                    let killed = if let Ok(anim_chunk) = res.chunk(st.enemy.anim_id) {
                        if let Ok(anim_set) = anim::load_anim_set(&anim_chunk) {
                            st.apply_damage(&anim_set, swing_damage)
                        } else {
                            false
                        }
                    } else {
                        false
                    };
                    self.combo_hits += 1;
                    self.combo_timer_ms = 2000;
                    // Artemis drains magic per hit
                    // (slice 17): `fJ * 3`, clamped
                    // at 0 (the original's `hl` can go
                    // negative; the HUD cannot draw it).
                    if self.player.is_artemis() {
                        self.magic =
                            (self.magic - swing_damage * 3).max(0);
                    }
                    if killed {
                        killed_drops.push((st.x, st.y, st.enemy.kind, st.enemy.em));
                    }
                }
            }
        }
        for (x, y, kind, em) in killed_drops {
            let drop_kind = match kind {
                12 => crate::pickups::MAGIC,
                _ => crate::pickups::GOLD,
            };
            self.pickups.spawn(x, y - 16, drop_kind, em, &mut self.rng);
        }
        let w = self.parsed.level.pixel_width() as i32;
        let h = self.parsed.level.pixel_height() as i32;
        // Box push/pull start (slice 22): a fresh
        // Left/Right press while grabbing (21) starts
        // the push (22, facing direction) or pull (23,
        // opposite) and runs u() at once; failure
        // returns to the grab pose. Only when the grab
        // was already held before this tick — a press
        // that walks the player into the box does not
        // also push it.
        let was_grabbing = self.player.action == 21;
        if was_grabbing && dir != 0 && dir != self.prev_dir {
            self.start_box_push(dir);
        }
        self.prev_dir = dir;
        self.player.tick(&self.player_anim, &self.flags, w, h, dir);
        // Box slots + slide (the original's per-frame
        // box update); the grab check runs after the
        // player tick, when the walk has stopped at
        // the box face.
        for b in &mut self.boxes {
            b.advance(&self.box_anim);
        }
        boxes::check_grab(&self.boxes, &self.flags, &mut self.player, &self.player_anim, dir);
        // Type-9 switch (slice 23): standing on a
        // flag-256 tile toggles the matching doors.
        // The player's tile is (tile_x, gb/16).
        doors::check_switch(
            &self.switches,
            &mut self.doors,
            &mut self.flags,
            self.player.tile_x(),
            self.player.gb / 16,
        );
        // Pickups home on the player's new position.
        let enemy_pos: Vec<(i32, i32, bool)> = self
            .enemy_states
            .iter()
            .map(|st| (st.x, st.y, !st.dead))
            .collect();
        let events = self
            .pickups
            .update((self.player.ga, self.player.gb), &enemy_pos);
        self.apply_pickup_events(events, res);
        // Weakened-marker pulse (the original's
        // `t(132)` at the end of the enemy update).
        if let Some(bar) = &mut self.bar {
            let BarState { anim, slot, .. } = bar;
            slot.advance(anim, 40);
        }
        // Boss-bar pulse (Phase 2, `t(131)`): the
        // slot advances each tick while the `@`
        // latch holds.
        if self.script.boss_latched() {
            if let Some(bb) = &mut self.boss_bar {
                let BarState { anim, slot, .. } = bb;
                slot.advance(anim, 40);
            }
        }
        // Script trigger check (z()): the player's
        // tile, after movement. Arena scripts are not
        // ported (Arena completion is waves, later).
        if !self.arena {
            let tx = self.player.ga / 16;
            let ty = self.player.gb / 16;
            let ev = self.script.check_trigger(tx, ty);
            self.handle_script_event(ev);
            self.apply_weapon_switch();
        }
    }

    pub fn draw(&self, fb: &mut Framebuffer, res: &mut Resources, font: &Font) {
        let tileset = match res.image(2058, palette_key(2068, self.level_index)) {
            Ok(t) => t,
            Err(_) => return,
        };
        level::draw_view(&self.parsed.level, &tileset, fb, self.player.cam_x, self.player.cam_y);
        let g = Graphics::default();
        if let Ok(anim_chunk) = res.chunk(3073) {
            if let Ok(anim_set) = anim::load_anim_set(&anim_chunk) {
                if let Ok(sprite_set) = res.sprite_set(3076, palette_key(3084, self.level_index)) {
                    if let Some(group) = self.parsed.group(23) {
                        anim::draw_static_sprites(
                            &g,
                            fb,
                            &sprite_set,
                            &anim_set,
                            &group.positions,
                            &group.extra,
                            self.player.cam_x,
                            self.player.cam_y,
                        );
                    }
                }
            }
        }
        // Boxes (slice 22): drawn after the statics,
        // before the enemies, at their px position
        // with their slot's current frame.
        for b in &self.boxes {
            b.draw(&g, fb, &self.box_anim, &self.box_sprites, self.player.cam_x, self.player.cam_y);
        }
        let _ = enemies::draw_enemy_states(&g, fb, res, &self.enemy_states, self.player.cam_x, self.player.cam_y);
        // Weakened markers above damaged enemies.
        if let Some(bar) = &self.bar {
            enemies::draw_weakened_markers(
                &g,
                fb,
                &bar.anim,
                &bar.sprites,
                bar.slot.visual,
                &self.enemy_states,
                self.player.cam_x,
                self.player.cam_y,
            );
        }
        let player_transform = if self.player.facing_right { 0 } else { 2 };
        if let Ok(frames) = anim::frames_for_timeline_frame(&self.player_anim, self.player.action, self.player.slot.visual, player_transform) {
            for fr in frames {
                if let Some(frame) = self.player_sprites.frame(fr.sprite_idx) {
                    midp::draw_sprite_frame(
                        &g,
                        fb,
                        &self.player_sprites.image,
                        frame,
                        self.player.ga + fr.dx - self.player.cam_x,
                        self.player.gb + fr.dy - self.player.cam_y,
                        fr.transform,
                    );
                }
            }
        }
        // Pickups render above the world, below the HUD
        // (the original's h(Graphics) runs in the same
        // world-draw pass).
        self.pickups
            .draw(&g, fb, res, self.player.cam_x, self.player.cam_y);
        let hud_state = hud::HudState {
            health: self.player.health,
            max_health: self.player.max_health,
            magic: self.magic,
            max_magic: 1024,
            gold: self.gold,
            combo_hits: self.combo_hits,
        };
        hud::draw_hud(&g, fb, res, font, &hud_state);
        // Boss bar (Phase 2, `d(Graphics)` state 100):
        // drawn at screen (12, F - 25) = (12, 295)
        // when the `@` latch holds, the player's
        // facing `hu` equals `aS`, and the player is
        // inside the viewport (`e.a(ga, gb, iQ, iR,
        // E, F)`). The `!fR` grapple guard is vacuous
        // here (grapple is not ported). The bar is a
        // HUD element: screen coordinates, not
        // camera-translated.
        if self.script.boss_latched()
            && self.script.boss_facing_right() == self.player.facing_right
            && self.player.ga >= self.player.cam_x
            && self.player.ga < self.player.cam_x + 240
            && self.player.gb >= self.player.cam_y
            && self.player.gb < self.player.cam_y + 320
        {
            if let Some(bb) = &self.boss_bar {
                if let Ok(frames) = anim::frames_for_timeline_frame(
                    &bb.anim,
                    self.script.boss_action() as usize,
                    bb.slot.visual,
                    0,
                ) {
                    for fr in frames {
                        if let Some(frame) = bb.sprites.frame(fr.sprite_idx) {
                            midp::draw_sprite_frame(
                                &g,
                                fb,
                                &bb.sprites.image,
                                frame,
                                12 + fr.dx,
                                295 + fr.dy,
                                fr.transform,
                            );
                        }
                    }
                }
            }
        }
        // Script dialog on top (audio-prompt box
        // geometry; original script-dialog box args
        // are untraced, see script.rs).
        if let Some(text) = self.script.dialog_text() {
            draw_dialog_box(fb, font, text);
        }
        // Level intro dialog on top of that (the two
        // are mutually exclusive in practice: scripts
        // cannot trigger while the intro is showing).
        if let Some(text) = &self.intro_text {
            draw_dialog_box(fb, font, text);
        }
    }
}

/// Draw a full-screen dialog box (audio-prompt
/// geometry, shared by script dialogs and the
/// level intro): black box, wrapped white text
/// from y=55, "Select" hint bottom right inside
/// the box. All level intros fit the box (longest
/// is 243 chars, ~7 lines), so no scrolling.
pub fn draw_dialog_box(fb: &mut Framebuffer, font: &Font, text: &str) {
    // Default Graphics color is black for the
    // box; text draws in white.
    let mut gg = Graphics::default();
    gg.fill_rect(fb, 12, 30, 211, 270);
    gg.set_color_rgb(255, 255, 255);
    let lines = wrap_dialog_text(font, text, 206);
    let mut y = 55;
    for line in lines {
        font.draw(&gg, fb, &line, 17, y, 4);
        y += 12;
    }
    // Softkey hint, bottom right.
    font.draw(&gg, fb, "Select", 220, 290, 8);
}


/// Word-wrap dialog text to `width` pixels, breaking
/// at spaces (the original also breaks at '.'; word
/// wrap matches it for the game's sentences).
fn wrap_dialog_text(font: &Font, text: &str, width: i32) -> Vec<String> {
    let mut lines = Vec::new();
    for part in text.split('\n') {
        let mut cur = String::new();
        for word in part.split(' ') {
            if cur.is_empty() {
                cur = word.to_string();
            } else if font.width(&format!("{cur} {word}")) <= width {
                cur.push(' ');
                cur.push_str(word);
            } else {
                lines.push(std::mem::take(&mut cur));
                cur = word.to_string();
            }
        }
        lines.push(cur);
    }
    lines
}

#[cfg(test)]
mod tests {
    /// Advance the script (if any) to completion:
    /// the spawn-area prayer dialog fires on the
    /// player's start tile and would otherwise
    /// freeze every later tick in these tests.
    fn drain_script(gp: &mut super::Gameplay) {
        while gp.script.active() {
            gp.script_advance();
        }
    }

    /// Tick once, then drain any script dialog
    /// that fired on the new tile (spawn-area
    /// tutorial dialogs would otherwise freeze
    /// the world for the rest of the test).
    fn t(
        gp: &mut super::Gameplay,
        res: &mut crate::resources::Resources,
        dir: i32,
        attack: bool,
    ) {
        gp.tick(res, dir, attack);
        drain_script(gp);
    }

    /// Phase 2: the boss bar latches on an `@`
    /// trigger, animates while latched, and clears
    /// when the player leaves the trigger tiles.
    /// Teleports to the `@21` wall-face trigger at
    /// tile (135, 28) in level 9217.
    #[test]
    fn boss_bar_latches_and_animates() {
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 9217, false).unwrap();
        gp.dismiss_intro();
        assert!(gp.boss_bar.is_some(), "boss-bar sets load");
        assert!(!gp.script.boss_latched());
        // Teleport onto the `@21` wall-face tile,
        // facing right (aS=true for `@21`).
        gp.player.ga = 135 * 16;
        gp.player.gb = 28 * 16;
        gp.player.facing_right = true;
        t(&mut gp, &mut res, 0, false);
        assert!(gp.script.boss_latched());
        assert_eq!(gp.script.boss_action(), 2, "aT = 1st digit of `@21`");
        assert!(gp.script.boss_facing_right(), "aS = 2nd digit '1'");
        // The slot is (re)set to action 2 on latch.
        let bb = gp.boss_bar.as_ref().unwrap();
        assert_eq!(bb.slot.action, 2);
        // The bar animates (t(131)) while latched:
        // action 2 is a 2-frame loop (t0 200 ms,
        // t1 100 ms), so after 6 ticks (240 ms) the
        // visual has moved from frame 0 to frame 1.
        let v0 = bb.slot.visual;
        assert_eq!(v0, 0, "action 2 starts on timeline frame 0");
        for _ in 0..6 {
            t(&mut gp, &mut res, 0, false);
        }
        assert!(gp.script.boss_latched(), "still on the trigger tile");
        let v1 = gp.boss_bar.as_ref().unwrap().slot.visual;
        assert_eq!(v1, 1, "boss-bar slot advances to frame 1 after 240 ms");
        // Draw the latched bar: smoke test (no
        // panic) and the bar changes pixels in its
        // screen band (y 270-300, x 0-80). The
        // camera lags a teleport, so snap it to
        // the player first — otherwise the
        // viewport guard (`e.a(ga, gb, iQ, iR, E,
        // F)`) correctly hides the bar.
        gp.player.cam_x = gp.player.ga - 120;
        gp.player.cam_y = gp.player.gb - 160;
        let mut fb = crate::midp::Framebuffer::new(240, 320);
        let font_set = res.sprite_set(1031, None).expect("font 1031");
        let font = crate::font::Font::from_sprite_set(font_set);
        gp.draw(&mut fb, &mut res, &font);
        // Same scene, same camera, player turned
        // away (aS=true needs facing right): the
        // bar hides. The band is fully opaque
        // ground, so compare pixel values, not
        // non-zero counts.
        gp.player.facing_right = false;
        let mut fb2 = crate::midp::Framebuffer::new(240, 320);
        gp.draw(&mut fb2, &mut res, &font);
        let changed = (270..300)
            .flat_map(|y| (0..80).map(move |x| (x, y)))
            .filter(|&(x, y)| fb.pixels[y * 240 + x] != fb2.pixels[y * 240 + x])
            .count();
        assert!(changed > 0, "latched bar changes pixels in its screen band");
        // Step off the trigger tiles: latch clears.
        gp.player.ga = 100 * 16;
        gp.player.gb = 35 * 16;
        gp.player.facing_right = true;
        t(&mut gp, &mut res, 0, false);
        assert!(!gp.script.boss_latched());
    }

    #[test]
    fn gameplay_load_and_walk() {
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 9217, false).unwrap();
        assert_eq!(gp.player.ga, 48);
        assert_eq!(gp.player.gb, 560);
        // Level 9217 has an intro dialog: dismiss it
        // before walking, or the tick freezes.
        gp.dismiss_intro();
        gp.tick(&mut res, 1, false);
        assert!(gp.player.ga >= 48);
        assert_eq!(gp.player.cam_x, 0);
    }

    /// Slice 21, Gameplay routing: Up double-jumps
    /// onto the climb column (tile 156) and attaches;
    /// a further Up climbs (35); Down held climbs
    /// down (34). Teleports to the temple floor.
    #[test]
    fn gameplay_climb_attach_up_down() {
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 9217, false).unwrap();
        gp.dismiss_intro();
        gp.player.ga = 2440;
        gp.player.gb = 528;
        gp.player.facing_right = true;
        assert!(gp.jump(), "first jump must start");
        for _ in 0..12 {
            t(&mut gp, &mut res, 1, false);
        }
        assert_eq!(gp.player.action, 24);
        assert!(gp.jump(), "double jump must start");
        let mut attached = false;
        for _ in 0..60 {
            t(&mut gp, &mut res, 1, false);
            if gp.player.action == 33 {
                attached = true;
                break;
            }
        }
        assert!(attached, "never attached: action {}", gp.player.action);
        assert_eq!(gp.player.ga, 156 * 16 - 10);
        // Up while holding climbs one cycle (-19 px).
        let start_gb = gp.player.gb;
        assert!(gp.jump(), "climb-up refused");
        assert_eq!(gp.player.action, 35);
        let mut held = false;
        for _ in 0..30 {
            t(&mut gp, &mut res, 0, false);
            if gp.player.action == 33 {
                held = true;
                break;
            }
        }
        assert!(held, "climb-up never held: action {}", gp.player.action);
        assert_eq!(gp.player.gb, start_gb - 19);
        // Down held climbs down one cycle (+20 px).
        gp.block_held = true;
        let start_gb = gp.player.gb;
        let mut down = false;
        for _ in 0..30 {
            t(&mut gp, &mut res, 0, false);
            if gp.player.action == 34 {
                down = true;
                break;
            }
        }
        assert!(down, "Down held never started climb-down: action {}", gp.player.action);
        let mut held = false;
        for _ in 0..30 {
            t(&mut gp, &mut res, 0, false);
            if gp.player.action == 33 {
                held = true;
                break;
            }
        }
        assert!(held, "climb-down never held: action {}", gp.player.action);
        assert_eq!(gp.player.gb, start_gb + 20);
        gp.block_held = false;
    }

    #[test]
    fn script_trigger_at_level_end_advances() {
        // Teleport onto the level-1 end trigger tile
        // (214, 30): script [^dialog, !, *].
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 9217, false).unwrap();
        gp.dismiss_intro();
        gp.player.ga = 214 * 16;
        gp.player.gb = 30 * 16;
        gp.tick(&mut res, 0, false);
        assert!(gp.script.active());
        assert_eq!(
            gp.script.dialog_text(),
            Some("The enemies in the temple were crushed, as Kratos continued his battle.")
        );
        // One Select skips the '!' and hits '*'.
        gp.script_advance();
        assert_eq!(gp.level_complete, Some(1));
        // Neighbour tiles of the same script are spent.
        gp.player.ga = 214 * 16;
        gp.player.gb = 31 * 16;
        gp.tick(&mut res, 0, false);
        assert!(!gp.script.active());
    }

    #[test]
    fn ground_combo_chains_with_damage_steps() {
        // Ground combo (slice 16): 54 (dmg 2), 55
        // (dmg 2), 56 (dmg 4), 60 finisher (dmg 4),
        // then back to 54. Each step is buffered by
        // firing after the 150 ms cancel window and
        // consumed when the swing ends.
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 9217, false).unwrap();
        gp.dismiss_intro();
        t(&mut gp, &mut res, 0, true);
        assert_eq!(gp.player.action, 54);
        assert_eq!(gp.player.combo_step, 1);
        assert_eq!(gp.player.swing_damage, 2);
        // Chain through 55, 56, 60, back to 54.
        for (want_action, want_step, want_dmg, want_finisher) in [
            (55, 2, 2, false),
            (56, 3, 4, false),
            (60, 3, 4, true),
            (54, 1, 2, false),
        ] {
            // Wait out the cancel window, then buffer.
            for _ in 0..4 {
                t(&mut gp, &mut res, 0, false);
            }
            t(&mut gp, &mut res, 0, true);
            assert!(gp.player.attack_buffered, "swing did not buffer");
            // Tick until the swing ends and chains.
            let mut chained = false;
            for _ in 0..40 {
                t(&mut gp, &mut res, 0, false);
                if gp.player.action == want_action {
                    chained = true;
                    break;
                }
            }
            assert!(chained, "combo did not chain to {want_action}");
            assert_eq!(gp.player.combo_step, want_step);
            assert_eq!(gp.player.swing_damage, want_dmg);
            assert_eq!(gp.player.finisher_pending, want_finisher);
        }
    }

    #[test]
    fn early_fire_does_not_buffer() {
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 9217, false).unwrap();
        gp.dismiss_intro();
        t(&mut gp, &mut res, 0, true);
        assert_eq!(gp.player.action, 54);
        for _ in 0..2 {
            t(&mut gp, &mut res, 0, false);
        }
        // 80 ms in: inside the cancel window.
        assert!(!gp.player.attack(&gp.player_anim));
        assert!(!gp.player.attack_buffered);
        for _ in 0..2 {
            t(&mut gp, &mut res, 0, false);
        }
        // 160 ms in: past the window, buffers.
        assert!(gp.player.attack(&gp.player_anim));
        assert!(gp.player.attack_buffered);
    }

    #[test]
    fn combo_damage_lands_on_enemy() {
        // The 54/55/56 swings deal 2/2/4 to an enemy
        // in front (slice 16): the hit loop uses the
        // current swing's damage, not a constant 2.
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 9217, false).unwrap();
        gp.dismiss_intro();
        // Stand 32 px left of the 448 enemy (13 hp),
        // facing right; it attacks in place instead
        // of walking, so it stays in range.
        gp.player.ga = 416;
        let health = |gp: &super::Gameplay| {
            gp.enemy_states
                .iter()
                .find(|st| st.enemy.x == 448)
                .map(|st| st.health)
                .unwrap()
        };
        assert_eq!(health(&gp), 13);
        t(&mut gp, &mut res, 0, true);
        assert_eq!(gp.player.action, 54);
        for _ in 0..40 {
            t(&mut gp, &mut res, 0, false);
            if health(&gp) == 11 {
                break;
            }
        }
        assert_eq!(health(&gp), 11, "54 swing should deal 2");
        // Chain to 55, then 56, checking each hit.
        // Two ticks after a hit leaves the swing
        // still running with the cancel window open
        // (four would run past the 54 swing's end).
        for (want_action, want_health) in [(55, 9), (56, 5)] {
            for _ in 0..2 {
                t(&mut gp, &mut res, 0, false);
            }
            t(&mut gp, &mut res, 0, true);
            assert!(gp.player.attack_buffered);
            for _ in 0..40 {
                t(&mut gp, &mut res, 0, false);
                if gp.player.action == want_action {
                    break;
                }
            }
            assert_eq!(gp.player.action, want_action);
            for _ in 0..40 {
                t(&mut gp, &mut res, 0, false);
                if health(&gp) == want_health {
                    break;
                }
            }
            assert_eq!(
                health(&gp),
                want_health,
                "swing {want_action} hit for the wrong damage"
            );
        }
    }

    #[test]
    fn artemis_ground_combo() {
        // Artemis ground combo (slice 17): 72
        // (dmg 4), 73 (dmg 10), 77 finisher
        // (dmg 10), then back to 72.
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 9217, false).unwrap();
        gp.dismiss_intro();
        // Artemis needs magic: with 0 the A()
        // guard unequips instead of swinging.
        gp.magic = 100;
        gp.player.weapon_mode = 1;
        // Away from spawn: the tutorial chain
        // there contains a `^` switch step that
        // would overwrite the mode on the first
        // tick (that is slice 17 working, not a
        // bug - just not what this test measures).
        gp.player.ga = 416;
        t(&mut gp, &mut res, 0, true);
        assert_eq!(gp.player.action, 72);
        assert_eq!(gp.player.combo_step, 1);
        assert_eq!(gp.player.swing_damage, 4);
        for (want_action, want_step, want_dmg, want_finisher) in [
            (73, 2, 10, false),
            (77, 2, 10, true),
            (72, 1, 4, false),
        ] {
            // Tick until the cancel window opens
            // (a fixed count cannot fit both the
            // 240 ms 72 swing and the post-chain
            // timer reset: 3 plains starve the
            // 73 chain, 4 plains end the 72 swing
            // on the buffer tick itself).
            for _ in 0..10 {
                t(&mut gp, &mut res, 0, false);
                if gp.player.attack_time_ms > 150 {
                    break;
                }
            }
            t(&mut gp, &mut res, 0, true);
            assert!(gp.player.attack_buffered, "swing did not buffer");
            let mut chained = false;
            for _ in 0..40 {
                t(&mut gp, &mut res, 0, false);
                if gp.player.action == want_action {
                    chained = true;
                    break;
                }
            }
            assert!(chained, "combo did not chain to {want_action}");
            assert_eq!(gp.player.combo_step, want_step);
            assert_eq!(gp.player.swing_damage, want_dmg);
            assert_eq!(gp.player.finisher_pending, want_finisher);
        }
    }


    #[test]
    fn weapon_cycle_and_magic_modes() {
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 9217, false).unwrap();
        gp.dismiss_intro();
        assert_eq!(gp.player.weapon_mode, 0);
        assert!(gp.cycle_weapon());
        assert_eq!(gp.player.weapon_mode, 1);
        assert!(gp.cycle_weapon());
        assert_eq!(gp.player.weapon_mode, 2);
        assert!(gp.cycle_weapon());
        assert_eq!(gp.player.weapon_mode, 3);
        assert!(gp.cycle_weapon());
        assert_eq!(gp.player.weapon_mode, 0);
        // Fire in a magic mode starts the cast
        // (Phase 3), not a blades swing.
        gp.magic = 500;
        gp.player.weapon_mode = 2;
        assert!(gp.player.attack(&gp.player_anim));
        assert_eq!(gp.player.action, 94);
        gp.player.end_magic_cast(&gp.player_anim);
        gp.player.weapon_mode = 3;
        assert!(gp.player.attack(&gp.player_anim));
        assert_eq!(gp.player.action, 95);
        gp.player.end_magic_cast(&gp.player_anim);
        // Cycling is blocked during a ground swing.
        // (Same spawn-tile caveat as above.)
        gp.magic = 100;
        gp.player.weapon_mode = 1;
        gp.player.ga = 416;
        t(&mut gp, &mut res, 0, true);
        assert_eq!(gp.player.action, 72);
        assert!(!gp.cycle_weapon());
        assert_eq!(gp.player.weapon_mode, 1);
    }

    #[test]
    fn artemis_drain_and_unequip() {
        // Artemis hits drain magic (fJ * 3); at 0
        // magic the next attack unequips (A()).
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 9217, false).unwrap();
        gp.dismiss_intro();
        gp.magic = 100;
        gp.player.weapon_mode = 1;
        gp.player.ga = 416;
        let health = |gp: &super::Gameplay| {
            gp.enemy_states
                .iter()
                .find(|st| st.enemy.x == 448)
                .map(|st| st.health)
                .unwrap()
        };
        t(&mut gp, &mut res, 0, true);
        assert_eq!(gp.player.action, 72);
        for _ in 0..40 {
            t(&mut gp, &mut res, 0, false);
            if health(&gp) == 9 {
                break;
            }
        }
        assert_eq!(health(&gp), 9, "72 swing should deal 4");
        assert_eq!(gp.magic, 88, "Artemis hit should drain 4 * 3");
        // Spend the rest: next attack unequips.
        gp.magic = 0;
        t(&mut gp, &mut res, 0, true);
        assert_eq!(gp.player.weapon_mode, 0);
        // And the next swing is back to blades.
        for _ in 0..40 {
            t(&mut gp, &mut res, 0, false);
            if gp.player.action == 0 {
                break;
            }
        }
        t(&mut gp, &mut res, 0, true);
        assert_eq!(gp.player.action, 54);
    }

    #[test]
    fn mode3_cast_spawns_damage_orbs_and_costs_256() {
        // Mode 3 (hd, Phase 3): fire with magic >
        // 256 spawns type-3 orbs totalling fL[gW]
        // (128 at gW 0), costs a flat 256, plays
        // action 95, and resets to blades (A()).
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 9217, false).unwrap();
        gp.dismiss_intro();
        gp.magic = 500;
        gp.player.weapon_mode = 3;
        // Stand by the 448 enemy so it is in the
        // viewport and gets targeted; tick first
        // so the camera catches up to the teleport.
        gp.player.ga = 416;
        for _ in 0..20 {
            t(&mut gp, &mut res, 0, false);
        }
        t(&mut gp, &mut res, 0, true);
        assert_eq!(gp.player.action, 95, "mode 3 should play action 95");
        assert_eq!(gp.player.weapon_mode, 0, "cast resets to blades");
        assert_eq!(gp.magic, 244, "flat 256 cost (500 - 256)");
        assert_eq!(gp.pickups.damage_orb_count(), 9, "128 -> 9 orbs");
        // The orbs home on the 448 enemy and deal
        // their total 128 damage (13 hp -> dead).
        let health = |gp: &super::Gameplay| {
            gp.enemy_states
                .iter()
                .find(|st| st.enemy.x == 448)
                .map(|st| st.health)
                .unwrap()
        };
        let start = health(&gp);
        for _ in 0..400 {
            t(&mut gp, &mut res, 0, false);
            if gp.pickups.damage_orb_count() == 0 {
                break;
            }
        }
        assert_eq!(gp.pickups.damage_orb_count(), 0, "orbs never landed");
        assert!(health(&gp) < start, "damage orbs dealt no damage");
    }

    #[test]
    fn mode3_gate_blocks_low_magic_and_full_orbs() {
        // Gate: magic <= 256, or 10+ active damage
        // orbs, falls back to a blades swing and
        // resets to blades (the original's s(54)).
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 9217, false).unwrap();
        gp.dismiss_intro();
        gp.magic = 256;
        gp.player.weapon_mode = 3;
        t(&mut gp, &mut res, 0, true);
        assert_eq!(gp.player.weapon_mode, 0, "failed gate resets to blades");
        assert_eq!(gp.player.action, 54, "failed gate falls to a swing");
        assert_eq!(gp.pickups.damage_orb_count(), 0);
        assert_eq!(gp.magic, 256, "no cost on a failed gate");
    }

    #[test]
    fn mode2_cast_channels_and_drains() {
        // Mode 2 (hc, Phase 3): fire with magic >
        // (gV+1)*4 starts action 94; each tick
        // with attack pressed drains 12; a tick
        // without attack ends the cast (idle +
        // blades). The hit itself applies no
        // damage (dX stagger is Phase 6).
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 9217, false).unwrap();
        gp.dismiss_intro();
        gp.magic = 100;
        gp.player.weapon_mode = 2;
        // Away from spawn (a spawn script `^` step
        // would otherwise reset the weapon mode).
        gp.player.ga = 416;
        t(&mut gp, &mut res, 0, true);
        assert_eq!(gp.player.action, 94, "mode 2 should play action 94");
        assert_eq!(gp.magic, 88, "12 drained on the fire tick");
        assert_eq!(gp.player.weapon_mode, 2, "channel keeps the mode");
        t(&mut gp, &mut res, 0, true);
        assert_eq!(gp.magic, 76, "12 drained per channel tick");
        assert_eq!(gp.player.action, 94, "still channeling");
        // Release attack: the cast ends.
        t(&mut gp, &mut res, 0, false);
        assert_eq!(gp.player.action, 0, "cast ends on release");
        assert_eq!(gp.player.weapon_mode, 0, "cast end resets to blades");
        assert_eq!(gp.magic, 76, "no drain after the cast ends");
    }

    #[test]
    fn mode2_cast_ends_when_magic_runs_out() {
        // Channel until magic hits 0: the cast
        // ends (idle + blades) instead of going
        // negative.
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 9217, false).unwrap();
        gp.dismiss_intro();
        gp.magic = 30;
        gp.player.weapon_mode = 2;
        // Away from spawn (a spawn script `^` step
        // would otherwise reset the weapon mode).
        gp.player.ga = 416;
        t(&mut gp, &mut res, 0, true);
        assert_eq!(gp.player.action, 94);
        assert_eq!(gp.magic, 18);
        t(&mut gp, &mut res, 0, true);
        assert_eq!(gp.magic, 6);
        assert_eq!(gp.player.action, 94);
        t(&mut gp, &mut res, 0, true);
        assert_eq!(gp.magic, 0, "drain clamps at 0");
        // The cast ends on the tick after magic
        // reaches 0 (the drain tick itself still
        // channels).
        t(&mut gp, &mut res, 0, true);
        assert_eq!(gp.player.action, 0, "cast ends at 0 magic");
        assert_eq!(gp.player.weapon_mode, 0);
    }

    #[test]
    fn block_negates_damage_and_plays_reaction() {
        // Blocking (slice 18): Down held, an enemy
        // hit deals no damage and plays the
        // block-impact reaction (action 97) facing
        // the attacker; releasing Down ends it.
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 9217, false).unwrap();
        gp.dismiss_intro();
        gp.player.ga = 416;
        gp.block_held = true;
        t(&mut gp, &mut res, 0, false);
        assert!(gp.player.blocking);
        assert_eq!(gp.player.action, 3);
        let mut reacted = false;
        for _ in 0..80 {
            t(&mut gp, &mut res, 0, false);
            if gp.player.action == 97 {
                reacted = true;
                break;
            }
        }
        assert!(reacted, "blocked hit never played action 97");
        assert_eq!(gp.player.health, 1024, "blocked hit must deal no damage");
        assert!(gp.player.facing_right, "reaction should face the attacker");
        gp.block_held = false;
        t(&mut gp, &mut res, 0, false);
        assert_eq!(gp.player.action, 0);
        assert!(!gp.player.blocking);
    }


    #[test]
    fn block_stance_ignores_attack_and_resumes() {
        // At spawn (no enemies in range): Down held
        // holds pose 3, attacks are ignored, a
        // 97 reaction ends back in pose 3 while
        // Down stays held, release ends the stance.
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 9217, false).unwrap();
        gp.dismiss_intro();
        gp.block_held = true;
        t(&mut gp, &mut res, 0, false);
        assert_eq!(gp.player.action, 3);
        assert!(gp.player.blocking);
        t(&mut gp, &mut res, 0, true);
        assert_eq!(gp.player.action, 3, "attack must be ignored while blocking");
        // Fake a blocked-hit reaction, then watch it
        // end back in the block pose while held.
        let slot = super::anim::AnimSlot::new(&gp.player_anim, 97);
        gp.player.action = 97;
        gp.player.slot = slot;
        let mut resumed = false;
        for _ in 0..40 {
            t(&mut gp, &mut res, 0, false);
            if gp.player.action == 3 {
                resumed = true;
                break;
            }
        }
        assert!(resumed, "reaction did not resume the block pose");
        assert!(gp.player.blocking);
        gp.block_held = false;
        t(&mut gp, &mut res, 0, false);
        assert_eq!(gp.player.action, 0);
        assert!(!gp.player.blocking);
    }

    #[test]
    fn air_attack_from_jump_then_falls() {
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 9217, false).unwrap();
        gp.dismiss_intro();
        assert!(gp.jump());
        for _ in 0..10 {
            t(&mut gp, &mut res, 0, false);
        }
        assert_eq!(gp.player.action, 24, "should still be in the jump arc");
        t(&mut gp, &mut res, 0, true);
        assert_eq!(gp.player.action, 67);
        assert_eq!(gp.player.combo_step, 1);
        assert_eq!(gp.player.swing_damage, 2);
        // Unbuffered air swing ends in a fall.
        let mut fell = false;
        for _ in 0..40 {
            t(&mut gp, &mut res, 0, false);
            if gp.player.action == 26 {
                fell = true;
                break;
            }
        }
        assert!(fell, "air swing did not end in a fall");
        // Fall lands back at idle with a reset combo.
        for _ in 0..60 {
            t(&mut gp, &mut res, 0, false);
            if gp.player.action == 0 {
                break;
            }
        }
        assert_eq!(gp.player.action, 0);
        assert_eq!(gp.player.combo_step, 0);
    }

    #[test]
    fn intro_dialog_for_levels_with_q() {
        let mut res = crate::resources::Resources::new("../extracted");
        // 9217 (Q=122) opens with an intro dialog.
        let mut gp = super::Gameplay::load(&mut res, 9217, false).unwrap();
        assert!(gp.intro_active());
        assert!(gp
            .intro_text
            .as_deref()
            .unwrap()
            .starts_with("Since defeating Ares"));
        // The world is frozen until it is dismissed.
        gp.tick(&mut res, 1, false);
        assert_eq!(gp.player.ga, 48);
        gp.dismiss_intro();
        assert!(!gp.intro_active());
        gp.tick(&mut res, 1, false);
        assert!(gp.player.ga > 48);
        // 19456 (Q=0) has no intro; 23552 and 27648 do.
        let gp2 = super::Gameplay::load(&mut res, 19456, false).unwrap();
        assert!(!gp2.intro_active());
        let gp3 = super::Gameplay::load(&mut res, 23552, false).unwrap();
        assert!(gp3.intro_active());
        let gp4 = super::Gameplay::load(&mut res, 27648, false).unwrap();
        assert!(gp4.intro_active());
    }

    /// Slice 22, Gameplay routing: walk right into the
    /// box at tile 128 (grab 21), release, fresh press
    /// right pushes it 2 tiles (eQ 130), and the box
    /// slides to its new tile while the player idles
    /// (a 2-tile push leaves it out of reach).
    /// Slice 23: the wall crossing in Gameplay — push
    /// the crate to the wall, jump onto it, double jump
    /// to the roof ledge, and mount onto the roof
    /// (past the ga=2161 wall). The roof-to-temple-
    /// floor route past the hanging pillar is a
    /// level-navigation detail (double-jump onto the
    /// row-20 structure, then drop); the level-end
    /// flow is verified separately below.
    #[test]
    /// Slice 23: the temple-floor run to the level
    /// end. From the temple floor (3200,528), walk
    /// right to the lever at tile (206,32); Down
    /// pulls it, opening the pillar door (cS=6) at
    /// (213,32); then walk through to the level-end
    /// trigger at tile 214 (cn=175) and
    /// `level_complete` is set.
    #[test]
    fn gameplay_temple_floor_to_level_end() {
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 9217, false).unwrap();
        gp.dismiss_intro();
        // Teleport to the temple floor, facing right.
        gp.player.ga = 3200;
        gp.player.gb = 528;
        gp.player.facing_right = true;
        gp.player.action = 0;
        gp.player.slot = crate::anim::AnimSlot::new(&gp.player_anim, 0);
        let mut ticks = 0;
        let mut pulled = false;
        // Walk right to the lever (206,32) at
        // ga=3296; press Down there (one tick with
        // block_held) to pull it, then keep walking.
        while ticks < 400 && gp.level_complete.is_none() {
            ticks += 1;
            if !pulled && gp.player.ga >= 206 * 16 {
                pulled = true;
                gp.block_held = true;
                gp.tick(&mut res, 0, false);
                gp.block_held = false;
                continue;
            }
            gp.tick(&mut res, 1, false);
            // Drain any script dialog that fires.
            while gp.script.active() {
                gp.script_advance();
            }
        }
        assert!(pulled, "the lever pull should have fired");
        assert!(gp.doors.iter().find(|d| d.id == 6).map(|d| d.open).unwrap_or(false), "pillar door should be open");
        assert!(gp.level_complete.is_some(), "level should complete; ga={} gb={} action={}", gp.player.ga, gp.player.gb, gp.player.action);
        assert_eq!(gp.level_complete, Some(1), "next level index 1");
    }

    fn gameplay_wall_crossing_mounts_roof() {
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 9217, false).unwrap();
        gp.dismiss_intro();
        gp.player.ga = 1900;
        gp.player.gb = 560;
        gp.player.facing_right = true;
        // Walk to the crate and grab it.
        for _ in 0..60 {
            t(&mut gp, &mut res, 1, false);
            if gp.player.action == 21 {
                break;
            }
        }
        assert_eq!(gp.player.action, 21, "never grabbed the crate");
        // Push it to the wall (128 -> 130 -> 132 -> 134):
        // after each push the player idles out of reach,
        // so walk back to the crate and grab it again.
        for want in [130, 132, 134] {
            t(&mut gp, &mut res, 0, false);
            t(&mut gp, &mut res, 1, false);
            assert_eq!(gp.player.action, 22, "push to {} never started", want);
            assert_eq!(gp.boxes[1].e_q, want, "box eQ {}", gp.boxes[1].e_q);
            for _ in 0..60 {
                t(&mut gp, &mut res, 0, false);
                if gp.player.action == 0 && gp.boxes[1].x == want * 16 {
                    break;
                }
            }
            assert_eq!(gp.boxes[1].x, want * 16, "box never slid to {}", want);
            if want == 134 {
                break;
            }
            // Walk back to the crate and grab it again.
            let mut grabbed = false;
            for _ in 0..60 {
                t(&mut gp, &mut res, 1, false);
                if gp.player.action == 21 {
                    grabbed = true;
                    break;
                }
            }
            assert!(grabbed, "never re-grabbed for push to {}", want + 2);
        }
        // Jump out of the grab onto the crate top.
        // After the last push the player idles out of
        // reach, so walk back and grab the crate again.
        let mut grabbed = false;
        for _ in 0..60 {
            t(&mut gp, &mut res, 1, false);
            if gp.player.action == 21 {
                grabbed = true;
                break;
            }
        }
        assert!(grabbed, "never re-grabbed the crate at the wall: ga={} action={}", gp.player.ga, gp.player.action);
        gp.jump();
        let mut on_box = false;
        for _ in 0..40 {
            t(&mut gp, &mut res, 1, false);
            if gp.player.action == 0 && gp.player.gb == 528 {
                on_box = true;
                break;
            }
        }
        assert!(on_box, "never landed on the crate: ga={} gb={} action={}", gp.player.ga, gp.player.gb, gp.player.action);
        // Double jump from the crate top to the roof ledge.
        gp.jump();
        for _ in 0..12 {
            t(&mut gp, &mut res, 1, false);
        }
        gp.jump();
        let mut hung = false;
        for _ in 0..60 {
            t(&mut gp, &mut res, 1, false);
            if gp.player.action == 37 {
                hung = true;
                break;
            }
        }
        assert!(hung, "never grabbed the roof ledge: ga={} gb={} action={}", gp.player.ga, gp.player.gb, gp.player.action);
        assert_eq!(gp.player.gb, 26 * 16 + crate::player::PLAYER_GD);
        // Mount onto the roof.
        gp.jump();
        assert_eq!(gp.player.action, 38, "mount never started: {}", gp.player.action);
        for _ in 0..40 {
            t(&mut gp, &mut res, 0, false);
            if gp.player.action == 0 {
                break;
            }
        }
        assert_eq!(gp.player.action, 0, "mount never finished");
        assert_eq!(gp.player.gb, 26 * 16, "not on the roof: gb={}", gp.player.gb);
        assert!(gp.player.ga > 136 * 16, "not past the wall: ga={}", gp.player.ga);
    }

    #[test]
    fn gameplay_box_grab_push() {
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 9217, false).unwrap();
        gp.dismiss_intro();
        gp.player.ga = 1900;
        gp.player.gb = 560;
        gp.player.facing_right = true;
        let mut grabbed = false;
        for _ in 0..60 {
            t(&mut gp, &mut res, 1, false);
            if gp.player.action == 21 {
                grabbed = true;
                break;
            }
        }
        assert!(
            grabbed,
            "never grabbed: ga {} action {}",
            gp.player.ga, gp.player.action
        );
        assert_eq!(gp.player.grabbed_box, Some(1), "the crate is item 1");
        assert_eq!(gp.boxes[1].e_q, 128);
        // Release the direction (so the next press is
        // a fresh edge), then press right to push.
        t(&mut gp, &mut res, 0, false);
        assert_eq!(gp.player.action, 21, "grab must hold through the release");
        t(&mut gp, &mut res, 1, false);
        assert_eq!(
            gp.player.action, 22,
            "fresh press pushes: action {}",
            gp.player.action
        );
        assert_eq!(gp.boxes[1].e_q, 130, "push moves 2 tiles: eQ {}", gp.boxes[1].e_q);
        // The push plays out (~25 ticks); the box
        // slides to its new tile (its action runs a
        // tick longer than the player's push) and the
        // player idles (a 2-tile push leaves the box
        // out of reach).
        for _ in 0..60 {
            t(&mut gp, &mut res, 0, false);
            if gp.player.action == 0 && gp.boxes[1].x == 130 * 16 {
                break;
            }
        }
        assert_eq!(
            gp.player.action, 0,
            "after the push the player idles: {}",
            gp.player.action
        );
        assert_eq!(gp.boxes[1].x, 130 * 16, "box slid to its new tile: {}", gp.boxes[1].x);
    }
}

#[cfg(test)]
mod level_tests {
    #[test]
    fn all_levels_and_arena_load() {
        let mut res = crate::resources::Resources::new("../extracted");
        let mut ids = super::LEVEL_IDS.to_vec();
        ids.push(super::ARENA_ID);
        for id in ids {
            let arena = id == super::ARENA_ID;
            let gp = super::Gameplay::load(&mut res, id, arena)
                .unwrap_or_else(|e| panic!("level {id} failed to load: {e}"));
            assert!(!gp.enemy_states.is_empty(), "level {id} has no enemies");
            assert!(gp.player.ga >= 0 && gp.player.gb >= 0, "level {id} spawn invalid");
        }
    }

    #[test]
    fn palette_keys_match_bg_table() {
        assert_eq!(super::palette_key(2068, 0), None);
        assert_eq!(super::palette_key(2068, 3), Some(2070));
        assert_eq!(super::palette_key(3084, 5), Some(3085));
        assert_eq!(super::level_index(27648), 9);
        assert_eq!(super::level_index(9216), 0); // Arena: no bB entry, index 0
    }
}





