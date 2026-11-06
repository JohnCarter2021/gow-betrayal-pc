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
    /// Orb value `em` (extra[4]): the value passed to
    /// the pickup spawner `b()` when this enemy dies.
    pub em: i32,
    pub anim_id: u32,
    pub sprite_id: u32,
    pub action: usize,
    /// Cutscene target `dL` (Phase 4): spawn bit
    /// extra[2] — this enemy is the `!` cutscene's
    /// aZ-table actor.
    pub cutscene_target: bool,
    /// Respawn-table index `dY` (Phase 4, extra[6]):
    /// indexes the type-14 trigger's entries.
    pub dy: i32,
    /// Respawn counter `ec` (Phase 7, extra[0],
    /// m(3)): how many times this enemy may respawn
    /// after dying (reset to `ed` by a flag-4
    /// trigger). Arena (`iN`) ignores it — waves
    /// are endless there.
    pub ec: i32,
    /// Respawn interval `et` (Phase 7, extra[1],
    /// m(10) * 1000 ms): the countdown between the
    /// enemy's death and its respawn.
    pub et: i32,
    /// Main special enemy `dM` (dS, e.java:2827):
    /// a kind-13 enemy whose extra[9] (`bl3`) is
    /// set — or whose extra[10] is set (that one
    /// is both `dM` and `dN`). Its health (`ek`
    /// and `en`) is quadrupled at load, and it
    /// deals half damage (see the attack branch).
    pub is_dm: bool,
    /// Second special enemy `dN` (dS, e.java:2834):
    /// a kind-13 enemy whose extra[10] (`bl2`) is
    /// set — it is also `dM`. The special-attack
    /// trigger never fires for it (`dN != n9`,
    /// e.java:6303).
    pub is_dn: bool,
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
            // dM/dN (dS, e.java:2827-2835): kind 13
            // extra[9] (`bl3`) marks the main enemy
            // `dM`; extra[10] (`bl2`) marks `dN` and
            // also `dM`. Either one quadruples the
            // enemy's health (`ek <<= 2; en = ek`)
            // at load — these are the elite/boss
            // enemies (e.g. 9217's kind-13 at
            // (1408,560) has 25 -> 100).
            let is_dm = group.kind == 13 && (e[9] == 1 || e[10] == 1);
            let is_dn = group.kind == 13 && e[10] == 1;
            let health = if is_dm {
                (e[3] as i32) << 2
            } else {
                e[3] as i32
            };
            out.push(Enemy {
                index: out.len(),
                kind: group.kind,
                x,
                y,
                facing_right: e[7] != 1,
                health,
                ev,
                el: e[5] as i32,
                em: e[4] as i32,
                anim_id,
                sprite_id,
                action: if ev { 1 } else { 0 },
                cutscene_target: e[2] == 1,
                dy: e[6] as i32,
                ec: e[0] as i32,
                et: e[1] as i32 * 1000,
                is_dm,
                is_dn,
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
    /// Tile flags for 9217 with the slice-20 footprints.
    fn flags_9217(parsed: &crate::entities::LevelEntities) -> crate::player::TileFlags {
        let mut res = crate::resources::Resources::new("../extracted");
        let footprints = crate::entities::load_footprints(&mut res, parsed).unwrap();
        crate::player::TileFlags::build(parsed, &footprints)
    }

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
        // dM (dS, e.java:2827-2831): extra[9] set,
        // so health is quadrupled (25 -> 100) and
        // it is the main enemy, not the dN one.
        assert!(list[8].is_dm, "9217 kind-13 is dM");
        assert!(!list[8].is_dn, "9217 kind-13 is not dN");
        assert_eq!(list[8].health, 100);
    }
    #[test]
    fn enemy_chase_moves_toward_player() {
        let data = std::fs::read("../extracted/RP9").unwrap();
        let pack = crate::rp::parse_pack(&data).unwrap();
        let chunk = pack.chunk(1).unwrap();
        let parsed = crate::entities::parse_level_entities(9217, &chunk).unwrap();
        let flags = flags_9217(&parsed);
        let list = super::enemies(&parsed);
        let enemy = list.iter().find(|e| e.x == 336).unwrap().clone();
        let anim_data = std::fs::read("../extracted/RP18").unwrap();
        let anim_pack = crate::rp::parse_pack(&anim_data).unwrap();
        let anim = crate::anim::load_anim_set(&anim_pack.chunk(0).unwrap()).unwrap();
        let mut rng = crate::pickups::OrbRng(1);
        let mut st = super::EnemyState::new(enemy, &anim);
        // Player 100 px to the right: within chase range (144).
        // The `eu` timer (Phase 4) makes the enemy pause
        // between walks, so run 60 ticks and assert it
        // has moved right overall (not every tick).
        for _ in 0..60 {
            st.tick(&anim, &flags, 436, 560, &mut rng);
        }
        assert!(st.x > 336, "enemy should walk right toward player, got {}", st.x);
    }
    #[test]
    fn eu_timer_idles_before_walking() {
        // Phase 4: the `eu` idle-to-walk timer. An enemy
        // in chase range with `eu >= 0` stays idle and
        // `eu` drops by 40 per tick; once `eu < 0` it
        // walks and `eu` re-randomises to 0..=511.
        let data = std::fs::read("../extracted/RP9").unwrap();
        let pack = crate::rp::parse_pack(&data).unwrap();
        let chunk = pack.chunk(1).unwrap();
        let parsed = crate::entities::parse_level_entities(9217, &chunk).unwrap();
        let flags = flags_9217(&parsed);
        let list = super::enemies(&parsed);
        let enemy = list.iter().find(|e| e.x == 336).unwrap().clone();
        let anim_data = std::fs::read("../extracted/RP18").unwrap();
        let anim_pack = crate::rp::parse_pack(&anim_data).unwrap();
        let anim = crate::anim::load_anim_set(&anim_pack.chunk(0).unwrap()).unwrap();
        let mut rng = crate::pickups::OrbRng(1);
        let mut st = super::EnemyState::new(enemy, &anim);
        // Set a large eu so the enemy must idle first.
        st.eu = 200;
        // Player in chase range (100 px right).
        for _ in 0..5 {
            st.tick(&anim, &flags, 436, 560, &mut rng);
        }
        // After 5 idle ticks eu dropped 200 -> 0 and the
        // enemy has not moved.
        assert_eq!(st.eu, 0);
        assert_eq!(st.x, 336);
        assert_eq!(st.action, 0);
        // One more tick: eu 0 -> -40, still idle (eu < 0
        // only gates the NEXT tick's walk).
        st.tick(&anim, &flags, 436, 560, &mut rng);
        assert_eq!(st.eu, -40);
        assert_eq!(st.action, 0);
        // Now eu < 0, so it walks and eu re-randomises.
        st.tick(&anim, &flags, 436, 560, &mut rng);
        assert_eq!(st.action, 1);
        assert!(st.x > 336, "enemy should walk after eu expires, got {}", st.x);
        assert!(st.eu >= 0 && st.eu <= 511, "eu re-randomised to 0..=511, got {}", st.eu);
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
        let mut rng = crate::pickups::OrbRng(1);
        let mut st = super::EnemyState::new(enemy, &anim);
        assert!(!st.apply_damage(&anim, 2, false));
        assert_eq!(st.health, 1);
        assert!(st.apply_damage(&anim, 2, false));
        // Killing blow starts the death animation (kind 11
        // -> action 8, ~1.6 s), not instant removal.
        assert!(st.dying);
        assert!(!st.dead);
        assert_eq!(st.action, 8);
        // Further hits are ignored while dying.
        assert!(!st.apply_damage(&anim, 2, false));
        // Advance until the animation finishes.
        let flags = flags_9217(&parsed);
        for _ in 0..100 {
            st.tick(&anim, &flags, 436, 560, &mut rng);
            if st.dead {
                break;
            }
        }
        assert!(st.dead, "death animation never finished");
        assert!(!st.dying);
    }

    #[test]
    fn enemy_staggers_on_nonlethal_hit_then_recovers() {
        let data = std::fs::read("../extracted/RP9").unwrap();
        let pack = crate::rp::parse_pack(&data).unwrap();
        let chunk = pack.chunk(1).unwrap();
        let parsed = crate::entities::parse_level_entities(9217, &chunk).unwrap();
        let list = super::enemies(&parsed);
        let enemy = list.iter().find(|e| e.x == 448).unwrap().clone();
        assert_eq!(enemy.health, 13);
        let anim_data = std::fs::read("../extracted/RP18").unwrap();
        let anim_pack = crate::rp::parse_pack(&anim_data).unwrap();
        let anim = crate::anim::load_anim_set(&anim_pack.chunk(0).unwrap()).unwrap();
        let mut rng = crate::pickups::OrbRng(1);
        let mut st = super::EnemyState::new(enemy, &anim);
        assert!(!st.apply_damage(&anim, 2, false));
        assert_eq!(st.health, 11);
        assert!(!st.dead && !st.dying);
        assert_eq!(st.action, 2, "kind 11 staggers with action 2");
        // While staggering, the enemy neither attacks nor
        // moves, even with the player in attack range.
        let flags = flags_9217(&parsed);
        let dmg = st.tick(&anim, &flags, 458, 560, &mut rng);
        assert_eq!(dmg, 0);
        assert_eq!(st.action, 2);
        // Action 2 runs ~600 ms = 15 ticks; it must end
        // back at idle, alive.
        for _ in 0..30 {
            st.tick(&anim, &flags, 458, 560, &mut rng);
            if st.action == 0 {
                break;
            }
        }
        assert_eq!(st.action, 0, "stagger never finished");
        assert!(!st.dead);
    }

    #[test]
    fn heavy_stagger_variants_and_slide() {
        // Phase 6: a finisher (heavy) selects the
        // heavy stagger variants — kind 11 -> 7,
        // kind 12 -> 9, kind 13 -> 8 — and the
        // stagger slides the enemy backward (set
        // 18432 action 2 totals dx -26).
        let data = std::fs::read("../extracted/RP9").unwrap();
        let pack = crate::rp::parse_pack(&data).unwrap();
        let chunk = pack.chunk(1).unwrap();
        let parsed = crate::entities::parse_level_entities(9217, &chunk).unwrap();
        let list = super::enemies(&parsed);
        let anim_data = std::fs::read("../extracted/RP18").unwrap();
        let anim_pack = crate::rp::parse_pack(&anim_data).unwrap();
        let anim = crate::anim::load_anim_set(&anim_pack.chunk(0).unwrap()).unwrap();
        let mut rng = crate::pickups::OrbRng(1);
        let flags = flags_9217(&parsed);
        // Kind 11 (x=448): normal -> 2, heavy -> 7.
        let enemy = list.iter().find(|e| e.x == 448).unwrap().clone();
        assert_eq!(enemy.kind, 11);
        let mut st = super::EnemyState::new(enemy, &anim);
        assert!(!st.apply_damage(&anim, 2, false));
        assert_eq!(st.action, 2);
        let mut st = super::EnemyState::new(
            list.iter().find(|e| e.x == 448).unwrap().clone(),
            &anim,
        );
        assert!(!st.apply_damage(&anim, 2, true));
        assert_eq!(st.action, 7, "kind 11 heavy stagger is 7");
        // The stagger slides the enemy backward
        // (away from its facing): x moves.
        let x0 = st.x;
        for _ in 0..30 {
            st.tick(&anim, &flags, 458, 560, &mut rng);
            if st.action == 0 {
                break;
            }
        }
        assert_eq!(st.action, 0, "heavy stagger never finished");
        assert_ne!(st.x, x0, "stagger should slide the enemy");
        // Kind 13 (x=1408): normal -> 4, heavy -> 8.
        let enemy13 = list.iter().find(|e| e.x == 1408).unwrap().clone();
        assert_eq!(enemy13.kind, 13);
        let mut st13 = super::EnemyState::new(enemy13, &anim);
        assert!(!st13.apply_damage(&anim, 2, false));
        assert_eq!(st13.action, 4);
        let mut st13 = super::EnemyState::new(
            list.iter().find(|e| e.x == 1408).unwrap().clone(),
            &anim,
        );
        assert!(!st13.apply_damage(&anim, 2, true));
        assert_eq!(st13.action, 8, "kind 13 heavy stagger is 8");
        // Phase 8: a kind-13 enemy playing action 12
        // when hit staggers with 13 (e.java:6463),
        // regardless of heavy.
        let mut st13 = super::EnemyState::new(
            list.iter().find(|e| e.x == 1408).unwrap().clone(),
            &anim,
        );
        st13.action = 12;
        st13.slot = crate::anim::AnimSlot::new(&anim, 12);
        assert!(!st13.apply_damage(&anim, 2, false));
        assert_eq!(st13.action, 13, "kind 13 in action 12 staggers with 13");
        let mut st13 = super::EnemyState::new(
            list.iter().find(|e| e.x == 1408).unwrap().clone(),
            &anim,
        );
        st13.action = 12;
        st13.slot = crate::anim::AnimSlot::new(&anim, 12);
        assert!(!st13.apply_damage(&anim, 2, true));
        assert_eq!(st13.action, 13, "kind 13 in action 12 staggers with 13 (heavy too)");
    }

    #[test]
    fn flag_2_tile_turns_a_walking_enemy() {
        // Phase 8: flag 2 is an enemy barrier
        // (e.java:6948) — a grounded walking enemy
        // will not step into a flag-2 tile; it
        // turns instead. Set flag 2 on the tile
        // ahead of a walking enemy and it should
        // reverse; without the flag it walks on.
        let data = std::fs::read("../extracted/RP9").unwrap();
        let pack = crate::rp::parse_pack(&data).unwrap();
        let chunk = pack.chunk(1).unwrap();
        let parsed = crate::entities::parse_level_entities(9217, &chunk).unwrap();
        let list = super::enemies(&parsed);
        let anim_data = std::fs::read("../extracted/RP18").unwrap();
        let anim_pack = crate::rp::parse_pack(&anim_data).unwrap();
        let anim = crate::anim::load_anim_set(&anim_pack.chunk(0).unwrap()).unwrap();
        let mut flags = flags_9217(&parsed);
        let mut rng = crate::pickups::OrbRng(1);
        // The 448 enemy walks right on flat ground
        // (foot tile y=34, x=28); put flag 2 on the
        // tile ahead (x=29, y=34). The player is
        // placed 100 px right so the enemy chases
        // (near_chase, |dx| < 144).
        let enemy = list.iter().find(|e| e.x == 448).unwrap().clone();
        let mut st = super::EnemyState::new(enemy, &anim);
        st.action = 1;
        st.facing_right = true;
        st.slot = crate::anim::AnimSlot::new(&anim, 1);
        let x0 = st.x;
        // No flag: walks right.
        for _ in 0..10 {
            st.tick(&anim, &flags, 548, 560, &mut rng);
            if !st.facing_right {
                break;
            }
        }
        assert!(st.x > x0, "no barrier: enemy walks right");
        // With flag 2 ahead: turns.
        let mut st = super::EnemyState::new(
            list.iter().find(|e| e.x == 448).unwrap().clone(),
            &anim,
        );
        st.action = 1;
        st.facing_right = true;
        st.slot = crate::anim::AnimSlot::new(&anim, 1);
        flags.set_flag(29, 34, 2);
        let mut turned = false;
        for _ in 0..10 {
            st.tick(&anim, &flags, 548, 560, &mut rng);
            if !st.facing_right {
                turned = true;
                break;
            }
        }
        assert!(turned, "flag-2 tile turns the enemy");
    }

    #[test]
    fn dx_decays_per_tick() {
        // Phase 6: dX decays by `40 + max_health/64`
        // per tick while positive (e.java:6627-6630).
        // The 448 enemy has max health 13, so the
        // decay is 40 per tick.
        let data = std::fs::read("../extracted/RP9").unwrap();
        let pack = crate::rp::parse_pack(&data).unwrap();
        let chunk = pack.chunk(1).unwrap();
        let parsed = crate::entities::parse_level_entities(9217, &chunk).unwrap();
        let list = super::enemies(&parsed);
        let anim_data = std::fs::read("../extracted/RP18").unwrap();
        let anim_pack = crate::rp::parse_pack(&anim_data).unwrap();
        let anim = crate::anim::load_anim_set(&anim_pack.chunk(0).unwrap()).unwrap();
        let mut rng = crate::pickups::OrbRng(1);
        let flags = flags_9217(&parsed);
        let enemy = list.iter().find(|e| e.x == 448).unwrap().clone();
        let mut st = super::EnemyState::new(enemy, &anim);
        assert_eq!(st.max_health, 13);
        st.dx = 100;
        st.tick(&anim, &flags, 9000, 560, &mut rng);
        assert_eq!(st.dx, 60);
        st.tick(&anim, &flags, 9000, 560, &mut rng);
        assert_eq!(st.dx, 20);
        st.tick(&anim, &flags, 9000, 560, &mut rng);
        assert!(st.dx <= 0, "dX should reach 0, got {}", st.dx);
    }

    #[test]
    fn weakened_marker_rule() {
        use super::weakened_marker_visible as vis;
        // Full health: no marker. At exactly half:
        // no marker (ek < en>>1 is strict).
        assert!(!vis(13, 13, false, false, 0));
        assert!(!vis(6, 13, false, false, 0));
        // Below half: marker. Dead/dying: none, even
        // with low health. Negative health: none.
        assert!(vis(5, 13, false, false, 0));
        assert!(!vis(5, 13, true, false, 0));
        assert!(!vis(5, 13, false, true, 0));
        assert!(!vis(-1, 13, false, false, 0));
        // Staggered (dX > 0): no weakened marker
        // (Phase 6, e.java:2571 — the slot-137
        // indicator shows instead).
        assert!(!vis(5, 13, false, false, 100));
        assert!(vis(5, 13, false, false, -40));
    }

    #[test]
    fn enemy_bbox_height_is_positive() {
        // Kind-11 enemy (set 18432, sprite 18433),
        // initial action 0: ef must be a real height.
        let mut res = crate::resources::Resources::new("../extracted");
        let anim_chunk = res.chunk(18432).unwrap();
        let anim = crate::anim::load_anim_set(&anim_chunk).unwrap();
        let sprites = res.sprite_set(18433, None).unwrap();
        let ef = super::enemy_bbox_height(&anim, &sprites, 0);
        assert!(ef > 16 && ef < 128, "ef = {ef}");
    }

    // ---- dS (enemy special attacks) ----

    fn anim_18432() -> crate::anim::AnimSet {
        let data = std::fs::read("../extracted/RP18").unwrap();
        let pack = crate::rp::parse_pack(&data).unwrap();
        crate::anim::load_anim_set(&pack.chunk(0).unwrap()).unwrap()
    }

    #[test]
    fn ds_transitions_by_action_and_kind() {
        // e.java:4314 (`i()`): `dS` is maintained
        // from the kind class and the new action.
        let anim = anim_18432();
        let data = std::fs::read("../extracted/RP9").unwrap();
        let pack = crate::rp::parse_pack(&data).unwrap();
        let chunk = pack.chunk(1).unwrap();
        let parsed = crate::entities::parse_level_entities(9217, &chunk).unwrap();
        let list = super::enemies(&parsed);
        // Kind 11 (x=336).
        let mut st = super::EnemyState::new(
            list.iter().find(|e| e.x == 336).unwrap().clone(),
            &anim,
        );
        assert_eq!(st.ds, -1);
        st.set_action(&anim, 5);
        assert_eq!(st.ds, 0);
        st.set_action(&anim, 6);
        assert_eq!(st.ds, 0);
        st.dx = 100;
        st.set_action(&anim, 8);
        assert_eq!(st.ds, 1);
        assert_eq!(st.dx, 0, "dS 1 resets dX");
        st.set_action(&anim, 2);
        assert_eq!(st.ds, 2);
        st.set_action(&anim, 11);
        assert_eq!(st.ds, 3);
        st.set_action(&anim, 3);
        assert_eq!(st.ds, 3);
        st.set_action(&anim, 0);
        assert_eq!(st.ds, -1);
        // Kind 13 (the 9217 dM at x=1408).
        let mut st13 = super::EnemyState::new(
            list.iter().find(|e| e.kind == 13).unwrap().clone(),
            &anim,
        );
        st13.set_action(&anim, 7);
        assert_eq!(st13.ds, 1);
        st13.set_action(&anim, 13);
        assert_eq!(st13.ds, 1);
        st13.set_action(&anim, 17);
        assert_eq!(st13.ds, 1);
        st13.set_action(&anim, 5);
        assert_eq!(st13.ds, -1);
        // Kind 12 (19456, first kind-12 enemy).
        let data = std::fs::read("../extracted/RP19").unwrap();
        let pack = crate::rp::parse_pack(&data).unwrap();
        let chunk = pack.chunk(0).unwrap();
        let parsed = crate::entities::parse_level_entities(19456, &chunk).unwrap();
        let list = super::enemies(&parsed);
        let mut st12 = super::EnemyState::new(
            list.iter().find(|e| e.kind == 12).unwrap().clone(),
            &anim,
        );
        st12.set_action(&anim, 7);
        assert_eq!(st12.ds, 1);
        st12.set_action(&anim, 12);
        assert_eq!(st12.ds, 1);
        st12.set_action(&anim, 5);
        assert_eq!(st12.ds, -1);
    }

    #[test]
    fn ei_eh_flags() {
        // e.java:4314: `ei` (kind 11 in 13/17/23/
        // 12/15/16/14/21) and `eh` (kind 11 in
        // 19/18/26/20) are pure functions of the
        // kind and the current action.
        let anim = anim_18432();
        let data = std::fs::read("../extracted/RP9").unwrap();
        let pack = crate::rp::parse_pack(&data).unwrap();
        let chunk = pack.chunk(1).unwrap();
        let parsed = crate::entities::parse_level_entities(9217, &chunk).unwrap();
        let list = super::enemies(&parsed);
        let mut st = super::EnemyState::new(
            list.iter().find(|e| e.x == 336).unwrap().clone(),
            &anim,
        );
        for a in [13, 17, 23, 12, 15, 16, 14, 21] {
            st.set_action(&anim, a);
            assert!(st.is_ei(), "action {a} is ei");
            assert!(!st.is_eh(), "action {a} is not eh");
        }
        for a in [19, 18, 26, 20] {
            st.set_action(&anim, a);
            assert!(st.is_eh(), "action {a} is eh");
            assert!(!st.is_ei(), "action {a} is not ei");
        }
        st.set_action(&anim, 5);
        assert!(!st.is_ei() && !st.is_eh(), "action 5 is neither");
        // Only kind 11 has ei/eh.
        let mut st13 = super::EnemyState::new(
            list.iter().find(|e| e.kind == 13).unwrap().clone(),
            &anim,
        );
        st13.set_action(&anim, 13);
        assert!(!st13.is_ei() && !st13.is_eh(), "kind 13 has no ei/eh");
    }

    #[test]
    fn ds_freeze_guard_during_grapple() {
        // e.java:4354: while a grapple is in
        // progress (and this is not the grappled
        // enemy), a non-`eh` enemy may not start
        // a new action other than 7 or 11 — an
        // idle enemy stays idle, a moving one is
        // forced to idle, and only the spawn-in
        // action is allowed through.
        let anim = anim_18432();
        let data = std::fs::read("../extracted/RP9").unwrap();
        let pack = crate::rp::parse_pack(&data).unwrap();
        let chunk = pack.chunk(1).unwrap();
        let parsed = crate::entities::parse_level_entities(9217, &chunk).unwrap();
        let list = super::enemies(&parsed);
        let mut st = super::EnemyState::new(
            list.iter().find(|e| e.x == 336).unwrap().clone(),
            &anim,
        );
        st.frozen = true;
        // Idle enemy: a new action is refused.
        st.set_action(&anim, 5);
        assert_eq!(st.action, 0, "frozen idle enemy stays idle");
        // Spawn-in is allowed through.
        st.set_action(&anim, 27);
        assert_eq!(st.action, 27, "frozen spawn-in is allowed");
        // A moving enemy is forced to idle.
        st.frozen = false;
        st.set_action(&anim, 1);
        assert_eq!(st.action, 1);
        st.frozen = true;
        st.set_action(&anim, 5);
        assert_eq!(st.action, 0, "frozen moving enemy is forced to idle");
        // Actions 7 and 11 are exempt.
        st.set_action(&anim, 11);
        assert_eq!(st.action, 11, "frozen action 11 is exempt");
        st.set_action(&anim, 7);
        assert_eq!(st.action, 7, "frozen action 7 is exempt");
    }

    #[test]
    fn dm_dn_flags_and_health() {
        // e.java:2827-2835: kind-13 extra[9] marks
        // `dM`, extra[10] marks `dN` (and also
        // `dM`); either quadruples the health.
        let data = std::fs::read("../extracted/RP9").unwrap();
        let pack = crate::rp::parse_pack(&data).unwrap();
        let chunk = pack.chunk(1).unwrap();
        let parsed = crate::entities::parse_level_entities(9217, &chunk).unwrap();
        let list = super::enemies(&parsed);
        let dm = list.iter().find(|e| e.kind == 13).unwrap();
        assert!(dm.is_dm && !dm.is_dn, "9217 kind-13 is dM, not dN");
        assert_eq!(dm.health, 100, "25 << 2");
        // 27648 has the only dN enemy.
        let data = std::fs::read("../extracted/RP27").unwrap();
        let pack = crate::rp::parse_pack(&data).unwrap();
        let chunk = pack.chunk(0).unwrap();
        let parsed = crate::entities::parse_level_entities(27648, &chunk).unwrap();
        let list = super::enemies(&parsed);
        let dn = list
            .iter()
            .find(|e| e.kind == 13 && e.anim_id == 14336)
            .expect("27648 has a dN enemy");
        assert!(dn.is_dm && dn.is_dn, "dN is also dM");
        assert_eq!(dn.health, 8000, "2000 << 2");
    }

    #[test]
    fn special_attack_trigger_fires_when_airborne() {
        // e.java:6303: an airborne enemy (the tile
        // at its feet is not solid), not `ei`/`eh`,
        // with `dS != 1`, in an allowed action,
        // that is not `dN`, starts its special
        // attack (11 kind 11).
        let anim = anim_18432();
        let data = std::fs::read("../extracted/RP9").unwrap();
        let pack = crate::rp::parse_pack(&data).unwrap();
        let chunk = pack.chunk(1).unwrap();
        let parsed = crate::entities::parse_level_entities(9217, &chunk).unwrap();
        let flags = flags_9217(&parsed);
        let list = super::enemies(&parsed);
        let mut rng = crate::pickups::OrbRng(1);
        let mut st = super::EnemyState::new(
            list.iter().find(|e| e.x == 336).unwrap().clone(),
            &anim,
        );
        // Lift it into the air (y=400, row 25 is
        // empty at tile 21) and idle it; the
        // player is far away so no normal attack
        // or chase fires.
        st.y = 400;
        assert!(!flags.solid(st.x / 16, st.y / 16), "test setup is airborne");
        st.set_action(&anim, 0);
        st.tick(&anim, &flags, 0, 0, &mut rng);
        assert_eq!(st.action, 11, "airborne enemy starts the special attack");
        assert_eq!(st.ds, 3, "action 11 sets dS 3");
    }

    #[test]
    fn special_attack_does_not_fire_when_grounded() {
        // The same enemy on the ground (y=560,
        // tile (21,35) solid) must not start its
        // special attack.
        let anim = anim_18432();
        let data = std::fs::read("../extracted/RP9").unwrap();
        let pack = crate::rp::parse_pack(&data).unwrap();
        let chunk = pack.chunk(1).unwrap();
        let parsed = crate::entities::parse_level_entities(9217, &chunk).unwrap();
        let flags = flags_9217(&parsed);
        let list = super::enemies(&parsed);
        let mut rng = crate::pickups::OrbRng(1);
        let mut st = super::EnemyState::new(
            list.iter().find(|e| e.x == 336).unwrap().clone(),
            &anim,
        );
        assert!(flags.solid(st.x / 16, st.y / 16), "test setup is grounded");
        st.set_action(&anim, 0);
        st.tick(&anim, &flags, 0, 0, &mut rng);
        assert_ne!(st.action, 11, "grounded enemy does not start the special attack");
    }

    #[test]
    fn special_attack_dives_and_lands() {
        // e.java:5867-5880 + 7044-7072: the special
        // attack dives (the anim's own dy plus the
        // accelerating `dJ`) and ends on landing
        // with the landing stagger (7 kind 11).
        let anim = anim_18432();
        let data = std::fs::read("../extracted/RP9").unwrap();
        let pack = crate::rp::parse_pack(&data).unwrap();
        let chunk = pack.chunk(1).unwrap();
        let parsed = crate::entities::parse_level_entities(9217, &chunk).unwrap();
        let flags = flags_9217(&parsed);
        let list = super::enemies(&parsed);
        let mut rng = crate::pickups::OrbRng(1);
        let mut st = super::EnemyState::new(
            list.iter().find(|e| e.x == 336).unwrap().clone(),
            &anim,
        );
        st.y = 400;
        st.set_action(&anim, 11);
        let y0 = st.y;
        let mut landed = false;
        for _ in 0..100 {
            st.tick(&anim, &flags, 0, 0, &mut rng);
            if st.action == 7 {
                landed = true;
                break;
            }
        }
        assert!(landed, "special attack lands (action 7), y={} action={}", st.y, st.action);
        assert!(st.y > y0, "it dove down from {y0} to {}", st.y);
        assert_eq!(st.y, 560, "it snaps to the ground tile top (row 35)");
        assert_eq!(st.ds, 2, "landing stagger 7 sets dS 2");
    }

    #[test]
    fn special_attack_deals_contact_damage() {
        // e.java:6600-6625: the special attack's
        // hitbox overlap deals el<<1 (a normal
        // enemy, not `dM`), once.
        let anim = anim_18432();
        let data = std::fs::read("../extracted/RP19").unwrap();
        let pack = crate::rp::parse_pack(&data).unwrap();
        let chunk = pack.chunk(0).unwrap();
        let parsed = crate::entities::parse_level_entities(19456, &chunk).unwrap();
        let flags = crate::player::TileFlags::build(
            &parsed,
            &crate::entities::load_footprints(
                &mut crate::resources::Resources::new("../extracted"),
                &parsed,
            )
            .unwrap(),
        );
        let list = super::enemies(&parsed);
        let mut rng = crate::pickups::OrbRng(1);
        let mut st = super::EnemyState::new(
            list.iter().find(|e| e.kind == 12).unwrap().clone(),
            &anim,
        );
        assert!(!st.enemy.is_dm, "kind-12 test enemy is not dM");
        // Airborne (lift it above its spawn) and
        // in the special attack; the player stands
        // 20 px to the right, inside the 32 px
        // contact range.
        st.y -= 64;
        st.set_action(&anim, 8);
        let el = st.el;
        let dmg = st.tick(&anim, &flags, st.x + 20, st.y, &mut rng);
        assert_eq!(dmg, el << 1, "special attack deals el<<1, got {dmg} (el={el})");
        // Only once.
        let dmg2 = st.tick(&anim, &flags, st.x + 20, st.y, &mut rng);
        assert_eq!(dmg2, 0, "contact damage lands once");
    }

    #[test]
    fn dm_deals_half_damage() {
        // e.java:6600-6609: the `dM != n9`
        // doubling does not apply to a `dM`
        // enemy — it deals el (action 5) / el<<1
        // (action 6) instead of el<<1 / el<<2.
        let anim = anim_18432();
        let data = std::fs::read("../extracted/RP9").unwrap();
        let pack = crate::rp::parse_pack(&data).unwrap();
        let chunk = pack.chunk(1).unwrap();
        let parsed = crate::entities::parse_level_entities(9217, &chunk).unwrap();
        let flags = flags_9217(&parsed);
        let list = super::enemies(&parsed);
        let mut rng = crate::pickups::OrbRng(1);
        let mut st = super::EnemyState::new(
            list.iter().find(|e| e.kind == 13).unwrap().clone(),
            &anim,
        );
        assert!(st.enemy.is_dm, "9217 kind-13 is dM");
        let el = st.el;
        // Action 5: hit at frame >= 2.
        st.set_action(&anim, 5);
        let mut dmg = 0;
        for _ in 0..5 {
            dmg = dmg.max(st.tick(&anim, &flags, st.x + 20, st.y, &mut rng));
        }
        assert_eq!(dmg, el, "dM action 5 deals el, got {dmg} (el={el})");
        // Action 6: el<<1.
        let mut st = super::EnemyState::new(
            list.iter().find(|e| e.kind == 13).unwrap().clone(),
            &anim,
        );
        st.set_action(&anim, 6);
        let mut dmg = 0;
        for _ in 0..5 {
            dmg = dmg.max(st.tick(&anim, &flags, st.x + 20, st.y, &mut rng));
        }
        assert_eq!(dmg, el << 1, "dM action 6 deals el<<1, got {dmg} (el={el})");
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
    /// Max health `en` (extra[3] at spawn).
    pub max_health: i32,
    /// Bbox height `ef` (P[3] from the spawn-time
    /// `a(set, slot, 0, 0, 0, P)` call): height of the
    /// initial action's timeline-0 composition. Set by
    /// the caller via `set_ef` once the enemy sprite
    /// set is loaded; 0 until then (marker falls back
    /// to the enemy's y in that case).
    pub ef: i32,
    /// Playing its death animation (action 8/7, see
    /// `death_action`): still drawn, but takes no more
    /// hits and deals no damage. `dead` is set only when
    /// the animation finishes.
    pub dying: bool,
    pub dead: bool,
    pub attack_hit_done: bool,
    attack_count: usize,
    /// Damage per hit this enemy deals, `el` = extra[5].
    pub el: i32,
    /// Idle-to-walk timer `eu` (Phase 4): when the
    /// enemy is in chase range but idle, it waits
    /// until `eu < 0` before walking; `eu` drops by
    /// 40 per tick while idle, and is re-randomised
    /// to 0..=511 when the enemy starts walking
    /// (`i(1, ...)` sets `eu = O.nextInt() & 0x1FF`).
    /// Starts 0 at spawn (so the first walk comes
    /// after a single idle tick).
    pub eu: i32,
    /// Stagger cooldown `dX` (Phase 6, e.java:282):
    /// a mode-2 (hc) hit adds `(fK[gV] << 5) + 40`;
    /// it decays by `40 + max_health/64` per tick
    /// (e.java:6627-6630) and is NOT clamped at 0
    /// (the original lets it go negative by one
    /// tick's decrement). While `dX > 0` the enemy
    /// shows the slot-137 indicator; while
    /// `dX > 500` melee hits do not apply
    /// (e.java:5863/5880); the weakened marker
    /// needs `dX <= 0` (e.java:2571).
    pub dx: i32,
    /// Respawn counter's initial value `ed`
    /// (Phase 7, e.java:2805 `ed = ec`): a flag-4
    /// trigger resets `ec` to this.
    pub ed: i32,
    /// Live respawn countdown `er` (Phase 7): set
    /// to `et` at spawn; counts down by 40 per tick
    /// while the enemy is dead (`dT < 0`). When it
    /// reaches 0 (and the other respawn conditions
    /// hold) the enemy respawns at its spawn point.
    pub er: i32,
    /// Spawn-in ticks remaining (Phase 8): after an
    /// Arena/level respawn the enemy plays its
    /// spawn-in action (27 kind 11, 14 kind 12, 18
    /// kind 13; e.java:6350-6367) for this many
    /// ticks before going idle. 0 when not
    /// spawning in. The 1-frame loops (14/18) need
    /// the timer to exit; the one-shot (27) exits
    /// on its own repeat end or the timer,
    /// whichever comes first.
    pub spawn_in: i32,
    /// Special-attack state `dS` (dS, e.java:4314):
    /// maintained by `set_action` from the kind
    /// class and the new action — kind 11: 5/6/16/15
    /// -> 0, 8/9/10/24 -> 1 (and `dx` = 0), 2/7 ->
    /// 2, 11/21/3 -> 3, else -1; kind 12: 7/12/13
    /// -> 1 (and `dx` = 0), else -1; kind 13:
    /// 7/13/17/16 -> 1 (and `dx` = 0), else -1.
    /// The special-attack trigger (e.java:6303)
    /// needs `dS != 1`. Starts -1 (idle).
    pub ds: i32,
    /// Dive velocity `dJ` (dS, e.java:5867-5878):
    /// the special attack's extra downward speed,
    /// 0 -> 4 -> 8 (+4 per 200 ms). Reset to 0
    /// whenever the enemy is not in its special
    /// attack.
    pub dj: i32,
    /// Dive timer `dK` (dS, e.java:5869-5872):
    /// ms since the last `dJ` step; reset with
    /// `dj` when the special attack is not active.
    pub dk: i32,
    /// Freeze guard (dS, e.java:4354): set by
    /// Gameplay's enemy loop while a grapple is
    /// in progress and this is not the grappled
    /// enemy — `set_action` then forces new
    /// actions to idle (0), except the spawn-in
    /// actions (27/14/18) and actions 7/11.
    /// (The `ay > 0` half of the original's
    /// guard is a camera pan that never occurs
    /// in the decompiled code — `ay` is only
    /// ever assigned -1 — so it is not ported.)
    pub frozen: bool,
    /// The enemy's anim set (cloned at load, Phase 4):
    /// lets the cutscene set the enemy's action to
    /// an aZ-table entry without a Resources lookup.
    pub anim_set: Option<anim::AnimSet>,
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
            max_health: enemy.health,
            ef: 0,
            dying: false,
            dead: false,
            attack_hit_done: false,
            attack_count: 0,
            el,
            eu: 0,
            dx: 0,
            ed: enemy.ec,
            er: enemy.et,
            spawn_in: 0,
            ds: -1,
            dj: 0,
            dk: 0,
            frozen: false,
            anim_set: None,
            enemy,
        }
    }

    /// `ei` (dS, e.java:4314): a kind-11 enemy in
    /// one of the elite attack actions (13, 17, 23,
    /// 12, 15, 16, 14, 21). A pure function of the
    /// kind and the current action — it is set by
    /// `i()` on every action change, and the
    /// special-attack trigger (e.java:6303) and the
    /// AI both gate on it.
    pub fn is_ei(&self) -> bool {
        self.enemy.kind == 11 && matches!(self.action, 13 | 17 | 23 | 12 | 15 | 16 | 14 | 21)
    }

    /// `eh` (dS, e.java:4314): a kind-11 enemy in
    /// one of the elite hurt/spawn actions (19, 18,
    /// 26, 20). Like `ei`, a pure function of the
    /// kind and the current action; the trigger and
    /// the freeze guard both gate on it.
    pub fn is_eh(&self) -> bool {
        self.enemy.kind == 11 && matches!(self.action, 19 | 18 | 26 | 20)
    }

    /// The enemy action setter `i(int, int)`
    /// (e.java:4314-4410), ported whole: it
    /// maintains `dS` (and resets `dx` to 0 when
    /// `dS` becomes 1), then applies the freeze
    /// guard when `frozen` is set (a grapple is
    /// in progress and this is not the grappled
    /// enemy): a non-`eh` enemy may not start a
    /// new action other than 7 or 11 — an idle
    /// enemy stays idle, a moving one is forced
    /// to idle, and only the spawn-in action
    /// (27 kind 11, 14 kind 12, 18 kind 13) is
    /// allowed through. `dS` is updated from the
    /// REQUESTED action before the guard runs,
    /// as in the original (so a rejected request
    /// still leaves its `dS` behind).
    /// The `eu` re-randomisation on action 1 is
    /// NOT done here: the port's chase branch
    /// already re-randomises `eu` when the enemy
    /// starts walking, which is the same moment
    /// (`i(1, ...)` is only ever called on the
    /// idle-to-walk transition).
    pub fn set_action(&mut self, anim_set: &anim::AnimSet, action: usize) {
        match self.enemy.kind {
            11 => match action {
                5 | 6 | 16 | 15 => self.ds = 0,
                8 | 9 | 10 | 24 => {
                    self.dx = 0;
                    self.ds = 1;
                }
                2 | 7 => self.ds = 2,
                11 | 21 | 3 => self.ds = 3,
                _ => self.ds = -1,
            },
            12 => {
                if matches!(action, 7 | 12 | 13) {
                    self.dx = 0;
                    self.ds = 1;
                } else {
                    self.ds = -1;
                }
            }
            13 => {
                if matches!(action, 7 | 13 | 17 | 16) {
                    self.dx = 0;
                    self.ds = 1;
                } else {
                    self.ds = -1;
                }
            }
            _ => self.ds = -1,
        }
        let new_eh = self.enemy.kind == 11 && matches!(action, 19 | 18 | 26 | 20);
        if self.frozen && !new_eh && action != 7 && action != 11 {
            let current = self.action;
            let spawn_in = match self.enemy.kind {
                12 => 14,
                13 => 18,
                _ => 27,
            };
            let idle_now = match self.enemy.kind {
                12 => matches!(current, 0 | 12 | 10),
                13 => matches!(current, 0 | 12 | 11),
                _ => current == 0,
            };
            if idle_now && action != spawn_in {
                // Idle enemy: no change.
                return;
            }
            if action == spawn_in {
                // Spawn-in is allowed through.
            } else {
                // Moving/acting enemy: forced to idle.
                self.action = 0;
                self.slot = anim::AnimSlot::new(anim_set, 0);
                return;
            }
        }
        self.action = action;
        self.slot = anim::AnimSlot::new(anim_set, action);
    }

    /// Spawn-in action by kind class (Phase 8,
    /// e.java:6350-6367): kind 11 -> 27 (the
    /// 14-frame one-shot; the `eh` branch's 26 is
    /// a 1-frame loop, not ported — the port has
    /// no `eh` flag), kind 12 -> 14, kind 13 -> 18.
    pub fn spawn_in_action(&self) -> usize {
        match self.enemy.kind {
            12 => 14,
            13 => 18,
            _ => 27,
        }
    }

    /// Death action from e.java's ek <= 0 handling (first
    /// hitbox-damage site, the one the sword uses): dQ 1
    /// (group kind 11) -> i(8), dQ 2/3 (kinds 12/13) ->
    /// i(7). The ei/eh special case (i(11), a looping
    /// anim) and the orb drop (b(...)) are NOT ported
    /// here — see REPORT. Type-3 dR == 17 skips i(7) in
    /// the original; dR is untraced for our enemies, so
    /// 7 is used for all kind-13 enemies (documented
    /// approximation).
    pub fn death_action(&self) -> usize {
        match self.enemy.kind {
            11 => 8,
            _ => 7,
        }
    }

    /// Stagger (flinch) action from e.java's non-lethal
    /// hit branch (Phase 6): kinds 11/12 -> action 2,
    /// kind 13 -> action 4 normally; the heavy
    /// variants apply when the player is in a
    /// finisher (hB == 60 blades / 77 Artemis,
    /// e.java:6460-6475 — the `ge != -1` grapple
    /// half of the condition is not ported, since
    /// grapple is not ported): kind 11 -> 7,
    /// kind 12 -> 9, kind 13 -> 8. The 13 variant
    /// (kind 13 hit by player action 12) is NOT
    /// ported — player action 12 is untraced.
    /// Measured on set 18432: action 2 = 10 frames,
    /// repeat 1, ~600 ms; action 4 = 2 frames, repeat 1,
    /// ~200 ms. Stagger root motion (set 18432):
    /// action 2 totals dx -26, action 7 -15,
    /// action 8 -32, action 9 -18 (backward).
    /// Stagger action for a non-lethal hit (Phase 6,
    /// e.java:6446-6490, with the Phase 8 kind-13
    /// variant): kind 11 -> 7 (heavy) / 2, kind 12
    /// -> 9 / 2, kind 13 -> 13 when the enemy is
    /// currently playing action 12 (`n2 == 12`,
    /// e.java:6463; `dM != n9` is not ported — the
    /// port has no main-enemy marker, so any kind-13
    /// enemy in action 12 takes the 13), else 8
    /// (heavy) / 4. `current_action` is the enemy's
    /// action before this hit.
    pub fn stagger_action(&self, heavy: bool, current_action: usize) -> usize {
        match self.enemy.kind {
            11 => {
                if heavy {
                    7
                } else {
                    2
                }
            }
            12 => {
                if heavy {
                    9
                } else {
                    2
                }
            }
            13 => {
                if current_action == 12 {
                    13
                } else if heavy {
                    8
                } else {
                    4
                }
            }
            _ => 2,
        }
    }

    /// Damage from a player hit: `fI[0] << gP` = 2 for the
    /// first combo step (gG = 0, gP = 0). Returns true if
    /// this hit killed the enemy: it starts dying (death
    /// animation plays, then `dead` is set by `tick`).
    /// A non-lethal hit starts the stagger action
    /// (interrupting an attack in progress, as in the
    /// original); hits on a dying or dead enemy are
    /// ignored. Facing is left unchanged on stagger
    /// (documented approximation — the original sets
    /// `ej` from the player's relative position).
    /// `heavy` is true when the player is in a finisher
    /// (action 60/77) — selects the heavy stagger
    /// variants (Phase 6).
    pub fn apply_damage(&mut self, anim_set: &anim::AnimSet, amount: i32, heavy: bool) -> bool {
        if self.dead || self.dying {
            return false;
        }
        // The kind-13 stagger variant (Phase 8) keys
        // on the action the enemy is playing when
        // hit, so capture it before the stagger
        // overwrites it.
        let current_action = self.action;
        self.health -= amount;
        if self.health <= 0 {
            self.dying = true;
            let death = self.death_action();
            self.set_action(anim_set, death);
            return true;
        }
        let stagger = self.stagger_action(heavy, current_action);
        self.set_action(anim_set, stagger);
        false
    }

    /// One 40 ms tick: advance the current action,
    /// decide the next one, and return the damage
    /// this enemy deals to the player this tick (0
    /// when it does not hit). `rng` supplies the
    /// `eu` re-randomisation when the enemy starts
    /// walking (Phase 4).
    pub fn tick(
        &mut self,
        anim_set: &anim::AnimSet,
        flags: &crate::player::TileFlags,
        player_x: i32,
        player_y: i32,
        rng: &mut crate::pickups::OrbRng,
    ) -> i32 {
        if self.dead {
            return 0;
        }
        // dX decay (Phase 6, e.java:6627-6630):
        // `40 + max_health/64` per tick while
        // positive; not clamped at 0 (the original
        // lets it go negative by one tick).
        if self.dx > 0 {
            self.dx -= 40 + self.max_health / 64;
        }
        // Dying: play the death animation with its
        // root-motion slide (Phase 6: the dx is
        // authored backward — set 18432 action 7
        // totals -15, action 8 -32 — so it is
        // applied RAW with the facing sign, like
        // the player's hurt slide), then mark fully
        // dead when it ends. No AI, no damage, no
        // further hits.
        if self.dying {
            let (dx, _dy) = self.slot.advance(anim_set, 40);
            self.x += if self.facing_right { dx } else { -dx };
            if self.slot.repeat == 0 {
                self.dying = false;
                self.dead = true;
            }
            return 0;
        }
        // Spawn-in (Phase 8): after a respawn the
        // enemy plays its spawn-in action before
        // going idle. The one-shot (27) ends on
        // its own repeat; the 1-frame loops (14/18)
        // exit on the timer (12 ticks ~ 500 ms).
        // No AI, no damage, no hits while it plays.
        if self.spawn_in > 0 {
            self.slot.advance(anim_set, 40);
            self.spawn_in -= 1;
            if self.spawn_in <= 0 || self.slot.repeat == 0 {
                self.spawn_in = 0;
                self.set_action(anim_set, 0);
            }
            return 0;
        }
        // Special attack (dS): kind 11 action 11,
        // kind 12 action 8, kind 13 action 10.
        // The enemy dives — the anim's own dy root
        // motion (action 11 totals +1371 over its
        // 63 loops) plus the accelerating `dJ`
        // (0 -> 4 -> 8, +4 per 200 ms) — and deals
        // contact damage once (el<<1, or el for a
        // `dM` enemy). It ends on landing (the tile
        // at the feet is solid, e.java:7044-7072:
        // the landing stagger 7 kind 11, 9 kind 12,
        // 8 kind 13) or when its anim ends, and
        // the enemy idles. This branch runs before
        // the stagger branch: kind 12's action 8
        // is a special attack here, not a stagger.
        if matches!((self.enemy.kind, self.action), (11, 11) | (12, 8) | (13, 10)) {
            let (dx, dy) = self.slot.advance(anim_set, 40);
            self.x += if self.facing_right { dx } else { -dx };
            self.dk += 40;
            if self.dk > 200 {
                self.dk -= 200;
                if self.dj < 8 {
                    self.dj += 4;
                }
            }
            self.y += dy + self.dj;
            let mut damage = 0;
            if !self.attack_hit_done
                && (player_x - self.x).abs() < 32
                && (player_y - self.y).abs() < 16
            {
                self.attack_hit_done = true;
                damage = if self.enemy.is_dm {
                    self.el
                } else {
                    self.el << 1
                };
            }
            let foot_row = self.y / 16;
            if flags.solid(self.x / 16, foot_row) {
                self.y = foot_row * 16;
                let land = match self.enemy.kind {
                    12 => 9,
                    13 => 8,
                    _ => 7,
                };
                self.set_action(anim_set, land);
                self.dj = 0;
                self.dk = 0;
            } else if self.slot.repeat == 0 {
                self.set_action(anim_set, 0);
                self.dj = 0;
                self.dk = 0;
            }
            return damage;
        }
        // Not in a special attack: `dJ`/`dK` reset
        // (e.java:5877-5878).
        self.dj = 0;
        self.dk = 0;
        // Stagger in progress (actions 2/4, and the
        // heavy variants 7/8/9 from apply_damage in
        // this port): play it with its root-motion
        // slide (set 18432 action 2 totals dx -26,
        // action 9 -18), then return to idle when
        // it ends. No movement, no attack.
        if matches!(self.action, 2 | 4 | 7 | 8 | 9) {
            let (dx, _dy) = self.slot.advance(anim_set, 40);
            self.x += if self.facing_right { dx } else { -dx };
            if self.slot.repeat == 0 {
                self.set_action(anim_set, 0);
            }
            return 0;
        }
        // Attack swing in progress: advance, apply its hit once
        // at frame >= 2, then return to idle when finished.
        if self.action == 5 || self.action == 6 {
            self.slot.advance(anim_set, 40);
            let mut damage = 0;
            if self.slot.frame >= 2 && !self.attack_hit_done {
                self.attack_hit_done = true;
                // el<<1 for action 5, el<<2 for
                // action 6 (a normal enemy, not
                // `dM`): e.java:6600-6609 — the
                // `dM != n9` doubling does not
                // apply to a `dM` enemy, so it
                // deals el (action 5) / el<<1
                // (action 6).
                damage = if self.enemy.is_dm {
                    if self.action == 6 {
                        self.el << 1
                    } else {
                        self.el
                    }
                } else if self.action == 6 {
                    self.el << 2
                } else {
                    self.el << 1
                };
            }
            if self.slot.repeat == 0 {
                self.set_action(anim_set, 0);
            }
            return damage;
        }
        let dx_player = player_x - self.x;
        let dy_player = player_y - self.y;
        let near_attack = dx_player.abs() < 48 && dy_player.abs() < 16;
        let near_chase = dx_player.abs() < 144 && dy_player.abs() < 32;
        // Special-attack trigger (e.java:6303):
        // an airborne (`!eA`) enemy — the tile at
        // its feet is not solid (flag 1,
        // e.java:5850) — not `ei`/`eh`, with
        // `dS != 1`, in an allowed action, that
        // is not the `dN` special enemy, starts
        // its special attack (11 kind 11, 8 kind
        // 12, 10 kind 13). In the port an enemy
        // that walked off a ledge floats (no
        // enemy gravity), so it reads airborne
        // until the dive brings it back to the
        // ground. The kind-1 exclusion list in
        // the original repeats 11 twice; the set
        // used here is {11, 27, 3, 24}.
        let grounded = flags.solid(self.x / 16, self.y / 16);
        let special_ok = match self.enemy.kind {
            11 => !matches!(self.action, 11 | 27 | 3 | 24),
            12 => !matches!(self.action, 8 | 14),
            13 => !matches!(self.action, 10 | 18),
            _ => false,
        };
        if !grounded
            && !self.is_ei()
            && !self.is_eh()
            && self.ds != 1
            && special_ok
            && !self.enemy.is_dn
        {
            let special = match self.enemy.kind {
                12 => 8,
                13 => 10,
                _ => 11,
            };
            self.set_action(anim_set, special);
            self.attack_hit_done = false;
            return 0;
        }
        // Frozen (a grapple is in progress and
        // this is not the grappled enemy): the
        // enemy goes idle and stays idle while
        // the grapple runs (the `i()` guard,
        // e.java:4354). The state-playback
        // branches above (dying, spawn-in,
        // stagger, attack, special) still run;
        // only new AI decisions are suppressed.
        if self.frozen {
            if self.action != 0 {
                self.set_action(anim_set, 0);
            }
            self.slot.advance(anim_set, 40);
            return 0;
        }
        // Attack trigger (type-1 AI: |N&3|==3 -> action 6, else 5;
        // approximated here as every 4th attack being action 6).
        if near_attack {
            self.attack_count += 1;
            let attack = if self.attack_count % 4 == 0 { 6 } else { 5 };
            self.set_action(anim_set, attack);
            self.attack_hit_done = false;
            if dx_player > 0 {
                self.facing_right = true;
            } else if dx_player < 0 {
                self.facing_right = false;
            }
            return 0;
        }
        // Chase (Phase 4, `eu` idle-to-walk timer):
        // the enemy idles until `eu < 0`, then walks
        // and `eu` re-randomises to 0..=511 for the
        // next idle. While idle in range, `eu` drops
        // by 40 per tick.
        if near_chase {
            if dx_player > 16 {
                self.facing_right = true;
            } else if dx_player < -16 {
                self.facing_right = false;
            }
            if self.eu < 0 {
                self.eu = (rng.next() & 0x1FF) as i32;
                // Fall through to the walk below.
            } else {
                self.eu -= 40;
                if self.action != 0 {
                    self.set_action(anim_set, 0);
                }
                self.slot.advance(anim_set, 40);
                return 0;
            }
        }
        let walk = if near_chase {
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
                self.set_action(anim_set, 0);
            }
            self.slot.advance(anim_set, 40);
            return 0;
        }
        if self.action != 1 {
            self.set_action(anim_set, 1);
        }
        let (dx, _dy) = self.slot.advance(anim_set, 40);
        let step = dx.abs();
        let gw = self.x / 16;
        let gy = (self.y - 1) / 16;
        let ahead = if self.facing_right { gw + 1 } else { gw - 1 };
        // Flag 2 is an enemy barrier (Phase 8,
        // e.java:6948): a grounded walking enemy
        // (action 1) will not step into a tile
        // with flag 2 set — it turns instead. The
        // original exempts the enemy's own spawn
        // column (`dZ`) and the `eh` state; the
        // port has neither, so the barrier is
        // absolute while walking. Flag 2 is set by
        // type-7 zone footprints and by respawns
        // (above the spawn point).
        let blocked = flags.solid(ahead, gy)
            || flags.solid(ahead, gy - 1)
            || flags.solid(ahead, gy - 2)
            || flags.get(ahead, gy) & 2 != 0;
        if blocked {
            self.facing_right = !self.facing_right;
        }
        self.x += if self.facing_right { step } else { -step };
        0
    }
}

/// Bbox height `ef` (slice 14): the P[3] of the
/// original's spawn-time `a(set, slot, 0, 0, 0, P)`
/// call — max(anchor_y + dy + h) minus
/// min(anchor_y + dy) over the initial action's
/// timeline-0 composition, transform 0. Returns 0
/// when the composition is empty or unloadable.
pub fn enemy_bbox_height(
    anim_set: &anim::AnimSet,
    sprite_set: &crate::resources::SpriteSet,
    action: usize,
) -> i32 {
    let Ok(frames) = anim::frames_for_timeline_frame(anim_set, action, 0, 0) else {
        return 0;
    };
    let mut top = i32::MAX;
    let mut bottom = i32::MIN;
    for fr in frames {
        let Some(frame) = sprite_set.frame(fr.sprite_idx) else {
            continue;
        };
        let y0 = fr.dy + frame.anchor_y;
        top = top.min(y0);
        bottom = bottom.max(y0 + frame.h);
    }
    if bottom == i32::MIN {
        0
    } else {
        bottom - top
    }
}

/// Weakened-marker visibility (slice 14, Phase 6),
/// from the `r(Graphics)` condition for slot 132,
/// with the port's approximations: no targeting
/// (`ge < 0` always — grapple/lock not ported),
/// `!eh` always (eh not ported), and the dQ/dR
/// boss exceptions skipped (dR/dM/dN not ported).
/// What remains is the health rule plus the dX
/// rule: alive, below half max health, and not
/// staggered (`dX <= 0`, e.java:2571).
pub fn weakened_marker_visible(health: i32, max_health: i32, dead: bool, dying: bool, dx: i32) -> bool {
    !dead && !dying && health >= 0 && health < max_health / 2 && dx <= 0
}

/// Draw the weakened markers (slot 132, slice 14):
/// anim set 1027, action 10, at the marker slot's
/// current visual, from sprite set 1040 (`aU`),
/// above each visible enemy at
/// `y - ef - (ef >> 2)` (the dN `+ (ef >> 3)`
/// variant is not ported). The marker slot is
/// advanced once per gameplay tick by the caller
/// (the original's `t(132)` at the end of the
/// enemy update), so it pulses through action 10's
/// 7 timeline frames on a ~1 s cycle.
#[allow(clippy::too_many_arguments)]
pub fn draw_weakened_markers(
    g: &Graphics,
    fb: &mut Framebuffer,
    bar_anim: &anim::AnimSet,
    bar_sprites: &crate::resources::SpriteSet,
    bar_visual: usize,
    states: &[EnemyState],
    cam_x: i32,
    cam_y: i32,
) {
    let Ok(frames) = anim::frames_for_timeline_frame(bar_anim, 10, bar_visual, 0) else {
        return;
    };
    for st in states {
        if !weakened_marker_visible(st.health, st.max_health, st.dead, st.dying, st.dx) {
            continue;
        }
        let base_y = st.y - st.ef - (st.ef >> 2);
        for fr in &frames {
            if let Some(frame) = bar_sprites.frame(fr.sprite_idx) {
                draw_sprite_frame(
                    g,
                    fb,
                    &bar_sprites.image,
                    frame,
                    st.x + fr.dx - cam_x,
                    base_y + fr.dy - cam_y,
                    fr.transform,
                );
            }
        }
    }
}

/// Draw the stagger indicator (Phase 6, e.java:2567-2570):
/// while an enemy's `dX > 0`, slot 137 (anim set 7168
/// action 31, sprite set 7169 `bH`) is drawn at the
/// enemy's position `(dT, dU)`, transform 0. The slot
/// is advanced 40 ms per staggered enemy per tick by
/// the caller (the original's `b(137, L)` inside the
/// per-enemy draw block), so it animates.
pub fn draw_stagger_indicators(
    g: &Graphics,
    fb: &mut Framebuffer,
    stagger_anim: &anim::AnimSet,
    stagger_sprites: &crate::resources::SpriteSet,
    stagger_visual: usize,
    states: &[EnemyState],
    cam_x: i32,
    cam_y: i32,
) {
    for st in states {
        if st.dead || st.dying || st.dx <= 0 {
            continue;
        }
        if let Ok(frames) =
            anim::frames_for_timeline_frame(stagger_anim, 31, stagger_visual, 0)
        {
            for fr in &frames {
                if let Some(frame) = stagger_sprites.frame(fr.sprite_idx) {
                    draw_sprite_frame(
                        g,
                        fb,
                        &stagger_sprites.image,
                        frame,
                        st.x + fr.dx - cam_x,
                        st.y + fr.dy - cam_y,
                        fr.transform,
                    );
                }
            }
        }
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
