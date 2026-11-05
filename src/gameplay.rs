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
        // Script triggers (type 8): positions are pixels,
        // tiles are pos/16; extras are (cn, co) pairs.
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
            intro_index,
            intro_text,
            block_held: false,
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
    pub fn jump(&mut self) -> bool {
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

    /// Cancel the active script (Exit key).
    pub fn script_cancel(&mut self) {
        self.script.cancel();
    }

    fn handle_script_event(&mut self, ev: ScriptEvent) {
        if ev == ScriptEvent::LevelComplete {
            self.level_complete = Some(self.level_index + 1);
        }
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
        // Block stance (slice 18): Down held starts
        // it from idle/walk (or resumes it after a
        // blocked-hit reaction ends in idle);
        // releasing Down ends it. Attacking or
        // airborne presses do not start it.
        if self.block_held {
            if !self.player.blocking || self.player.action == 0 {
                self.player.start_block(&self.player_anim);
            }
        } else if self.player.blocking {
            self.player.stop_block(&self.player_anim);
        }
        if attack && !self.player.blocking {
            // `A()` (slice 17): Artemis with no magic
            // left unequips instead of swinging.
            if self.player.is_artemis() && self.magic <= 0 {
                self.player.unequip_all();
            } else {
                self.player.attack(&self.player_anim);
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
        self.player.tick(&self.player_anim, &self.flags, w, h, dir);
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
        // Fire does nothing in a magic mode.
        gp.player.weapon_mode = 2;
        assert!(!gp.player.attack(&gp.player_anim));
        assert_eq!(gp.player.action, 0);
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




