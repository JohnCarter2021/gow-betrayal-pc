//! Enemy spawn (slice 3d-1): type 11/12/13 groups, as in
//! `e.d(int, int)` case 11/12/13.
//!
//! Extra layout per item, in read order:
//! [ec m3, et m10 (x1000), dL m1, en m16, em m16, el m10,
//!  dY m10, ej m1 (facing = != 1), ev m1]
//! plus for type 13: [bl3 m1, bl2 m1].
//! Sprite/anim sets (bC = 0 -> no palette key):
//! - type 11 (dQ 1): sprite 4097 / anim 4096, or 18433 / 18432 if dA.
//! - type 12 (dQ 2): sprite 17409 / anim 17408, or 16385 / 16384 if dB.
//! - type 13 (dQ 3): sprite 15361 / anim 15360, except the
//!   special pair: bl3 && !bl2 -> sprite 13313 / anim 13312 (dM),
//!   bl2 -> sprite 14337 / anim 14336 (dN).
//! Initial action = 1 if ev, else 0 (the `n23` passed to `i()`).
//! Movement, combat and health changes are NOT in this chunk;
//! enemies are drawn at spawn with their initial action, in
//! `r(Graphics)` order: after static sprites, before the player.

use crate::anim;
use crate::entities::LevelEntities;
use crate::midp::{draw_sprite_frame, Framebuffer, Graphics};
use crate::resources::Resources;

#[derive(Debug, Clone)]
pub struct Enemy {
    pub index: usize,
    pub kind: u32,
    pub x: i32,
    pub y: i32,
    pub facing_right: bool,
    pub health: i32,
    /// Patrol flag `ev` (extra[8]): these enemies walk, others idle.
    pub ev: bool,
    /// Base damage `el` (extra[5]): player damage = el<<1
    /// (action 5) or el<<2 (action 6) for a normal enemy.
    pub el: i32,
    pub anim_id: u32,
    pub sprite_id: u32,
    pub action: usize,
}

pub fn enemies(parsed: &LevelEntities) -> Vec<Enemy> {
    let mut out = Vec::new();
    for group in &parsed.groups {
        if group.kind != 11 && group.kind != 12 && group.kind != 13 {
            continue;
        }
        let stride = if group.kind == 13 { 11 } else { 9 };
        for (p, &(x, y)) in group.positions.iter().enumerate() {
            let e = &group.extra[p * stride..p * stride + stride];
            let ev = e[8] == 1;
            let (anim_id, sprite_id) = match group.kind {
                11 => {
                    if parsed.level.da {
                        (18432, 18433)
                    } else {
                        (4096, 4097)
                    }
                }
                12 => {
                    if parsed.level.db {
                        (16384, 16385)
                    } else {
                        (17408, 17409)
                    }
                }
                _ => {
                    if group.kind == 13 && e[9] == 1 && e[10] != 1 {
                        (13312, 13313)
                    } else if group.kind == 13 && e[10] == 1 {
                        (14336, 14337)
                    } else {
                        (15360, 15361)
                    }
                }
            };
            out.push(Enemy {
                index: out.len(),
                kind: group.kind,
                x,
                y,
                facing_right: e[7] != 1,
                health: e[3] as i32,
                ev,
                el: e[5] as i32,
                anim_id,
                sprite_id,
                action: if ev { 1 } else { 0 },
            });
        }
    }
    out
}

/// Draw all enemies at spawn, clipped by the framebuffer.
/// Returns (drawn, skipped) counts; a missing anim/sprite chunk
/// is skipped, not fatal, so one bad set cannot hide the rest.
pub fn draw_enemies(
    g: &Graphics,
    fb: &mut Framebuffer,
    res: &mut Resources,
    list: &[Enemy],
    cam_x: i32,
    cam_y: i32,
) -> (usize, usize) {
    let mut drawn = 0;
    let mut skipped = 0;
    for enemy in list {
        let Ok(anim_chunk) = res.chunk(enemy.anim_id) else {
            skipped += 1;
            continue;
        };
        let Ok(anim_set) = anim::load_anim_set(&anim_chunk) else {
            skipped += 1;
            continue;
        };
        let Ok(sprite_set) = res.sprite_set(enemy.sprite_id, None) else {
            skipped += 1;
            continue;
        };
        let transform = if enemy.facing_right { 0 } else { 2 };
        let Ok(frames) = anim::frames_for_timeline_frame(&anim_set, enemy.action, 0, transform)
        else {
            skipped += 1;
            continue;
        };
        for fr in frames {
            if let Some(frame) = sprite_set.frame(fr.sprite_idx) {
                draw_sprite_frame(
                    g,
                    fb,
                    &sprite_set.image,
                    frame,
                    enemy.x + fr.dx - cam_x,
                    enemy.y + fr.dy - cam_y,
                    fr.transform,
                );
            }
        }
        drawn += 1;
    }
    (drawn, skipped)
}

#[cfg(test)]
mod tests {
    #[test]
    fn enemy_spawn_9217() {
        let data = std::fs::read("../extracted/RP9").unwrap();
        let pack = crate::rp::parse_pack(&data).unwrap();
        let chunk = pack.chunk(1).unwrap();
        let parsed = crate::entities::parse_level_entities(9217, &chunk).unwrap();
        assert!(parsed.level.da, "9217 should select the dA enemy set");
        let list = super::enemies(&parsed);
        assert_eq!(list.len(), 9);
        assert_eq!(list[0].x, 816);
        assert_eq!(list[0].y, 528);
        assert_eq!(list[0].anim_id, 18432);
        assert_eq!(list[0].sprite_id, 18433);
        // The single type-13 enemy is the special dM pair.
        assert_eq!(list[8].kind, 13);
        assert_eq!(list[8].anim_id, 13312);
        assert_eq!(list[8].sprite_id, 13313);
        assert_eq!(list[8].health, 25);
    }
    #[test]
    fn enemy_chase_moves_toward_player() {
        let data = std::fs::read("../extracted/RP9").unwrap();
        let pack = crate::rp::parse_pack(&data).unwrap();
        let chunk = pack.chunk(1).unwrap();
        let parsed = crate::entities::parse_level_entities(9217, &chunk).unwrap();
        let flags = crate::player::TileFlags::build(&parsed);
        let list = super::enemies(&parsed);
        let enemy = list.iter().find(|e| e.x == 336).unwrap().clone();
        let anim_data = std::fs::read("../extracted/RP18").unwrap();
        let anim_pack = crate::rp::parse_pack(&anim_data).unwrap();
        let anim = crate::anim::load_anim_set(&anim_pack.chunk(0).unwrap()).unwrap();
        let mut st = super::EnemyState::new(enemy, &anim);
        // Player 100 px to the right: within chase range (144).
        // 20 ticks stay outside attack range (<48 px).
        for _ in 0..20 {
            st.tick(&anim, &flags, 436, 560);
        }
        assert!(st.x > 336, "enemy should walk right toward player, got {}", st.x);
        assert_eq!(st.action, 1);
        assert!(st.facing_right);
    }
    #[test]
    fn enemy_dies_after_enough_hits() {
        let data = std::fs::read("../extracted/RP9").unwrap();
        let pack = crate::rp::parse_pack(&data).unwrap();
        let chunk = pack.chunk(1).unwrap();
        let parsed = crate::entities::parse_level_entities(9217, &chunk).unwrap();
        let list = super::enemies(&parsed);
        let enemy = list.iter().find(|e| e.x == 336).unwrap().clone();
        assert_eq!(enemy.health, 3);
        let anim_data = std::fs::read("../extracted/RP18").unwrap();
        let anim_pack = crate::rp::parse_pack(&anim_data).unwrap();
        let anim = crate::anim::load_anim_set(&anim_pack.chunk(0).unwrap()).unwrap();
        let mut st = super::EnemyState::new(enemy, &anim);
        assert!(!st.apply_damage(2));
        assert_eq!(st.health, 1);
        assert!(st.apply_damage(2));
        assert!(st.dead);
    }
}

/// A live enemy (slice 3d-2): position, facing and animation slot.
/// Movement subset (3d-2): chase + patrol, no combat.
/// Chase (type-1 AI): within 144 px horizontally / 32 px
/// vertically of the player, face the player and walk
/// (action 1, root motion from the anim set); within 48/16 px
/// the enemy stops (attack is 3d-3, not yet ported).
/// Patrol fallback: `ev` enemies walk when the player is far,
/// flipping at spawn +/- 64 px or at a solid tile ahead;
/// non-`ev` enemies idle (action 0) when the player is far.
/// NOT yet ported: the idle->walk `eu` timer, gravity, or the
/// `aY`/`aZ` script tables (type 6). Note on the patrol flip:
/// the decompiled `ev` check literally reads `diff > 64 ->
/// left, diff < 64 -> right`, which would jitter at spawn+64;
/// the symmetric -64/+64 rule used here is the sane patrol
/// reading and is flagged for verification against the real game.
pub struct EnemyState {
    pub enemy: Enemy,
    pub x: i32,
    pub y: i32,
    pub facing_right: bool,
    pub action: usize,
    pub slot: anim::AnimSlot,
    /// Current health `ek` (starts at `en`, extra[3]).
    pub health: i32,
    pub dead: bool,
    pub attack_hit_done: bool,
    attack_count: usize,
    /// Damage per hit this enemy deals, `el` = extra[5].
    pub el: i32,
}

impl EnemyState {
    pub fn new(enemy: Enemy, anim_set: &anim::AnimSet) -> EnemyState {
        let slot = anim::AnimSlot::new(anim_set, enemy.action);
        let health = enemy.health;
        let el = enemy.el;
        EnemyState {
            x: enemy.x,
            y: enemy.y,
            facing_right: enemy.facing_right,
            action: enemy.action,
            slot,
            health,
            dead: false,
            attack_hit_done: false,
            attack_count: 0,
            el,
            enemy,
        }
    }

    /// Damage from a player hit: `fI[0] << gP` = 2 for the
    /// first combo step (gG = 0, gP = 0). Returns true if this
    /// hit killed the enemy. Stagger/death animation not yet
    /// ported: a dead enemy is simply removed from drawing.
    pub fn apply_damage(&mut self, amount: i32) -> bool {
        if self.dead {
            return false;
        }
        self.health -= amount;
        if self.health <= 0 {
            self.dead = true;
            return true;
        }
        false
    }

    /// One tick. Returns damage dealt to the player this tick
    /// (0 if no enemy attack hit landed).
    pub fn tick(
        &mut self,
        anim_set: &anim::AnimSet,
        flags: &crate::player::TileFlags,
        player_x: i32,
        player_y: i32,
    ) -> i32 {
        if self.dead {
            return 0;
        }
        // Attack swing in progress: advance, apply its hit once
        // at frame >= 2, then return to idle when finished.
        if self.action == 5 || self.action == 6 {
            self.slot.advance(anim_set, 40);
            let mut damage = 0;
            if self.slot.frame >= 2 && !self.attack_hit_done {
                self.attack_hit_done = true;
                // el<<1 for action 5, el<<2 for action 6 (normal
                // enemy, not dM): see the damage formula in e.java.
                damage = if self.action == 6 {
                    self.el << 2
                } else {
                    self.el << 1
                };
            }
            if self.slot.repeat == 0 {
                self.action = 0;
                self.slot = anim::AnimSlot::new(anim_set, 0);
            }
            return damage;
        }
        let dx_player = player_x - self.x;
        let dy_player = player_y - self.y;
        let near_attack = dx_player.abs() < 48 && dy_player.abs() < 16;
        let near_chase = dx_player.abs() < 144 && dy_player.abs() < 32;
        // Attack trigger (type-1 AI: |N&3|==3 -> action 6, else 5;
        // approximated here as every 4th attack being action 6).
        if near_attack {
            self.attack_count += 1;
            self.action = if self.attack_count % 4 == 0 { 6 } else { 5 };
            self.slot = anim::AnimSlot::new(anim_set, self.action);
            self.attack_hit_done = false;
            if dx_player > 0 {
                self.facing_right = true;
            } else if dx_player < 0 {
                self.facing_right = false;
            }
            return 0;
        }
        let walk = if near_chase {
            if dx_player > 16 {
                self.facing_right = true;
            } else if dx_player < -16 {
                self.facing_right = false;
            }
            true
        } else if self.enemy.ev {
            // Patrol fallback for ev enemies when the player is far.
            let diff = self.x - self.enemy.x;
            if diff > 64 {
                self.facing_right = false;
            } else if diff < -64 {
                self.facing_right = true;
            }
            true
        } else {
            false
        };
        if !walk {
            if self.action != 0 {
                self.action = 0;
                self.slot = anim::AnimSlot::new(anim_set, 0);
            }
            self.slot.advance(anim_set, 40);
            return 0;
        }
        if self.action != 1 {
            self.action = 1;
            self.slot = anim::AnimSlot::new(anim_set, 1);
        }
        let (dx, _dy) = self.slot.advance(anim_set, 40);
        let step = dx.abs();
        let gw = self.x / 16;
        let gy = (self.y - 1) / 16;
        let ahead = if self.facing_right { gw + 1 } else { gw - 1 };
        let blocked = flags.solid(ahead, gy)
            || flags.solid(ahead, gy - 1)
            || flags.solid(ahead, gy - 2);
        if blocked {
            self.facing_right = !self.facing_right;
        }
        self.x += if self.facing_right { step } else { -step };
        0
    }
}

/// Draw live enemies, like `draw_enemies` but from state.
pub fn draw_enemy_states(
    g: &Graphics,
    fb: &mut Framebuffer,
    res: &mut Resources,
    states: &[EnemyState],
    cam_x: i32,
    cam_y: i32,
) -> (usize, usize) {
    let mut drawn = 0;
    let mut skipped = 0;
    for st in states {
        if st.dead {
            skipped += 1;
            continue;
        }
        let Ok(anim_chunk) = res.chunk(st.enemy.anim_id) else {
            skipped += 1;
            continue;
        };
        let Ok(anim_set) = anim::load_anim_set(&anim_chunk) else {
            skipped += 1;
            continue;
        };
        let Ok(sprite_set) = res.sprite_set(st.enemy.sprite_id, None) else {
            skipped += 1;
            continue;
        };
        let transform = if st.facing_right { 0 } else { 2 };
        let Ok(frames) =
            anim::frames_for_timeline_frame(&anim_set, st.action, st.slot.visual, transform)
        else {
            skipped += 1;
            continue;
        };
        for fr in frames {
            if let Some(frame) = sprite_set.frame(fr.sprite_idx) {
                draw_sprite_frame(
                    g,
                    fb,
                    &sprite_set.image,
                    frame,
                    st.x + fr.dx - cam_x,
                    st.y + fr.dy - cam_y,
                    fr.transform,
                );
            }
        }
        drawn += 1;
    }
    (drawn, skipped)
}
