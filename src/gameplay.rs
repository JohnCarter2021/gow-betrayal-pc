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
use crate::enemies::{self, EnemyState};
use crate::entities::{self, LevelEntities};
use crate::font::Font;
use crate::hud;
use crate::level;
use crate::midp::{self, Framebuffer, Graphics};
use crate::player::{Player, TileFlags};
use crate::resources::Resources;
use crate::resources::SpriteSet;

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
}

impl Gameplay {
    pub fn load(res: &mut Resources, level_id: u32, arena: bool) -> Result<Gameplay, String> {
        let chunk = res.chunk(level_id).map_err(|e| format!("{e:?}"))?;
        let parsed = entities::parse_level_entities(level_id, &chunk).map_err(|e| e.to_string())?;
        let flags = TileFlags::build(&parsed);
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
        let mut enemy_states = Vec::new();
        for e in enemies::enemies(&parsed) {
            if let Ok(anim_chunk) = res.chunk(e.anim_id) {
                if let Ok(anim_set) = anim::load_anim_set(&anim_chunk) {
                    enemy_states.push(EnemyState::new(e, &anim_set));
                }
            }
        }
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
        })
    }

    /// One 40 ms tick. `dir`: -1 left, 0 none, +1 right.
    /// `attack`: fire key pressed this frame.
    pub fn tick(&mut self, res: &mut Resources, dir: i32, attack: bool) {
        if attack {
            self.player.attack(&self.player_anim);
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
                        self.player.health -= dmg;
                    }
                }
            }
        }
        if self.player.action == 54 && self.player.slot.frame >= 2 && !self.player.attack_hit_done {
            self.player.attack_hit_done = true;
            for st in &mut self.enemy_states {
                if st.dead {
                    continue;
                }
                let in_front = if self.player.facing_right {
                    st.x - self.player.ga >= 0 && st.x - self.player.ga <= 32
                } else {
                    self.player.ga - st.x >= 0 && self.player.ga - st.x <= 32
                };
                if in_front && (st.y - self.player.gb).abs() < 16 {
                    let killed = st.apply_damage(2);
                    self.combo_hits += 1;
                    self.combo_timer_ms = 2000;
                    if killed {
                        // Death is logged by the caller via enemy_states.
                    }
                }
            }
        }
        let w = self.parsed.level.pixel_width() as i32;
        let h = self.parsed.level.pixel_height() as i32;
        self.player.tick(&self.player_anim, &self.flags, w, h, dir);
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
        let _ = enemies::draw_enemy_states(&g, fb, res, &self.enemy_states, self.player.cam_x, self.player.cam_y);
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
        let hud_state = hud::HudState {
            health: self.player.health,
            max_health: self.player.max_health,
            magic: 0,
            max_magic: 1024,
            gold: 0,
            combo_hits: self.combo_hits,
        };
        hud::draw_hud(&g, fb, res, font, &hud_state);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn gameplay_load_and_walk() {
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 9217, false).unwrap();
        assert_eq!(gp.player.ga, 48);
        assert_eq!(gp.player.gb, 560);
        gp.tick(&mut res, 1, false);
        assert!(gp.player.ga >= 48);
        assert_eq!(gp.player.cam_x, 0);
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
