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
                em: e[4] as i32,
                anim_id,
                sprite_id,
                action: if ev { 1 } else { 0 },
                cutscene_target: e[2] == 1,
                dy: e[6] as i32,
                ec: e[0] as i32,
                et: e[1] as i32 * 1000,
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
        assert_eq!(list[8].health, 25);
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
            anim_set: None,
            enemy,
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
    pub fn stagger_action(&self, heavy: bool) -> usize {
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
                if heavy {
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
        self.health -= amount;
        if self.health <= 0 {
            self.dying = true;
            self.action = self.death_action();
            self.slot = anim::AnimSlot::new(anim_set, self.action);
            return true;
        }
        self.action = self.stagger_action(heavy);
        self.slot = anim::AnimSlot::new(anim_set, self.action);
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
                self.action = 0;
                self.slot = anim::AnimSlot::new(anim_set, 0);
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
                    self.action = 0;
                    self.slot = anim::AnimSlot::new(anim_set, 0);
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
