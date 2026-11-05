//! Orb pickups (slice 12): gold, health, magic and damage orbs.
//!
//! Traced from e.java's `b(int, int, int, int)` spawner, the
//! per-frame pickup update loop, and `h(Graphics)` draw:
//!
//! * Spawn `b(x, y, type, value)`: type 1/2 values are
//!   shifted left 3 first. The value is split into orbs:
//!   big = 18 / small = 9 when value > 60, else 6 / 3;
//!   orbs take the big value while more than 24 remains,
//!   else the small value; a final overshoot is folded
//!   back into the last orb. At most 19 orbs exist.
//!   Each orb scatters from one random int: x = spawn_x
//!   +/- (N & 9), vx = +/- (N & 7) << 8, y = spawn_y
//!   +/- (N & 9) - 7, vy = ((N >> 3) & 7) << 8.
//! * Per tick, an orb homes on its target (player when
//!   target is None, else an enemy): vx/vy change by
//!   9 * 40 = 360 toward the target, clamped to +/-3584
//!   (x decays >> 2 when aligned or capped, y decays
//!   by >> 3 around a +/-8 dead zone centred on
//!   target_y - 16, i.e. target_y - gD/2 with gD = 32).
//!   Position integrates as pos += vel >> 8, and the
//!   4-slot trail shifts so slot 0 is newest.
//! * Collection: when the orb overlaps the target rect
//!   and its age exceeds 75 ms it is collected (type 0:
//!   gold, 1: health, 2: magic, 3: damage to the target
//!   enemy) and swap-removed; otherwise its age grows
//!   by 40 ms. An orb overlapping while too young ages
//!   only once it leaves overlap, exactly as in the
//!   original.
//! * Draw (`h(Graphics)`): sprite set 1034, frame
//!   (type * 4 + trail_slot) at each trail position.
//!
//! Approximations, all documented in REPORT: the
//! collection rect is |dx| <= 16 and target_y - 32 <=
//! orb_y <= target_y (the original uses the per-frame
//! player bbox gC/gD, which the port does not store);
//! the gold-full gate (hr >= hs) is skipped because
//! hs is set per level in the original and its
//! initialisation is untraced; level-placed
//! collectibles (the fm/ft system that also calls
//! b()) are not spawned yet — only death drops are.
//! A zero-value trailing orb on exact multiples of
//! the orb size is faithful to the original loop
//! (see the tests), not a port bug. So is the
//! "glued orb" quirk: an orb that ends up exactly on
//! the player with ~0 velocity stays overlapping
//! every tick, so its age never grows past 75 and it
//! never collects — until the player jumps, turns
//! sharply, or otherwise breaks overlap (verified:
//! a glued value-1 orb collected for the full 10
//! gold after a jump).

use crate::midp::{draw_sprite_frame, Framebuffer, Graphics};
use crate::resources::Resources;

/// One random int per orb, matching Java Random's
/// bit use in `b()`. A tiny xorshift; callers seed it.
pub struct OrbRng(pub u32);

impl OrbRng {
    pub fn next(&mut self) -> u32 {
        let mut x = self.0;
        if x == 0 {
            x = 0x9E3779B9;
        }
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }
}

/// Pickup types, matching `fE` in e.java.
pub const GOLD: i32 = 0;
pub const HEALTH: i32 = 1;
pub const MAGIC: i32 = 2;
pub const DAMAGE: i32 = 3;

/// Max live orbs, matching the `fG < 18` cap (slots 0..=18).
pub const MAX_ORBS: usize = 19;

/// One orb. Positions are level pixels; velocities are
/// fixed point << 8, as in the original.
#[derive(Debug, Clone)]
pub struct Pickup {
    pub kind: i32,
    pub value: i32,
    pub x: i32,
    pub y: i32,
    pub vx: i32,
    pub vy: i32,
    /// Trail, newest first: [current, 1 tick ago, ...].
    pub trail: [(i32, i32); 4],
    /// Age in ms (`fD`); collectable only past 75.
    pub age_ms: i32,
    /// None = homes on the player; Some(i) = homes on
    /// enemy state index i (`fC`, -1 = player).
    pub target: Option<usize>,
}

/// What an update tick produced, for the caller to apply
/// (health/gold/magic live on the caller, not here).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickupEvent {
    /// A player-targeted orb was collected.
    Collected { kind: i32, value: i32 },
    /// An enemy-targeted orb hit enemy state index.
    HitEnemy { index: usize, value: i32 },
}

#[derive(Debug, Default)]
pub struct Pickups {
    pub orbs: Vec<Pickup>,
}

impl Pickups {
    /// Spawn orbs for `b(x, y, kind, value)`. Death
    /// sites call this with the enemy's `em` value.
    pub fn spawn(&mut self, x: i32, y: i32, kind: i32, value: i32, rng: &mut OrbRng) {
        if value == 0 {
            return;
        }
        let mut v = if kind == HEALTH || kind == MAGIC {
            value << 3
        } else {
            value
        };
        let (big, small) = if v > 60 { (18, 9) } else { (6, 3) };
        // The original loop runs until an overshoot folds
        // back — so when the value hits exactly 0 it
        // spawns one more orb, which folds to 0. That
        // trailing zero-value orb is faithful, not a
        // port bug (see the test below).
        loop {
            if self.orbs.len() >= MAX_ORBS {
                // Cap reached with value left: fold the
                // remainder into the last orb (b()'s tail).
                if let Some(last) = self.orbs.last_mut() {
                    last.value += v;
                }
                break;
            }
            let orb_value = if v > 24 { big } else { small };
            let folded = v - orb_value < 0;
            v -= orb_value;
            let mut val = orb_value;
            if folded {
                val += v;
                v = 0;
            }
            let n = rng.next();
            let sign_x = if n & 0x80 != 0 { 1 } else { -1 };
            let ox = x + sign_x * (n & 9) as i32;
            let vx = sign_x * (n & 7) as i32 * 256;
            let sign_y = if n & 0x100 != 0 { 1 } else { -1 };
            let oy = y + sign_y * (n & 9) as i32 - 7;
            let vy = ((n >> 3) & 7) as i32 * 256;
            self.orbs.push(Pickup {
                kind,
                value: val,
                x: ox,
                y: oy,
                vx,
                vy,
                trail: [(ox, oy); 4],
                age_ms: 0,
                target: None,
            });
            if folded {
                break;
            }
        }
    }

    /// One 40 ms tick. `player` is (x, y); `enemies` are
    /// (x, y, alive) per state index for damage orbs.
    /// Returns collection/damage events; collected and
    /// damage orbs are swap-removed here.
    pub fn update(
        &mut self,
        player: (i32, i32),
        enemies: &[(i32, i32, bool)],
    ) -> Vec<PickupEvent> {
        let mut events = Vec::new();
        let mut i = 0;
        while i < self.orbs.len() {
            let (tx, ty) = match self.orbs[i].target {
                None => player,
                Some(e) => match enemies.get(e) {
                    Some(&(ex, ey, _)) => (ex, ey),
                    None => player,
                },
            };
            let orb = &mut self.orbs[i];
            // Homing: 9 * L = 360 per tick, clamp 3584.
            if tx < orb.x && orb.vx > -3584 {
                orb.vx -= 360;
            } else if tx > orb.x && orb.vx < 3584 {
                orb.vx += 360;
            } else {
                orb.vx >>= 2;
            }
            let aim_y = ty - 16;
            if aim_y + 8 < orb.y && orb.vy > -3584 {
                orb.vy -= 360;
            } else if aim_y - 8 > orb.y && orb.vy < 3584 {
                orb.vy += 360;
            } else {
                orb.vy -= orb.vy >> 3;
            }
            // Collection test (see module docs for the
            // rect approximation).
            let in_range =
                (orb.x - tx).abs() <= 16 && orb.y >= ty - 32 && orb.y <= ty;
            if in_range {
                if orb.age_ms > 75 {
                    let kind = orb.kind;
                    let value = orb.value;
                    let target = orb.target;
                    self.orbs.swap_remove(i);
                    match target {
                        None => events.push(PickupEvent::Collected { kind, value }),
                        Some(e) => {
                            if kind == DAMAGE {
                                events.push(PickupEvent::HitEnemy { index: e, value });
                            } else {
                                // Non-damage orbs never target
                                // enemies in practice; treat as
                                // collected to avoid a leak.
                                events.push(PickupEvent::Collected { kind, value });
                            }
                        }
                    }
                    continue;
                }
                // Too young and overlapping: no ageing
                // this tick, exactly as in the original.
            } else {
                orb.age_ms += 40;
            }
            // Integrate + shift the trail (slot 0 newest).
            orb.x += orb.vx >> 8;
            orb.y += orb.vy >> 8;
            orb.trail = [
                (orb.x, orb.y),
                orb.trail[0],
                orb.trail[1],
                orb.trail[2],
            ];
            i += 1;
        }
        events
    }

    /// Draw all orbs: sprite set 1034, frame
    /// (kind * 4 + trail_slot) at each trail position.
    pub fn draw(
        &self,
        g: &Graphics,
        fb: &mut Framebuffer,
        res: &mut Resources,
        cam_x: i32,
        cam_y: i32,
    ) {
        let Ok(sprite_set) = res.sprite_set(1034, None) else {
            return;
        };
        for orb in &self.orbs {
            for slot in 0..4usize {
                let frame_idx = (orb.kind * 4 + slot as i32) as usize;
                let Some(frame) = sprite_set.frame(frame_idx) else {
                    continue;
                };
                let (tx, ty) = orb.trail[slot];
                draw_sprite_frame(
                    g,
                    fb,
                    &sprite_set.image,
                    frame,
                    tx - cam_x,
                    ty - cam_y,
                    0,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_splits_value_into_orbs() {
        let mut p = Pickups::default();
        let mut rng = OrbRng(0x12345678);
        // em = 10, gold: 3, 3, 3, 1.
        p.spawn(100, 100, GOLD, 10, &mut rng);
        let values: Vec<i32> = p.orbs.iter().map(|o| o.value).collect();
        assert_eq!(values, vec![3, 3, 3, 1]);
        assert_eq!(values.iter().sum::<i32>(), 10);
    }

    #[test]
    fn spawn_shifts_health_and_magic_values() {
        let mut p = Pickups::default();
        let mut rng = OrbRng(0x12345678);
        // em = 10, health: value << 3 = 80 -> 18,18,18,18,8.
        p.spawn(100, 100, HEALTH, 10, &mut rng);
        let values: Vec<i32> = p.orbs.iter().map(|o| o.value).collect();
        assert_eq!(values, vec![18, 18, 18, 18, 8]);
    }

    #[test]
    fn spawn_exact_multiple_leaves_zero_value_orb() {
        let mut p = Pickups::default();
        let mut rng = OrbRng(0x12345678);
        // 12 = 3 * 4: the original loop spawns a
        // trailing zero-value orb; keep it faithful.
        p.spawn(100, 100, GOLD, 12, &mut rng);
        let values: Vec<i32> = p.orbs.iter().map(|o| o.value).collect();
        assert_eq!(values, vec![3, 3, 3, 3, 0]);
    }

    #[test]
    fn orb_homes_and_collects() {
        let mut p = Pickups::default();
        let mut rng = OrbRng(1);
        // em = 10 -> orbs 3, 3, 3, 1.
        p.spawn(100, 100, GOLD, 10, &mut rng);
        assert_eq!(p.orbs.len(), 4);
        let mut total = 0;
        for _ in 0..600 {
            for ev in p.update((200, 100), &[]) {
                if let PickupEvent::Collected { kind: GOLD, value } = ev {
                    total += value;
                }
            }
            if p.orbs.is_empty() {
                break;
            }
        }
        assert_eq!(total, 10, "orbs never fully reached the player");
        assert!(p.orbs.is_empty());
    }

    #[test]
    fn orb_too_young_does_not_collect() {
        let mut p = Pickups::default();
        let mut rng = OrbRng(1);
        p.spawn(200, 100, GOLD, 10, &mut rng);
        // Orbs right on the player but age 0: the first
        // ticks must not collect them.
        let events = p.update((200, 100), &[]);
        assert!(events.is_empty(), "fresh orbs collected too early");
        assert_eq!(p.orbs.len(), 4);
    }
}

