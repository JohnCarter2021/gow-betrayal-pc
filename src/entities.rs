//! Entity groups, slice 3b-1: the part of `e.a(byte[], false)` that
//! runs on the u=10 call, after the tile map (slice 3a).
//!
//! Sequence, from `decompiled/e.java`:
//! - Case 0 tail (still in the u=0 call, after the tile map):
//!   `bw = m(10)` (group count), `ja = iX*16`, `jb = iY*16`,
//!   `bz = l(ja*jb + 1)` (bits per cumulative position delta).
//! - u=10 branch: for each of the `bw` groups:
//!   `n8 = m(10)` (group type), `n4 = m(16)` (count),
//!   then `n4` positions, cumulative: `n3 += m(bz)`, wrapping
//!   `while n3 >= ja { n3 -= ja; n2 += 1 }`, giving
//!   `(x, y) = (n3, n2)` in level pixels.
//!   Then `d(n8, n4)` reads that type's extra fields, in order.
//!
//! This module parses positions AND the `d()` extra reads, but does
//! not yet interpret them semantically (that is 3b-2/3c). Extra
//! values are stored flattened, in read order, so a later chunk can
//! name them. The per-type read patterns, from `d(int, int)`:
//!   26: none (player spawn: bs[0], bt[0])
//!   6: 40 x m(10), 40 x m(10)
//!   22: per item: m(1), m(10), m(10), m(10), m(10)
//!   31: per item: m(1)
//!   3: per item: m(1), m(1)
//!   23: per item, REVERSED loop (n2-1..0): m(8), m(3)
//!   36, 17, 21: per item: m(6)
//!   32: per item: m(6), m(1)
//!   11/12: per item: m(3), m(10), m(1), m(16), m(16), m(10),
//!     m(10), m(1), m(1)
//!   13: as 11/12 plus m(1), m(1)
//!   33: per item: m(6), m(10), m(3)
//!   27: per item: m(3), m(1)
//!   34: per item: m(1), m(3), m(3), m(3), m(3)
//!   35: per item: m(3), m(10), m(1), m(1)
//!   8: per item: m(10), m(3)
//!   5, 29, 0, 18, 10: per item: m(10)
//!   9: per item: m(10), m(3)
//!   25: none
//!   14: per item: m(10), then 15 x (m(10), m(10), m(1))
//!   30: per item: m(1)
//!   7: per item: m(6), m(1), m(1)
//!   28: per item: m(10), m(10), m(3), m(10)
//!   4: per item: m(3), m(10)
//!   (types with no case in `d()`: no extra reads)
//!
//! Verification for For Sparta (9217): 27 groups, 1032 entities,
//! and the reader ends 1 bit short of the chunk end (102815 of
//! 102816 bits) — i.e. the whole chunk is consumed by tile map +
//! groups, with 1 padding bit. Any missing or extra read in `d()`
//! would desync every following group and break that total, so
//! this is a strong check on the patterns above.

use crate::level::{bits_needed, BitReader, Level};
use crate::rp::RpError;
use crate::level::parse_level_from_reader;

#[derive(Debug, Clone)]
pub struct EntityGroup {
    pub kind: u32,
    pub positions: Vec<(i32, i32)>,
    /// All `d()` extra reads, flattened, in read order.
    pub extra: Vec<u32>,
}

impl EntityGroup {
    pub fn player_spawn(&self) -> Option<(i32, i32)> {
        if self.kind == 26 {
            self.positions.first().copied()
        } else {
            None
        }
    }
}

#[derive(Debug)]
pub struct LevelEntities {
    pub level: Level,
    pub groups: Vec<EntityGroup>,
    /// Bits consumed from the chunk by tile map + groups.
    pub bits_consumed: usize,
    pub chunk_bits: usize,
}

impl LevelEntities {
    pub fn total_entities(&self) -> usize {
        self.groups.iter().map(|g| g.positions.len()).sum()
    }
    pub fn group(&self, kind: u32) -> Option<&EntityGroup> {
        self.groups.iter().find(|g| g.kind == kind)
    }
}

fn read_extra(br: &mut BitReader, kind: u32, count: usize, extra: &mut Vec<u32>) {
    let mut r = |n: usize| extra.push(br.read(n));
    match kind {
        6 => {
            for _ in 0..40 {
                r(10);
            }
            for _ in 0..40 {
                r(10);
            }
        }
        22 => {
            for _ in 0..count {
                r(1);
                r(10);
                r(10);
                r(10);
                r(10);
            }
        }
        31 | 30 => {
            for _ in 0..count {
                r(1);
            }
        }
        3 => {
            for _ in 0..count {
                r(1);
                r(1);
            }
        }
        23 => {
            for _ in 0..count {
                r(8);
                r(3);
            }
        }
        36 | 17 | 21 => {
            for _ in 0..count {
                r(6);
            }
        }
        32 => {
            for _ in 0..count {
                r(6);
                r(1);
            }
        }
        11 | 12 => {
            for _ in 0..count {
                r(3);
                r(10);
                r(1);
                r(16);
                r(16);
                r(10);
                r(10);
                r(1);
                r(1);
            }
        }
        13 => {
            for _ in 0..count {
                r(3);
                r(10);
                r(1);
                r(16);
                r(16);
                r(10);
                r(10);
                r(1);
                r(1);
                r(1);
                r(1);
            }
        }
        33 => {
            for _ in 0..count {
                r(6);
                r(10);
                r(3);
            }
        }
        27 => {
            for _ in 0..count {
                r(3);
                r(1);
            }
        }
        34 => {
            for _ in 0..count {
                r(1);
                r(3);
                r(3);
                r(3);
                r(3);
            }
        }
        35 => {
            for _ in 0..count {
                r(3);
                r(10);
                r(1);
                r(1);
            }
        }
        8 => {
            for _ in 0..count {
                r(10);
                r(3);
            }
        }
        5 | 29 | 0 | 18 | 10 => {
            for _ in 0..count {
                r(10);
            }
        }
        9 => {
            for _ in 0..count {
                r(10);
                r(3);
            }
        }
        14 => {
            for _ in 0..count {
                r(10);
                for _ in 0..15 {
                    r(10);
                    r(10);
                    r(1);
                }
            }
        }
        7 => {
            for _ in 0..count {
                r(6);
                r(1);
                r(1);
            }
        }
        28 => {
            for _ in 0..count {
                r(10);
                r(10);
                r(3);
                r(10);
            }
        }
        4 => {
            for _ in 0..count {
                r(3);
                r(10);
            }
        }
        _ => {}
    }
}

pub fn parse_level_entities(id: u32, data: &[u8]) -> Result<LevelEntities, RpError> {
    let mut br = BitReader::new(data);
    let level = parse_level_from_reader(id, &mut br)?;
    // Case-0 tail.
    let group_count = br.read(10) as usize;
    let ja = (level.width_tiles * 16) as i32;
    let bz = bits_needed((level.pixel_width() * level.pixel_height() + 1) as usize);
    let mut groups = Vec::with_capacity(group_count);
    for _ in 0..group_count {
        let kind = br.read(10);
        let count = br.read(16) as usize;
        let mut positions = Vec::with_capacity(count);
        let mut x = 0i32;
        let mut y = 0i32;
        for _ in 0..count {
            x += br.read(bz) as i32;
            while x >= ja {
                x -= ja;
                y += 1;
            }
            positions.push((x, y));
        }
        let mut extra = Vec::new();
        read_extra(&mut br, kind, count, &mut extra);
        groups.push(EntityGroup {
            kind,
            positions,
            extra,
        });
    }
    let bits_consumed = br.bits_consumed();
    Ok(LevelEntities {
        level,
        groups,
        bits_consumed,
        chunk_bits: data.len() * 8,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parse_for_sparta_entities() {
        let data = std::fs::read("../extracted/RP9").unwrap();
        let pack = crate::rp::parse_pack(&data).unwrap();
        let chunk = pack.chunk(1).unwrap();
        let parsed = parse_level_entities(9217, chunk).unwrap();
        assert_eq!(parsed.groups.len(), 27);
        assert_eq!(parsed.total_entities(), 1032);
        // Whole chunk consumed, 1 padding bit.
        assert_eq!(parsed.chunk_bits - parsed.bits_consumed, 1);
        // Player spawn, type 26, first (and only) position.
        let spawn = parsed.group(26).unwrap();
        assert_eq!(spawn.positions, vec![(48, 560)]);
        // Static sprite placements, type 23: 653 positions.
        assert_eq!(parsed.group(23).unwrap().positions.len(), 653);
        // Enemies: type 11 x8, type 13 x1.
        assert_eq!(parsed.group(11).unwrap().positions.len(), 8);
        assert_eq!(parsed.group(13).unwrap().positions.len(), 1);
        // All positions inside the level.
        for g in &parsed.groups {
            for &(x, y) in &g.positions {
                assert!(x >= 0 && (x as usize) < parsed.level.pixel_width());
                assert!(y >= 0 && (y as usize) < parsed.level.pixel_height() + 16);
            }
        }
    }
}

/// Evidence-backed names for group types, from how `d()` uses them.
/// Types not listed here are deliberately unnamed — their extra
/// patterns are parsed (3b-1) but their role is not yet proven.
pub fn kind_name(kind: u32) -> Option<&'static str> {
    Some(match kind {
        26 => "player spawn",
        11 => "enemy class 1 (dQ=1)",
        12 => "enemy class 2 (dQ=2)",
        13 => "enemy class 3 (dQ=3)",
        23 => "static sprite placements (registered in s())",
        3 => "camera zones (used in o() to adjust camera offset)",
        6 => "enemy action tables (aY/aZ, 40 entries each)",
        22 | 31 | 5 | 9 | 29 | 0 | 18 | 10 | 25 | 30 => "tile-property marker (ORs a flag into iW)",
        7 | 17 | 21 | 32 | 36 => "sprite object (own sprite set)",
        27 | 34 | 35 => "sprite object + tile flags",
        8 => "tile-flag marker + extra (m10, m3)",
        14 => "tile-flag marker + 15 animation pairs",
        _ => return None,
    })
}

/// Start camera, from `t()` + the clamping at the end of `o()`:
///
///   iQ = ga - 120 - ((hu ? 1 : -1) * 240 >> 2)
///   iR = gb - 266
///   clamp iQ to [0, pixel_width - 240], iR to [0, pixel_height - 320]
///
/// For For Sparta: spawn (48, 560), facing_right = true, so
/// iQ = 48 - 120 - 60 = -132 -> clamped to 0, iR = 294.
/// That is the first gameplay frame's camera, before any smoothing
/// in `o()` (which starts from the same unclamped values and
/// clamps on its first call, so the visible result is the same).
pub fn start_camera(level: &Level, spawn: (i32, i32)) -> (i32, i32) {
    let (ga, gb) = spawn;
    let look = if level.facing_right { 60 } else { -60 };
    let raw_q = ga - 120 - look;
    let raw_r = gb - 266;
    let max_q = (level.pixel_width() as i32 - 240).max(0);
    let max_r = (level.pixel_height() as i32 - 320).max(0);
    (raw_q.clamp(0, max_q), raw_r.clamp(0, max_r))
}

#[cfg(test)]
mod spawn_tests {
    use super::*;
    #[test]
    fn for_sparta_start_camera() {
        let data = std::fs::read("../extracted/RP9").unwrap();
        let pack = crate::rp::parse_pack(&data).unwrap();
        let chunk = pack.chunk(1).unwrap();
        let parsed = parse_level_entities(9217, chunk).unwrap();
        assert!(parsed.level.facing_right);
        let spawn = parsed.group(26).unwrap().positions[0];
        assert_eq!(spawn, (48, 560));
        assert_eq!(start_camera(&parsed.level, spawn), (0, 294));
        assert_eq!(kind_name(26), Some("player spawn"));
        assert_eq!(kind_name(23), Some("static sprite placements (registered in s())"));
        assert_eq!(kind_name(4), None); // not yet proven, stays unnamed
    }
}
