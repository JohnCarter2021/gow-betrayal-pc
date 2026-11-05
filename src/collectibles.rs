//! Level-placed collectibles, type-4 (Phase 5).
//!
//! Trace (from `decompiled/e.java`):
//!
//! * Type 4 (case ~3280): per item, `ft = m(3)`
//!   (the kind), `fs = m(10)` (the value); `fq`/`fr`
//!   are the position (bs/bt). Slot `121+i` holds
//!   anim set 5120, action `ft`.
//! * Load-time remap (e.java ~3283): if `ft == 3`,
//!   `fs = fs > 0 ? -1 : -2` — a health upgrade
//!   (`-1`, `++ho`) or a magic upgrade (`-2`,
//!   `++hp`). Collected upgrades are remembered in
//!   the save bitmask `he` (bit `bC` for health,
//!   bit `bC+10` for magic).
//! * Pickup (e.java ~5205): when the player is in
//!   state 31 and an uncollected item's bbox
//!   overlaps the player's, `ft += 4` (the
//!   "collected" action), `s(31)` (pickup pose),
//!   `fp = true`.
//! * Grant (e.java ~5437): on the next update, a
//!   collected item (`ft >= 4`) with `fs != 0`
//!   either grants an upgrade (`fs == -1` health,
//!   `fs == -2` magic, `I = 104`) or spawns orbs:
//!   `b(fq + fn/2, fr - fo - 7, ft - 8, fs)` —
//!   orbs of kind `ft` (0 gold, 1 health, 2 magic)
//!   totalling `fs`.
//!
//! The port spawns the orbs / grants the upgrade
//! at pickup time (functionally the same frame as
//! the original's next-update grant). The state-104
//! upgrade interstitial is not ported; the upgrade
//! applies at once (max rises and current refills
//! to the new max) and the pickup pose plays.

use crate::entities::LevelEntities;

/// A level-placed collectible (type-4 item).
pub struct Collectible {
    /// Item x in px (`fq`).
    pub x: i32,
    /// Item y in px (`fr`).
    pub y: i32,
    /// Kind `ft`: 0 gold, 1 health, 2 magic,
    /// 3 upgrade.
    pub kind: i32,
    /// Value `fs`: orb total for kinds 0-2;
    /// for kind 3, >0 = health upgrade, 0 = magic.
    pub value: i32,
    /// Already collected (`fp`).
    pub collected: bool,
}

/// Load the type-4 collectibles from the parsed
/// level (group 4: positions are px; extras are
/// `[ft, fs]` per item).
pub fn load_collectibles(parsed: &LevelEntities) -> Vec<Collectible> {
    let Some(group) = parsed.group(4) else {
        return Vec::new();
    };
    group
        .positions
        .iter()
        .enumerate()
        .map(|(i, &(px, py))| Collectible {
            x: px,
            y: py,
            kind: group.extra.get(i * 2).copied().unwrap_or(0) as i32,
            value: group.extra.get(i * 2 + 1).copied().unwrap_or(0) as i32,
            collected: false,
        })
        .collect()
}

/// The outcome of touching a collectible.
pub enum CollectOutcome {
    /// Orbs of `kind` totalling `value` (kinds 0-2).
    Orbs { kind: i32, value: i32, x: i32, y: i32 },
    /// A health upgrade (`ho + 1`).
    HealthUpgrade,
    /// A magic upgrade (`hp + 1`).
    MagicUpgrade,
}

/// Check whether the player (feet at `ga`/`gb`,
/// bbox `ga-10..ga+10`, `gb-34..gb`) overlaps an
/// uncollected item. Returns the outcome and
/// marks the item collected. The item occupies a
/// 16x16 box centred on its position.
pub fn check_collectible(
    items: &mut [Collectible],
    ga: i32,
    gb: i32,
) -> Option<CollectOutcome> {
    let (pl, pt, pr, pb) = (ga - 10, gb - 34, ga + 10, gb);
    for it in items.iter_mut() {
        if it.collected {
            continue;
        }
        let (il, it_, ir, ib) = (it.x - 8, it.y - 8, it.x + 8, it.y + 8);
        if il < pr && ir > pl && it_ < pb && ib > pt {
            it.collected = true;
            return Some(match it.kind {
                3 => {
                    if it.value > 0 {
                        CollectOutcome::HealthUpgrade
                    } else {
                        CollectOutcome::MagicUpgrade
                    }
                }
                kind => CollectOutcome::Orbs {
                    kind,
                    value: it.value,
                    x: it.x,
                    y: it.y,
                },
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type4_loads_and_remaps() {
        let mut res = crate::resources::Resources::new("../extracted");
        let parsed = crate::entities::parse_level_entities(9217, &res.chunk(9217).unwrap()).unwrap();
        let items = load_collectibles(&parsed);
        assert_eq!(items.len(), 7, "level 9217 has 7 type-4 items");
        // (189,8) ft=0 gold 200; (147,26) ft=2 magic 999.
        let gold = items.iter().find(|i| i.x == 189 * 16 && i.y == 8 * 16).unwrap();
        assert_eq!((gold.kind, gold.value), (0, 200));
        let magic = items.iter().find(|i| i.x == 147 * 16 && i.y == 26 * 16).unwrap();
        assert_eq!((magic.kind, magic.value), (2, 999));
    }

    #[test]
    fn upgrade_item_in_19456() {
        let mut res = crate::resources::Resources::new("../extracted");
        let parsed = crate::entities::parse_level_entities(19456, &res.chunk(19456).unwrap()).unwrap();
        let items = load_collectibles(&parsed);
        // (91,12) ft=3 fs=1 -> health upgrade;
        // (198,34) ft=3 fs=0 -> magic upgrade.
        let h = items.iter().find(|i| i.x == 91 * 16 && i.y == 12 * 16).unwrap();
        assert_eq!((h.kind, h.value), (3, 1));
        let m = items.iter().find(|i| i.x == 198 * 16 && i.y == 34 * 16).unwrap();
        assert_eq!((m.kind, m.value), (3, 0));
    }

    #[test]
    fn touch_collects_once() {
        let mut items = vec![
            Collectible { x: 100, y: 100, kind: 0, value: 200, collected: false },
            Collectible { x: 500, y: 100, kind: 3, value: 1, collected: false },
        ];
        // Player feet at (100, 108): overlaps item 0.
        let out = check_collectible(&mut items, 100, 108);
        assert!(matches!(out, Some(CollectOutcome::Orbs { kind: 0, value: 200, .. })));
        assert!(items[0].collected);
        // Touching again does nothing.
        assert!(check_collectible(&mut items, 100, 108).is_none());
        // Far away: nothing.
        assert!(check_collectible(&mut items, 300, 100).is_none());
        // Upgrade item.
        let out = check_collectible(&mut items, 500, 108);
        assert!(matches!(out, Some(CollectOutcome::HealthUpgrade)));
    }
}
