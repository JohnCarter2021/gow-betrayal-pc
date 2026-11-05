//! Type-9 switches and type-35 doors (slice 23).
//!
//! Trace (from `decompiled/e.java`):
//!
//! * Type 9 (case ~3060): `cX[i] = m(3)` (switch id),
//!   `len = m(10)`; the loader sets flag 256 on the
//!   column `b(x, y+k, 256)` for `k` in `0..=len`
//!   (slice 20). `cY`/`cZ` are the switch's tile
//!   position (the column top).
//! * Type 35 (case ~2976): `cS = m(3)` (door id),
//!   `cU = m(10)`, `cV = m(1)==1` (open), `cW =
//!   m(1)==1`; slot `100+i` <- anim set 2049,
//!   action `cT = (cU<<2) + 2 - (cV? 2: 0)`; bbox
//!   via sprite set 2052 -> `cO = P[2]/16`, `cQ =
//!   P[3]/16`; footprint flag 1 over
//!   `(cK, cL-cQ+1, cO, cQ)` when `!cV` (slice 20).
//! * `g()` flag-256 handler (e.java ~3999-4024):
//!   when the player stands on a flag-256 tile,
//!   scan up to the column top `n8`, find the
//!   switch with `(cY, cZ) == (tile_x, n8)`, toggle
//!   `cV` for every type-35 door whose `cS` == the
//!   switch's `cX`, then clear flag 256 up the
//!   whole column (single-use per column).
//! * Type-35 update (e.java ~5660): the door's
//!   slot action `cT` cycles between `(cU<<2)+0`
//!   and `(cU<<2)+2`; when closed (`!cV`) and the
//!   action reaches offset 1, flag 1 is re-set over
//!   the footprint (the runtime re-set). When open,
//!   flag 1 is never re-set, so the door stays
//!   passable.
//!
//! Port (documented simplification): when a switch
//! toggles a door, the port updates the tile flags
//! immediately (clear flag 1 over the footprint when
//! the door opens, set it when it closes) instead
//! of waiting for the door's animation cycle to
//! reach the re-set point. The observable result
//! (the door opens/closes) is the same; the
//! per-frame timing differs. The door's sprite is
//! not drawn (type-35 objects are not in the
//! port's static-sprite draw) — a visual gap, the
//! door is invisible but solid/passable per flags.
//!
//! 9217: one switch at tile (208,19), id 2, len 6
//! (flag-256 column rows 19-25); it toggles the
//! door at (217,24) (id 2, starts open). The
//! pillar door at (213,32) (id 6) has no switch —
//! it opens from the type-34 lever at (206,32)
//! (cH=[6]), pulled by Down on the temple floor.
//! That lever is the last gate before the
//! level-end trigger at tile 214.

use crate::anim::AnimSlot;
use crate::entities::LevelEntities;
use crate::player::TileFlags;
use crate::resources::Resources;

/// A type-9 switch: a flag-256 column the player
/// touches to toggle doors.
#[derive(Debug, Clone)]
pub struct Switch {
    /// Switch id `cX` (matches a door's `cS`).
    pub id: u32,
    /// Tile position `(cY, cZ)` (column top).
    pub x: i32,
    pub y: i32,
    /// Column length `len` (flag 256 covers rows
    /// `y..=y+len`).
    pub len: u32,
}

/// A type-35 door/gate: an animated sprite object
/// whose footprint is solid when closed (`!cV`)
/// and passable when open (`cV`).
pub struct Door {
    /// Door id `cS` (matches a switch's `cX`).
    pub id: u32,
    /// Action base `cU` (slot action `cT` is
    /// `(cU<<2)+0` or `+2`).
    pub c_u: u32,
    /// Open flag `cV`: true = open (passable).
    pub open: bool,
    /// Footprint top-left tile `(cK, cL-cQ+1)`.
    pub x: i32,
    pub y: i32,
    /// Footprint size in tiles `(cO, cQ)`.
    pub w: i32,
    pub h: i32,
    /// Anim slot (100+i <- set 2049). The slot
    /// action follows `cT`; the port keeps it for
    /// the door's visual when drawn.
    pub slot: AnimSlot,
}

/// Load the type-9 switches from the parsed level
/// (group 9: positions are px; extras are
/// `[len, cX]` per item — the loader reads
/// `m(10)` (len) then `m(3)` (cX), so extra[0]
/// is the column length and extra[1] is the id).
pub fn load_switches(parsed: &LevelEntities) -> Vec<Switch> {
    let Some(group) = parsed.group(9) else {
        return Vec::new();
    };
    group
        .positions
        .iter()
        .enumerate()
        .map(|(i, &(px, py))| Switch {
            id: group.extra.get(i * 2 + 1).copied().unwrap_or(0),
            x: px / 16,
            y: py / 16,
            len: group.extra.get(i * 2).copied().unwrap_or(0),
        })
        .collect()
}

/// Load the type-35 doors from the parsed level
/// (group 35: positions are px; extras are
/// `[cS, cU, cV, cW]` per item). The bbox comes
/// from anim set 2049 + sprite set 2052 (the
/// same pair `load_footprints` uses); `palette_key`
/// is the level's sprite key for set 2052 (pass
/// `palette_key(2062, level_index)`).
pub fn load_doors(
    res: &mut Resources,
    parsed: &LevelEntities,
    palette_key: Option<u32>,
) -> Result<Vec<Door>, String> {
    let Some(group) = parsed.group(35) else {
        return Ok(Vec::new());
    };
    let anim_chunk = res.chunk(2049).map_err(|e| format!("door anim 2049: {e}"))?;
    let anim_set = crate::anim::load_anim_set(&anim_chunk).map_err(|e| format!("door anim 2049: {e}"))?;
    let sprite_set = res
        .sprite_set(2052, palette_key)
        .map_err(|e| format!("door sprite 2052: {e}"))?;
    let mut doors = Vec::new();
    for (i, &(px, py)) in group.positions.iter().enumerate() {
        let c_s = group.extra.get(i * 4).copied().unwrap_or(0);
        let c_u = group.extra.get(i * 4 + 1).copied().unwrap_or(0);
        let c_v = group.extra.get(i * 4 + 2).copied().unwrap_or(0) != 0;
        // The bbox is taken from the FIRST cT
        // (`(cU<<2)+0`), before cT is reassigned to
        // `(cU<<2)+2-(cV? 2: 0)` — matching
        // `load_footprints` (slice 20).
        let bbox_action = (c_u << 2) as usize;
        let action = (c_u << 2) + 2 - if c_v { 2 } else { 0 };
        let (w_px, h_px) = crate::entities::action_bbox(&anim_set, &sprite_set, bbox_action);
        let c_o = (w_px / 16).max(1);
        let c_q = (h_px / 16).max(1);
        let c_k = px / 16;
        let c_l = py / 16;
        doors.push(Door {
            id: c_s,
            c_u,
            open: c_v,
            x: c_k,
            y: c_l - c_q + 1,
            w: c_o,
            h: c_q,
            slot: AnimSlot::new(&anim_set, action as usize),
        });
    }
    Ok(doors)
}

/// A type-34 lever/switch (slice 23): pulling it
/// (Down press near it) toggles the doors whose
/// `cS` is in its `cH` list and flips `cD`.
/// Trace (pressed-key case 6, e.java ~4920): near
/// a lever -> `ga = cy` (snap to the lever's x),
/// `s(15)` if `!cD` else `s(17)`, `cD = !cD`.
/// The `x(n)` helper (e.java ~7209) toggles the
/// doors: for each `cH` value, every door with
/// `cS == cH` flips `cV` (and sets `ay`/`az` +
/// `iB` when `cW`, a camera cue the port skips).
/// 9217 levers: (137,25) cH=[1,0,0,0], (169,32)
/// cH=[3,0,0,0], (206,32) cH=[6,0,0,0] — the
/// last opens the pillar door (cS=6) before the
/// level-end trigger.
/// Port (documented simplification): the pull
/// animations s(15)/s(17) are not played (the
/// player stays in place); the doors toggle and
/// the flags update immediately. The lever is
/// edge-triggered on the Down press (the port
/// tracks the previous `block_held`).
#[derive(Debug, Clone)]
pub struct Lever {
    /// Lever tile x (`cy/16`, the snap target).
    pub x: i32,
    /// Lever tile y.
    pub y: i32,
    /// Door ids this lever toggles (`cH`, 4 values;
    /// 0 = no door).
    pub door_ids: [u32; 4],
    /// Lever state `cD` (flips each pull).
    pub flipped: bool,
}

/// Load the type-34 levers from the parsed level
/// (group 34: positions are px; extras are
/// `[cA, cH0, cH1, cH2, cH3]` per item).
pub fn load_levers(parsed: &LevelEntities) -> Vec<Lever> {
    let Some(group) = parsed.group(34) else {
        return Vec::new();
    };
    group
        .positions
        .iter()
        .enumerate()
        .map(|(i, &(px, py))| Lever {
            x: px / 16,
            y: py / 16,
            door_ids: [
                group.extra.get(i * 5 + 1).copied().unwrap_or(0),
                group.extra.get(i * 5 + 2).copied().unwrap_or(0),
                group.extra.get(i * 5 + 3).copied().unwrap_or(0),
                group.extra.get(i * 5 + 4).copied().unwrap_or(0),
            ],
            flipped: false,
        })
        .collect()
}

/// Toggle the doors whose id is in `ids`,
/// updating the tile flags over each door's
/// footprint (shared by the switch and lever
/// handlers).
fn toggle_doors_by_id(doors: &mut [Door], flags: &mut TileFlags, ids: &[u32]) {
    for door in doors.iter_mut() {
        if !ids.contains(&door.id) {
            continue;
        }
        door.open = !door.open;
        for dy in 0..door.h {
            for dx in 0..door.w {
                if door.open {
                    flags.clear_flag(door.x + dx, door.y + dy, 1);
                } else {
                    flags.set_flag(door.x + dx, door.y + dy, 1);
                }
            }
        }
    }
}

/// Pull a lever (slice 23): the player is grounded
/// (action 0/9) at `(ga, gb)` and Down was just
/// pressed. If a lever is near (within 16 px
/// horizontally and 32 px vertically of its tile
/// centre), snap the player to the lever's x,
/// flip the lever, and toggle its doors. Returns
/// true when a lever was pulled.
/// Type-28 rectangle door trigger (Phase 5).
/// `x`/`y` are the top-left tile (`da`/`db`),
/// `w`/`h` the size in tiles (`dc`/`dd`, inclusive),
/// `door_id` the door it toggles (`de`, m(3)),
/// `count` the fire limit (`dg`, m(10)) — the
/// trigger fires at most `count` times, and only
/// when `count > 0`. `fired` is `df` (starts 0);
/// `cooldown_ms` is `gK` (2000 ms after each fire).
pub struct RectTrigger {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub door_id: u32,
    pub count: u32,
    pub fired: u32,
    pub cooldown_ms: i32,
}

/// Load the type-28 rectangle triggers from the
/// parsed level (group 28: positions are px;
/// extras are `[dc, dd, de, dg]` per item).
pub fn load_rect_triggers(parsed: &LevelEntities) -> Vec<RectTrigger> {
    let Some(group) = parsed.group(28) else {
        return Vec::new();
    };
    group
        .positions
        .iter()
        .enumerate()
        .map(|(i, &(px, py))| RectTrigger {
            x: px / 16,
            y: py / 16,
            w: group.extra.get(i * 4).copied().unwrap_or(0) as i32,
            h: group.extra.get(i * 4 + 1).copied().unwrap_or(0) as i32,
            door_id: group.extra.get(i * 4 + 2).copied().unwrap_or(0),
            count: group.extra.get(i * 4 + 3).copied().unwrap_or(0),
            fired: 0,
            cooldown_ms: 0,
        })
        .collect()
}

/// Type-28 rectangle trigger (Phase 5): when the
/// player stands on a flag-32 tile inside a
/// rectangle, and the trigger still has fires
/// left (`fired < count`, `count > 0`) and its
/// cooldown has expired, toggle the door `de`
/// and start the 2-second cooldown (`gK = 2000`).
/// Call once per tick with the player's tile;
/// the cooldown decrements by 40 ms per call.
/// Returns the door id toggled, if any.
pub fn check_rect_trigger(
    triggers: &mut [RectTrigger],
    doors: &mut [Door],
    flags: &mut TileFlags,
    tile_x: i32,
    tile_y: i32,
) -> Option<u32> {
    for trig in triggers.iter_mut() {
        if trig.cooldown_ms > 0 {
            trig.cooldown_ms -= 40;
        }
        if tile_x < trig.x || tile_x > trig.x + trig.w || tile_y < trig.y || tile_y > trig.y + trig.h {
            continue;
        }
        if flags.get(tile_x, tile_y) & 32 == 0 {
            continue;
        }
        if trig.count == 0 || trig.fired >= trig.count || trig.cooldown_ms > 0 {
            continue;
        }
        trig.fired += 1;
        trig.cooldown_ms = 2000;
        let ids = [trig.door_id];
        toggle_doors_by_id(doors, flags, &ids);
        return Some(trig.door_id);
    }
    None
}

/// Lever pull (Phase 5): when the player presses
/// Down near a type-34 lever, the lever flips and
/// its doors toggle. Returns `(snap_x, action)`:
/// the player snaps to the lever x and plays the
/// pull animation — action 15 when the lever was
/// un-pulled (`!cD`), action 17 when it was pulled
/// (`cD`), matching `s(15)` / `s(17)` in e.java.
/// The doors toggle at once (the original flips
/// `cD` in the same tick); the animation is
/// cosmetic and plays out to idle.
pub fn check_lever(
    levers: &mut [Lever],
    doors: &mut [Door],
    flags: &mut TileFlags,
    ga: i32,
    gb: i32,
) -> Option<(i32, usize)> {
    for lever in levers.iter_mut() {
        let lx = lever.x * 16 + 8;
        let ly = lever.y * 16;
        if (ga - lx).abs() <= 16 && (gb - ly).abs() <= 32 {
            let action = if lever.flipped { 17 } else { 15 };
            lever.flipped = !lever.flipped;
            let ids = lever.door_ids;
            toggle_doors_by_id(doors, flags, &ids);
            return Some((lx, action));
        }
    }
    None
}
pub fn check_switch(
    switches: &[Switch],
    doors: &mut [Door],
    flags: &mut TileFlags,
    tile_x: i32,
    tile_y: i32,
) -> bool {
    if !flags.has(tile_x, tile_y, 256) {
        return false;
    }
    // Column top: scan up while flag 256 holds.
    let mut top = tile_y;
    while flags.has(tile_x, top, 256) {
        top -= 1;
    }
    top += 1;
    // Find the switch at (tile_x, top).
    let Some(sw) = switches.iter().find(|s| s.x == tile_x && s.y == top) else {
        return false;
    };
    let id = sw.id;
    toggle_doors_by_id(doors, flags, &[id]);
    // Clear flag 256 up the column (single-use).
    let mut y = top;
    while flags.has(tile_x, y, 256) {
        flags.clear_flag(tile_x, y, 256);
        y += 1;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::TileFlags;
    use crate::resources::Resources;

    /// Slice 23: the 9217 switch (id 2 at (208,19),
    /// len 6 — flag-256 column rows 19-25) toggles
    /// the door at (217,24) (id 2). The pillar door
    /// at (213,32) (id 6) has no switch; it is not
    /// part of the level-1 route (see the module
    /// doc). Extras are `[len, cX]` — the loader
    /// reads `m(10)` then `m(3)`.
    #[test]
    fn switch_toggles_door_open() {
        let mut res = Resources::new("../extracted");
        let chunk = res.chunk(9217).expect("level 9217 chunk");
        let parsed = crate::entities::parse_level_entities(9217, &chunk).unwrap();
        let footprints = crate::entities::load_footprints(&mut res, &parsed).unwrap();
        let mut flags = TileFlags::build(&parsed, &footprints);
        let switches = load_switches(&parsed);
        assert_eq!(switches.len(), 1, "one type-9 switch in 9217");
        assert_eq!(switches[0].id, 2, "switch id is cX=2 (extras are [len, cX])");
        assert_eq!((switches[0].x, switches[0].y), (208, 19));
        assert_eq!(switches[0].len, 6, "switch column len 6 (rows 19-25)");
        let mut doors = load_doors(&mut res, &parsed, None).unwrap();
        assert_eq!(doors.len(), 6, "six type-35 doors in 9217");
        let door = doors.iter().find(|d| d.id == 2).expect("door id 2");
        assert!(door.open, "door 2 starts open (cV=1)");
        // The switch column (208,19..25) is flag 256.
        assert!(flags.has(208, 20, 256), "switch column flag 256 at (208,20)");
        assert!(flags.has(208, 25, 256), "switch column flag 256 at (208,25)");
        // Touch the switch column at row 20.
        assert!(check_switch(&switches, &mut doors, &mut flags, 208, 20));
        let door = doors.iter().find(|d| d.id == 2).expect("door id 2");
        assert!(!door.open, "door 2 closes after the switch (was open)");
        // The switch column is single-use: flag 256 gone.
        assert!(!flags.has(208, 20, 256), "switch column flag 256 cleared");
        assert!(!check_switch(&switches, &mut doors, &mut flags, 208, 20), "no re-trigger");
    }

    /// Slice 23: the lever at (206,32) (cH=[6])
    /// opens the pillar door (id 6) when pulled.
    /// The player stands on the temple floor at
    /// (206,32) and presses Down near the lever.
    #[test]
    fn lever_opens_pillar_door() {
        let mut res = Resources::new("../extracted");
        let chunk = res.chunk(9217).expect("level 9217 chunk");
        let parsed = crate::entities::parse_level_entities(9217, &chunk).unwrap();
        let footprints = crate::entities::load_footprints(&mut res, &parsed).unwrap();
        let mut flags = TileFlags::build(&parsed, &footprints);
        let mut levers = load_levers(&parsed);
        assert_eq!(levers.len(), 3, "three type-34 levers in 9217");
        let lever = levers.iter().find(|l| l.x == 206 && l.y == 32).expect("lever at (206,32)");
        assert_eq!(lever.door_ids[0], 6, "lever (206,32) toggles door 6");
        let mut doors = load_doors(&mut res, &parsed, None).unwrap();
        let door = doors.iter().find(|d| d.id == 6).expect("door id 6");
        assert!(!door.open, "pillar door starts closed");
        assert!(flags.solid(213, 30), "pillar footprint solid before the pull");
        // The player on the temple floor at the lever.
        let snap = check_lever(&mut levers, &mut doors, &mut flags, 206 * 16 + 8, 32 * 16);
        assert_eq!(snap, Some((206 * 16 + 8, 15)), "player snaps to the lever x and plays pull (15)");
        let door = doors.iter().find(|d| d.id == 6).expect("door id 6");
        assert!(door.open, "pillar door opens after the pull");
        assert!(!flags.solid(213, 30) && !flags.solid(214, 32), "pillar footprint cleared");
        // Pulling again closes it.
        let snap = check_lever(&mut levers, &mut doors, &mut flags, 206 * 16 + 8, 32 * 16);
        assert_eq!(snap, Some((206 * 16 + 8, 17)), "second pull plays un-pull (17)");
        let door = doors.iter().find(|d| d.id == 6).expect("door id 6");
        assert!(!door.open, "pillar door closes on the second pull");
    }
}

