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

use crate::anim;
use crate::level::{bits_needed, BitReader, Level};
use crate::resources::{Resources, SpriteSet};
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
/// Type-3 camera zones (slice 23): one
/// `CameraZone` per item. Positions are the
/// `aE`/`aF` px points; extras are `[aH, aG]`
/// per item (`aH` = vertical, `aG` = negative
/// direction). Empty when the level has no
/// type-3 group.
pub fn camera_zones(parsed: &LevelEntities) -> Vec<crate::player::CameraZone> {
    let Some(group) = parsed.group(3) else {
        return Vec::new();
    };
    group
        .positions
        .iter()
        .enumerate()
        .map(|(i, &(x, y))| crate::player::CameraZone {
            x,
            y,
            vertical: group.extra.get(i * 2).copied().unwrap_or(0) != 0,
            negative: group.extra.get(i * 2 + 1).copied().unwrap_or(0) != 0,
        })
        .collect()
}

pub fn start_camera(level: &Level, spawn: (i32, i32)) -> (i32, i32) {
    let (ga, gb) = spawn;
    let look = if level.facing_right { 60 } else { -60 };
    let raw_q = ga - 120 - look;
    let raw_r = gb - 266;
    let max_q = (level.pixel_width() as i32 - 240).max(0);
    let max_r = (level.pixel_height() as i32 - 320).max(0);
    (raw_q.clamp(0, max_q), raw_r.clamp(0, max_r))
}

/// A rectangular tile footprint that sets (and/or clears)
/// tile flags at level load, from a sprite object's bbox.
/// `kind`/`item` identify the source item (group kind and
/// position index within the group) so `TileFlags::build`
/// can apply footprints in stream order, matching the
/// original's loader. `set` flags are ORed in, `clear`
/// flags are ANDed out. (x, y) is the footprint's top-left
/// tile; (w, h) its size in tiles.
#[derive(Debug, Clone, Copy)]
pub struct Footprint {
    pub kind: u32,
    pub item: usize,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub set: i16,
    pub clear: i16,
}

/// Bbox of an anim action's timeline-0 composition, in
/// pixels (width, height), mirroring
/// `enemies::enemy_bbox_height` for both dims.
pub fn action_bbox(anim_set: &anim::AnimSet, sprite_set: &SpriteSet, action: usize) -> (i32, i32) {
    let Ok(frames) = anim::frames_for_timeline_frame(anim_set, action, 0, 0) else {
        return (0, 0);
    };
    let (mut l, mut r, mut t, mut b) = (i32::MAX, i32::MIN, i32::MAX, i32::MIN);
    for fr in frames {
        let Some(f) = sprite_set.frame(fr.sprite_idx) else {
            continue;
        };
        let x0 = fr.dx + f.anchor_x;
        let y0 = fr.dy + f.anchor_y;
        l = l.min(x0);
        r = r.max(x0 + f.w);
        t = t.min(y0);
        b = b.max(y0 + f.h);
    }
    if r == i32::MIN {
        (0, 0)
    } else {
        (r - l, b - t)
    }
}

/// Sprite footprints for the tile flags (slice 20), from
/// `e.java`'s level-load cases. The palette key is
/// irrelevant here: only the frame rects feed the bbox,
/// and those do not depend on the palette, so all three
/// sprite sets load with no key.
///
/// - Type 27 (boxes): anim set 6144 + sprite set 6146.
///   Per item: `eN = extra[0]` (sprite/action), `eW =
///   extra[1]` (climbable). Bbox `P` gives `eU = P[2]/16`,
///   `eV = P[3]/16`; footprint tiles `(eQ+i, eR-k)` for
///   `i in 0..eU`, `k in 1..=eV` get flags 1 and 512 (and
///   8 when `eW`), plus the base tile `(eQ, eR)` gets 1.
/// - Type 7: anim set 2048 + sprite set 2051. Per item:
///   `fa = extra[0] << 1`. The footprint (flag 1 on
///   `(fb+i, fc-k)`, `i in 0..fh`, `k in 1..=fi`) is set
///   only when the slot's repeat is non-zero, i.e.
///   `b(116+i, 1)` returned true; when it is 0 the
///   original runs `j(i, 2); q(i);` instead, which is not
///   ported (no such item in 9217: action 2 repeats).
/// - Type 35: anim set 2049 + sprite set 2052. Per item:
///   `cU = extra[1]`, `cV = extra[2]`. Bbox of sprite
///   `cU << 2` gives `cO = P[2]/16`, `cQ = P[3]/16`;
///   footprint `(cK+i, cL-k)`, `i in 0..cO`, `k in 0..cQ`:
///   flag 1 is SET when `!cV`, CLEARED when `cV` (a
///   doorway/gate the player can pass through).
pub fn load_footprints(res: &mut Resources, parsed: &LevelEntities) -> Result<Vec<Footprint>, String> {
    let mut out = Vec::new();
    if let Some(g) = parsed.group(27) {
        let anim_chunk = res.chunk(6144).map_err(|e| format!("{e:?}"))?;
        let anim = anim::load_anim_set(&anim_chunk).map_err(|e| e.to_string())?;
        let sprites = res.sprite_set(6146, None).map_err(|e| format!("{e:?}"))?;
        for (p, &(x, y)) in g.positions.iter().enumerate() {
            let e_n = g.extra[p * 2] as usize;
            let e_w = g.extra[p * 2 + 1] != 0;
            let (w, h) = action_bbox(&anim, &sprites, e_n);
            let (e_u, e_v) = (w / 16, h / 16);
            let (e_q, e_r) = (x / 16, y / 16);
            let mut set: i16 = 1 | 512;
            if e_w {
                set |= 8;
            }
            out.push(Footprint { kind: 27, item: p, x: e_q, y: e_r - e_v, w: e_u, h: e_v, set, clear: 0 });
            out.push(Footprint { kind: 27, item: p, x: e_q, y: e_r, w: 1, h: 1, set: 1, clear: 0 });
        }
    }
    if let Some(g) = parsed.group(7) {
        let anim_chunk = res.chunk(2048).map_err(|e| format!("{e:?}"))?;
        let anim = anim::load_anim_set(&anim_chunk).map_err(|e| e.to_string())?;
        let sprites = res.sprite_set(2051, None).map_err(|e| format!("{e:?}"))?;
        for (p, &(x, y)) in g.positions.iter().enumerate() {
            let fa = (g.extra[p * 3] as usize) << 1;
            if anim::AnimSlot::new(&anim, fa).repeat == 0 {
                continue;
            }
            let (w, h) = action_bbox(&anim, &sprites, fa);
            let (f_h, f_i) = (w / 16, h / 16);
            let (f_b, f_c) = (x / 16, y / 16);
            out.push(Footprint { kind: 7, item: p, x: f_b, y: f_c - f_i, w: f_h, h: f_i, set: 1, clear: 0 });
        }
    }
    if let Some(g) = parsed.group(35) {
        let anim_chunk = res.chunk(2049).map_err(|e| format!("{e:?}"))?;
        let anim = anim::load_anim_set(&anim_chunk).map_err(|e| e.to_string())?;
        let sprites = res.sprite_set(2052, None).map_err(|e| format!("{e:?}"))?;
        for (p, &(x, y)) in g.positions.iter().enumerate() {
            let c_u = g.extra[p * 4 + 1] as usize;
            let c_v = g.extra[p * 4 + 2] != 0;
            let (w, h) = action_bbox(&anim, &sprites, c_u << 2);
            let (c_o, c_q) = (w / 16, h / 16);
            let (c_k, c_l) = (x / 16, y / 16);
            let (set, clear) = if c_v { (0, 1) } else { (1, 0) };
            out.push(Footprint { kind: 35, item: p, x: c_k, y: c_l - c_q + 1, w: c_o, h: c_q, set, clear });
        }
    }
    Ok(out)
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




