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
use crate::boxes::{self, GameBox};
use crate::doors::{self, Door, Lever, Switch};
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

/// Grapple state (Phase 8, e.java `fR`/`fS`/`fQ`/`fT`):
/// Down on a weakened enemy starts the button-mashing
/// mini-game. `enemy` is the grappled enemy's index
/// (`ge`); `f_q` is the mashes needed (2 for kind-12,
/// 3 for kind-13 enemies); `f_s` is the mashes done
/// (-1 = a wrong button broke the grapple); `f_t` is
/// the currently required button (0 Up, 1 Left,
/// 2 Attack, 3 Right, 4 Down). The slot-131 bar
/// (shared with the boss bar) shows the prompt for
/// `f_t` via anim 1027 action `fU[f_t]`.
pub struct GrappleState {
    pub enemy: usize,
    pub f_q: i32,
    pub f_s: i32,
    pub f_t: i32,
}

/// Prompt actions `fU` (e.java:593): fT -> anim 1027
/// action, the icon the slot-131 bar shows for the
/// required button.
pub const GRAPPLE_PROMPT_ACTIONS: [usize; 5] = [2, 4, 5, 6, 8];

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
    /// Max magic `hn = hp*192 + 1024` (Phase 5):
    /// rises by 192 per magic upgrade (ft=3, fs=0).
    pub max_magic: i32,
    /// Health level `ho` (Phase 5): rises by 1 per
    /// health upgrade (ft=3, fs>0); max health is
    /// `ho*192 + 1024`.
    pub health_level: i32,
    /// Magic level `hp` (Phase 5): rises by 1 per
    /// magic upgrade; max magic is `hp*192 + 1024`.
    pub magic_level: i32,
    /// Level-placed collectibles, type-4 (Phase 5):
    /// orb bundles (ft 0-2) and upgrades (ft 3).
    pub collectibles: Vec<crate::collectibles::Collectible>,
    rng: crate::pickups::OrbRng,
    /// Script triggers and playback (slice 13).
    pub script: ScriptEngine,
    /// Set when a script's lone `*` fires: the index
    /// of the next level (level_index + 1; 10 means
    /// all levels done). The caller (Game, --play)
    /// consumes it and advances or returns to menu.
    pub level_complete: Option<usize>,
    /// Pause menu, state 102 (Phase 5, rest):
    /// `Some` while the menu is open; the world
    /// freezes (tick is a no-op) until it closes.
    pub menu: Option<crate::pause_menu::PauseMenu>,
    /// Arena kill count `dI` (Phase 5, rest):
    /// incremented on every enemy death; shown
    /// as "Kills: N" in the Arena HUD.
    pub kills: i32,
    /// Weakened-marker animation (slice 14): anim
    /// set 1027, sprite set 1040 (`aU`), slot for
    /// action 10, advanced once per tick. None when
    /// either set fails to load (markers disabled).
    bar: Option<BarState>,
    /// Boss bar (Phase 2): anim set 1027, sprite
    /// set 1040 (`aU`), slot 131. The slot is
    /// (re)set to the latched action `aT` when an
    /// `@` trigger latches; it advances (t(131))
    /// each tick while latched. None when either
    /// set fails to load (bar disabled).
    boss_bar: Option<BarState>,
    /// Stagger indicator (Phase 6): anim set 7168
    /// action 31, sprite set 7169 (`bH`), slot 137
    /// (e.java:3415 `a(137, 7168, 31)` at level
    /// load). Drawn at each enemy with `dX > 0`
    /// (e.java:2567-2570); the slot advances 40 ms
    /// per staggered enemy per draw (the original's
    /// `b(137, L)`). None when either set fails to
    /// load (indicator disabled).
    stagger_bar: Option<BarState>,
    /// Collectible sets (Phase 8): anim set 5120 +
    /// sprite set 5123, slots 121+n (e.java:3296).
    /// Drawn at each uncollected item's position
    /// with action `ft` (0 gold, 1 health, 2 magic,
    /// 3 upgrade). None when either set fails to
    /// load (collectibles stay invisible).
    pub collectible_sets: Option<(AnimSet, SpriteSet)>,
    /// Previous tick's player health `hk` (Phase 6):
    /// the hurt check compares it with the current
    /// health to find the damage taken this tick
    /// (e.java:5378-5400, `hk - hj`). Updated at
    /// the end of every tick.
    prev_player_health: i32,
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
    /// Grapple mini-game (Phase 8): `Some` while
    /// the player is grappling an enemy (`fR`).
    /// None otherwise.
    pub grapple: Option<GrappleState>,
    /// Key code of the most recent discrete press
    /// (Phase 8, the grapple mash reads `s`): set
    /// by Game from the Input before each tick
    /// (Up 1, Left 2, Right 5, Down 6, Attack 8,
    /// Select 27, Exit 29); Gameplay consumes it
    /// (resets to 0) at the start of each tick.
    /// Tests set it directly.
    pub key_code: i32,
    /// Type-27 boxes (slice 22) and their anim /
    /// sprite sets (6144 / 6146).
    pub boxes: Vec<GameBox>,
    box_anim: AnimSet,
    box_sprites: SpriteSet,
    /// Type-9 switches and type-35 doors (slice 23):
    /// the switch columns toggle doors open/closed.
    pub switches: Vec<Switch>,
    pub doors: Vec<Door>,
    /// Type-34 levers (slice 23): Down near one
    /// toggles its doors (the (206,32) lever opens
    /// the pillar door before the level end).
    pub levers: Vec<Lever>,
    /// Type-28 rectangle door triggers (Phase 5):
    /// standing on a flag-32 tile inside a rect
    /// toggles its door (up to `count` times).
    pub rect_triggers: Vec<doors::RectTrigger>,
    /// Down held on the previous tick (slice 23):
    /// a lever pull fires only on the press edge
    /// (block_held false -> true).
    prev_block_held: bool,
    /// Held direction on the previous tick (slice 22):
    /// a push/pull starts only on a fresh press edge
    /// (dir != prev_dir) while grabbing.
    prev_dir: i32,
    /// Cutscene playback (Phase 4, state 105): the
    /// type-6 aY/aZ action tables, loaded at level
    /// load. `cutscene_ba`/`cutscene_bb` are the
    /// current table indices (persist across
    /// cutscenes within a level, reset only on
    /// level load, D()). `cutscene_active` is true
    /// while a cutscene is playing.
    cutscene_ay: [u32; 40],
    cutscene_az: [u32; 40],
    cutscene_ba: usize,
    cutscene_bb: usize,
    /// Cutscene target enemy `dL` (the enemy whose
    /// spawn bit extra[2] is set), or None when no
    /// enemy has it (player-only cutscene).
    cutscene_target: Option<usize>,
    pub cutscene_active: bool,
    /// Flag-4 enemy respawn (Phase 4, type-14):
    /// triggers that refresh off-screen enemies
    /// when the player crosses them. Each trigger
    /// has a tile position and, per enemy index,
    /// a new spawn point (eE, eF) and a flag (eG).
    respawn_triggers: Vec<RespawnTrigger>,
    /// Checkpoint (Phase 7, `fY`/`fZ`/`hv`): the
    /// respawn point, set to the spawn at level
    /// load (`d(26)`) and updated whenever the
    /// player stands on a flag-1024 tile (type-25,
    /// the `g()` handler). Death respawns here.
    checkpoint_x: i32,
    checkpoint_y: i32,
    checkpoint_facing: bool,
    /// Death finished (Phase 7): the player's
    /// action-52 animation has ended, so the
    /// caller (Game) shows the death screen
    /// (state 108). Set in tick when
    /// `player.death_finished` first becomes true.
    pub death_finished: bool,
    /// Upgrade interstitial (Phase 7, state 104):
    /// `Some` while the Gorgon Eye / Phoenix
    /// Feather animation plays; the world is
    /// frozen (tick advances only the interstitial)
    /// until its timer runs out.
    upgrade: Option<UpgradeState>,
    /// Slot-136 animation for the interstitial
    /// (anim set 1026 action 0 = health, 1 =
    /// magic; `fu` = sprite set 1038). None when
    /// either set fails to load (the interstitial
    /// still runs, but draws no icon).
    upgrade_bar: Option<BarState>,
    /// Corner ornaments for the interstitial box
    /// (`cq` = sprite set 1030). None when the
    /// set fails to load.
    upgrade_corners: Option<SpriteSet>,
    /// Upgrade `he` bit pending (Phase 7): set to
    /// `Some(true)` for a health upgrade or
    /// `Some(false)` for a magic upgrade when one
    /// is collected; the caller (Game) applies it
    /// to the save (`he |= 1 << bC` / `<< 10`)
    /// and clears it. The bit is set at collection
    /// (e.java:5442/5449), before the interstitial.
    pub pending_he_health: Option<bool>,
    /// Full string table (English by default; Game
    /// replaces it on a language change): the
    /// interstitial (state 104) draws strings
    /// 147/148/151/158/189 from it.
    strings: Vec<String>,
}

/// One upgrade interstitial (Phase 7, state 104).
struct UpgradeState {
    /// `fv`: true = health (Gorgon Eye, string
    /// 147), false = magic (Phoenix Feather, 148).
    health: bool,
    /// `fw`: the countdown, 192 down by 3 per tick
    /// (~2.5 s). At 0 the interstitial ends.
    timer: i32,
    /// The max the interstitial grows toward:
    /// (new level) * 192 + 1024. The current max
    /// rises by 3 per tick toward this.
    target_max: i32,
}

/// One flag-4 respawn trigger (Phase 4, type-14).
struct RespawnTrigger {
    /// Trigger tile (eC, eD).
    tile_x: i32,
    tile_y: i32,
    /// Vertical run length (the `n74` count): flag
    /// 4 covers tiles (tile_x, tile_y ..= tile_y+len).
    len: usize,
    /// Per enemy index (dY): (new spawn x in px,
    /// new spawn y in px, flag eG). An entry with
    /// x == 0 means "no respawn for this enemy".
    entries: Vec<(i32, i32, bool)>,
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
        let footprints = entities::load_footprints(res, &parsed)?;
        let flags = TileFlags::build(&parsed, &footprints);
        let player_anim_chunk = res.chunk(28672).map_err(|e| format!("{e:?}"))?;
        let player_anim = anim::load_anim_set(&player_anim_chunk).map_err(|e| e.to_string())?;
        let player_sprites = res.sprite_set(28673, None).map_err(|e| format!("{e:?}"))?;
        let spawn = parsed
            .group(26)
            .and_then(|g| g.positions.first().copied())
            .ok_or("no player spawn (type 26)")?;
        let (start_cam_x, start_cam_y) = entities::start_camera(&parsed.level, spawn);
        // The checkpoint starts at the spawn (Phase
        // 7, `d(26)`); capture the facing before
        // `parsed` moves into the struct below.
        let spawn_facing = parsed.level.facing_right;
        let mut player = Player::new(&player_anim, spawn, spawn_facing, start_cam_x, start_cam_y);
        player.health = 1024;
        player.max_health = 1024;
        // Type-3 camera zones (slice 23).
        player.set_camera_zones(entities::camera_zones(&parsed));
        // Type-7 camera zones (Phase 8): the
        // half width `ff` comes from the zone
        // sprite's frame — anim set 2048 action
        // `fa`, drawn from sprite set 2051
        // (e.java:3211-3217, `eY = b(2051, ...)`
        // at 3396). The frame width is halved:
        // `ff` is the half width (the draw check
        // at e.java:2501 uses `ff << 1`).
        let zones_7: Vec<crate::player::CameraZone7> =
            if let (Ok(anim_chunk), Ok(sprites)) =
                (res.chunk(2048), res.sprite_set(2051, None))
            {
                if let Ok(anim_set) = anim::load_anim_set(&anim_chunk) {
                    entities::camera_zones_7(&parsed)
                        .into_iter()
                        .map(|(x, y, action, enabled, negative)| {
                            let half_w = anim::frames_for_timeline_frame(
                                &anim_set, action, 0, 0,
                            )
                            .ok()
                            .and_then(|frames| frames.first().copied())
                            .and_then(|fr| {
                                sprites
                                    .frames
                                    .get(fr.sprite_idx)
                                    .map(|f| f.w / 2)
                            })
                            .unwrap_or(0);
                            crate::player::CameraZone7 {
                                x,
                                y,
                                half_w,
                                enabled,
                                negative,
                            }
                        })
                        .collect()
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            };
        player.set_camera_zones_7(zones_7);
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
                    st.anim_set = Some(anim_set);
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
        // Boss-bar sets (Phase 2): same anim set
        // 1027 + sprite set 1040 as the marker, but
        // slot 131 with the latched action (set on
        // latch; initialised to action 2).
        let boss_bar = match (
            res.chunk(1027).ok().and_then(|c| anim::load_anim_set(&c).ok()),
            res.sprite_set(1040, None).ok(),
        ) {
            (Some(anim_set), Some(sprites)) => {
                let slot = crate::anim::AnimSlot::new(&anim_set, 2);
                Some(BarState {
                    anim: anim_set,
                    sprites,
                    slot,
                })
            }
            _ => None,
        };
        // Stagger-indicator sets (Phase 6): anim set
        // 7168 action 31 + sprite set 7169 (`bH`),
        // slot 137 (e.java:3415, level-load case 9).
        let stagger_bar = match (
            res.chunk(7168).ok().and_then(|c| anim::load_anim_set(&c).ok()),
            res.sprite_set(7169, None).ok(),
        ) {
            (Some(anim_set), Some(sprites)) => {
                let slot = crate::anim::AnimSlot::new(&anim_set, 31);
                Some(BarState {
                    anim: anim_set,
                    sprites,
                    slot,
                })
            }
            _ => None,
        };
        // Collectible sets (Phase 8): anim set 5120
        // + sprite set 5123, slots 121+n (e.java:
        // 3296 `a(121 + n, 5120, ft[n])` at level
        // load). The sprite def is 5123, not 5120:
        // anim 5120's actions reference sprite
        // indices 0-32 and set 5123 has exactly 33
        // frames (5124/5125 are other sets in RP5).
        // Actions 0-3 are the gold/health/magic/
        // upgrade icons (1 frame, repeat 63);
        // action 4 is the 3-frame collected anim.
        // None when either set fails to load
        // (collectibles stay invisible, as before).
        let collectible_sets = match (
            res.chunk(5120).ok().and_then(|c| anim::load_anim_set(&c).ok()),
            res.sprite_set(5123, None).ok(),
        ) {
            (Some(anim), Some(sprites)) => Some((anim, sprites)),
            _ => None,
        };
        // Upgrade-interstitial sets (Phase 7, state
        // 104): anim set 1026 (actions 0 = health,
        // 1 = magic) + sprite set 1038 (`fu`), slot
        // 136 (e.java:5446/5454 `a(136, 1026, 0/1)`).
        // The slot action is set when the interstitial
        // starts; the initial action here is a
        // placeholder (0). Corner ornaments `cq` =
        // sprite set 1030.
        let upgrade_bar = match (
            res.chunk(1026).ok().and_then(|c| anim::load_anim_set(&c).ok()),
            res.sprite_set(1038, None).ok(),
        ) {
            (Some(anim_set), Some(sprites)) => {
                let slot = crate::anim::AnimSlot::new(&anim_set, 0);
                Some(BarState {
                    anim: anim_set,
                    sprites,
                    slot,
                })
            }
            _ => None,
        };
        let upgrade_corners = res.sprite_set(1030, None).ok();
        // Type-27 boxes (slice 22): anim set 6144 +
        // sprite set 6146 with the level palette key
        // (6150 + bG[i]).
        let level_idx = level_index(level_id);
        let box_anim_chunk = res.chunk(boxes::BOX_ANIM_SET).map_err(|e| format!("{e:?}"))?;
        let box_anim = anim::load_anim_set(&box_anim_chunk).map_err(|e| e.to_string())?;
        let box_sprites = res
            .sprite_set(
                boxes::BOX_SPRITE_SET,
                palette_key(boxes::BOX_SPRITE_KEY_BASE, level_idx),
            )
            .map_err(|e| format!("{e:?}"))?;
        let boxes = boxes::load_boxes(
            res,
            &parsed,
            palette_key(boxes::BOX_SPRITE_KEY_BASE, level_idx),
        )?;
        // Type-9 switches and type-35 doors (slice 23).
        let switches = doors::load_switches(&parsed);
        let doors = doors::load_doors(
            res,
            &parsed,
            palette_key(2062, level_idx),
        )?;
        // Type-34 levers (slice 23).
        let levers = doors::load_levers(&parsed);
        // Type-28 rectangle door triggers (Phase 5).
        let rect_triggers = doors::load_rect_triggers(&parsed);
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
        // Type-6 cutscene action tables (Phase 4):
        // 40 aY values then 40 aZ values, each
        // 10-bit, in group(6).extra. Empty when the
        // level has no type-6 group.
        let (cutscene_ay, cutscene_az) = match parsed.group(6) {
            Some(g6) => {
                let mut ay = [0u32; 40];
                let mut az = [0u32; 40];
                for i in 0..40 {
                    ay[i] = g6.extra.get(i).copied().unwrap_or(0);
                    az[i] = g6.extra.get(40 + i).copied().unwrap_or(0);
                }
                (ay, az)
            }
            None => ([0u32; 40], [0u32; 40]),
        };
        // Cutscene target `dL`: the enemy whose spawn
        // bit (extra[2]) is set, as an index into
        // enemy_states (None when no enemy has it).
        let cutscene_target = enemy_states
            .iter()
            .position(|st| st.enemy.cutscene_target);
        // Flag-4 respawn triggers (Phase 4, type-14):
        // per item, extra = [count, (eE, eF, eG) x15].
        // The trigger tile is the item's position.
        let mut respawn_triggers = Vec::new();
        if let Some(g14) = parsed.group(14) {
            for (i, &(px, py)) in g14.positions.iter().enumerate() {
                let base = i * 46;
                let len = g14.extra.get(base).copied().unwrap_or(0) as usize;
                let mut entries = Vec::with_capacity(15);
                for k in 0..15 {
                    let ee = g14.extra.get(base + 1 + k * 3).copied().unwrap_or(0) as i32;
                    let ef = g14.extra.get(base + 1 + k * 3 + 1).copied().unwrap_or(0) as i32;
                    let eg = g14.extra.get(base + 1 + k * 3 + 2).copied().unwrap_or(0) != 0;
                    entries.push((ee * 16, ef * 16, eg));
                }
                respawn_triggers.push(RespawnTrigger {
                    tile_x: px / 16,
                    tile_y: py / 16,
                    len,
                    entries,
                });
            }
        }
        // Level-placed collectibles, type-4 (Phase 5).
        // Loaded before `parsed` moves into the struct.
        let collectibles = crate::collectibles::load_collectibles(&parsed);
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
            // Maxima from the save levels (Phase 5):
            // ho/hp are 0 on a fresh save, so both
            // maxima start at 1024. `Game::load` (game.rs)
            // overwrites these from the save after load.
            max_magic: 1024,
            health_level: 0,
            magic_level: 0,
            // Level-placed collectibles, type-4 (Phase 5).
            collectibles,
            // Time-based seed; exact scatter does not
            // matter for gameplay, only for tests (which
            // seed their own rng).
            rng: crate::pickups::OrbRng(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.subsec_nanos() ^ (d.as_secs() as u32))
                    .unwrap_or(0x9E3779B9),
            ),
            script: ScriptEngine::new(triggers, strings.clone()),
            level_complete: None,
            menu: None,
            kills: 0,
            bar,
            boss_bar,
            stagger_bar,
            collectible_sets,
            prev_player_health: 1024,
            intro_index,
            intro_text,
            block_held: false,
            grapple: None,
            key_code: 0,
            boxes,
            box_anim,
            box_sprites,
            switches,
            doors,
            levers,
            rect_triggers,
            prev_block_held: false,
            prev_dir: 0,
            cutscene_ay,
            cutscene_az,
            cutscene_ba: 0,
            cutscene_bb: 0,
            cutscene_target,
            cutscene_active: false,
            respawn_triggers,
            // Checkpoint (Phase 7, `d(26)`): the
            // respawn point starts at the spawn.
            checkpoint_x: spawn.0,
            checkpoint_y: spawn.1,
            checkpoint_facing: spawn_facing,
            death_finished: false,
            upgrade: None,
            upgrade_bar,
            upgrade_corners,
            pending_he_health: None,
            strings: strings.clone(),
        })
    }

    /// Re-resolve all string-table text after a
    /// language change: script strings and the
    /// intro dialog text (slice 15).
    pub fn set_strings(&mut self, strings: Vec<String>) {
        self.intro_text = self.intro_index.and_then(|i| strings.get(i).cloned());
        self.strings = strings.clone();
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

    /// Open the pause menu (state 102, Phase 5,
    /// rest). The world freezes while it is open;
    /// the caller routes menu input to it and
    /// acts on the outcome it returns.
    pub fn open_menu(&mut self) {
        self.menu = Some(crate::pause_menu::PauseMenu::new());
    }

    /// True while the pause menu is open.
    pub fn menu_open(&self) -> bool {
        self.menu.is_some()
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
                        self.magic = (self.magic + value).min(self.max_magic);
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
                                // Orbs never select the
                                // heavy stagger variants
                                // (Phase 6: those come
                                // from a melee finisher).
                                st.apply_damage(&anim_set, value, false)
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
                        // Arena kill count `dI` (Phase 5,
                        // rest): a damage orb killed it.
                        self.kills += 1;
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
    /// Up pressed (slice 21): while holding on a wall
    /// (action 33) it climbs up one cycle; otherwise
    /// it jumps (or double-jumps, slice 19).
    /// Up while blocking (Phase 6, e.java:4799-4810):
    /// the block stance exits to idle (`hw = false;
    /// s(0)`) and the jump does NOT happen.
    pub fn jump(&mut self) -> bool {
        if self.player.blocking {
            self.player.blocking = false;
            self.player.action = 0;
            self.player.slot = anim::AnimSlot::new(&self.player_anim, 0);
            return false;
        }
        if self.player.action == 33 {
            return self.player.climb_up(&self.player_anim, &self.flags);
        }
        self.player.jump(&self.player_anim)
    }

    /// Advance the active script one step (Select key).
    /// A `*` step sets `level_complete` for the caller.
    /// During a cutscene (Phase 4) Select does nothing
    /// — the cutscene plays on its own and advances
    /// the script when it ends (C()).
    pub fn script_advance(&mut self) {
        if self.cutscene_active {
            return;
        }
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

    /// Start a box push/pull (slice 22): the player is
    /// grabbing (21) and pressed Left/Right. Pressing
    /// the facing direction pushes (22, gl = 1),
    /// pressing the opposite pulls (23, gl = 0). `u()`
    /// runs at once; on failure the player returns to
    /// the grab pose.
    fn start_box_push(&mut self, dir: i32) {
        let Some(gd) = self.player.grabbed_box else {
            return;
        };
        let push = (dir > 0) == self.player.facing_right;
        let action = if push { 22 } else { 23 };
        self.player.push_pull = if push { 1 } else { 0 };
        self.player.action = action;
        self.player.slot = anim::AnimSlot::new(&self.player_anim, action);
        let (gw, gx) = (self.player.tile_x(), self.player.walk_tile_y());
        let facing = self.player.facing_right;
        if !boxes::try_push(
            &mut self.boxes,
            gd,
            &mut self.flags,
            facing,
            action,
            gw,
            gx,
            &self.box_anim,
        ) {
            self.player.action = 21;
            self.player.slot = anim::AnimSlot::new(&self.player_anim, 21);
        }
    }

    /// Cancel the active script (Exit key).
    pub fn script_cancel(&mut self) {
        self.script.cancel();
    }

    fn handle_script_event(&mut self, ev: ScriptEvent) {
        if ev == ScriptEvent::LevelComplete {
            self.level_complete = Some(self.level_index + 1);
        }
        if ev == ScriptEvent::BossBarLatched {
            // Phase 2: an `@` trigger latched — (re)set
            // the boss-bar slot (131) to the latched
            // action `aT` (fresh slot, like `a(131,
            // 1027, aT)`).
            if let Some(bb) = &mut self.boss_bar {
                let action = self.script.boss_action();
                bb.slot = crate::anim::AnimSlot::new(&bb.anim, action as usize);
            }
        }
        if ev == ScriptEvent::Cutscene {
            // Phase 4: a `!` step — start state-105
            // playback. The player takes aY[ba] and
            // the target enemy (dL) takes aZ[bb].
            // The world is frozen while it plays.
            self.start_cutscene();
        }
    }

    /// Start a `!` cutscene (Phase 4, state 105):
    /// set the player's action to aY[ba] and the
    /// target enemy's action to aZ[bb]. When the
    /// current aY entry is 0 the cutscene ends
    /// immediately (nothing to play).
    fn start_cutscene(&mut self) {
        let action = self.cutscene_ay[self.cutscene_ba] as usize;
        if action == 0 {
            // Nothing to play: end at once (C()).
            self.end_cutscene();
            return;
        }
        self.cutscene_active = true;
        self.player.action = action;
        self.player.slot = anim::AnimSlot::new(&self.player_anim, action);
        if let Some(ti) = self.cutscene_target {
            let eaction = self.cutscene_az[self.cutscene_bb] as usize;
            if eaction != 0 {
                if let Some(st) = self.enemy_states.get_mut(ti) {
                    // The cutscene's aZ action goes
                    // through `i()` too (so `dS`
                    // stays in sync); no grapple is
                    // active during a cutscene.
                    st.frozen = false;
                    if let Some(anim_set) = st.anim_set.clone() {
                        st.set_action(&anim_set, eaction);
                    }
                }
            }
        }
    }

    /// End the cutscene (C()): back to gameplay
    /// and advance the script past the `!` step.
    fn end_cutscene(&mut self) {
        self.cutscene_active = false;
        let ev = self.script.advance();
        self.handle_script_event(ev);
        self.apply_weapon_switch();
    }

    /// One cutscene tick (Phase 4, state 105):
    /// advance the player's cutscene action and
    /// the target enemy's. When an action finishes,
    /// step the table (ba/bb); when the next aY
    /// entry is 0 and the next aZ is 0 (or there is
    /// no target), the cutscene ends (C()).
    fn tick_cutscene(&mut self) {
        // Player: advance the cutscene action.
        self.player.slot.advance(&self.player_anim, 40);
        if self.player.slot.repeat == 0 {
            let next_ay = self.cutscene_ay.get(self.cutscene_ba + 1).copied().unwrap_or(0);
            let next_az = self.cutscene_az.get(self.cutscene_bb + 1).copied().unwrap_or(0);
            let no_enemy = self.cutscene_target.is_none();
            if next_ay == 0 && (next_az == 0 || no_enemy) {
                self.end_cutscene();
                return;
            } else if next_ay == 0 {
                // Repeat the current player action.
                let action = self.cutscene_ay[self.cutscene_ba] as usize;
                self.player.action = action;
                self.player.slot = anim::AnimSlot::new(&self.player_anim, action);
            } else {
                self.cutscene_ba += 1;
                let action = self.cutscene_ay[self.cutscene_ba] as usize;
                self.player.action = action;
                self.player.slot = anim::AnimSlot::new(&self.player_anim, action);
            }
        }
        // Target enemy: advance its cutscene action.
        if let Some(ti) = self.cutscene_target {
            if let Some(st) = self.enemy_states.get_mut(ti) {
                if let Some(anim_set) = st.anim_set.clone() {
                    st.slot.advance(&anim_set, 40);
                    if st.slot.repeat == 0 {
                        let next_ay = self.cutscene_ay.get(self.cutscene_ba + 1).copied().unwrap_or(0);
                        let next_az = self.cutscene_az.get(self.cutscene_bb + 1).copied().unwrap_or(0);
                        if next_ay == 0 && next_az == 0 {
                            self.end_cutscene();
                            return;
                        } else if next_az == 0 {
                            let action = self.cutscene_az[self.cutscene_bb] as usize;
                            st.frozen = false;
                            st.set_action(&anim_set, action);
                        } else {
                            self.cutscene_bb += 1;
                            let action = self.cutscene_az[self.cutscene_bb] as usize;
                            st.frozen = false;
                            st.set_action(&anim_set, action);
                        }
                    }
                }
            }
        }
    }

    /// Mode-3 burst fire (Phase 3, `hd`): gate is
    /// magic > 256 and fewer than 10 active damage
    /// orbs (`fG < 10`). On fire: action 95, spawn
    /// type-3 orbs totalling `fL[gW]` from
    /// `(ga, gb - gD + gD/4)`, flat 256 magic cost,
    /// reset to blades (`A()`). On a failed gate:
    /// reset to blades and take a normal swing
    /// (the original's fall-through to `s(54)`).
    fn try_fire_mode3(&mut self) {
        if self.magic > crate::player::Player::MODE3_COST
            && self.pickups.damage_orb_count() < 10
        {
            self.player.attack(&self.player_anim);
            self.spawn_mode3_orbs();
            self.magic = (self.magic - crate::player::Player::MODE3_COST).max(0);
            self.player.unequip_all();
        } else {
            self.player.unequip_all();
            self.player.attack(&self.player_anim);
        }
    }

    /// Spawn the mode-3 damage orbs (Phase 3):
    /// `b(ga, gb - gD + gD/4, 3, fL[gW])`. The
    /// orbs round-robin their targets over the
    /// live enemies in the viewport (the
    /// original's `P[]`), like `b()` does.
    fn spawn_mode3_orbs(&mut self) {
        let g_w = self.player.magic_level_mode3.clamp(0, 4) as usize;
        let value = crate::player::Player::F_L[g_w];
        let x = self.player.ga;
        let y = self.player.gb - 34 + 8;
        // Viewport enemies (the original's rect
        // is iQ-64, iR-64, 368x448 around the
        // camera; the port uses the camera rect
        // expanded by 64 px).
        let cam_x = self.player.cam_x;
        let cam_y = self.player.cam_y;
        let targets: Vec<usize> = self
            .enemy_states
            .iter()
            .enumerate()
            .filter(|(_, st)| {
                !st.dead
                    && st.x >= cam_x - 64
                    && st.x < cam_x + 240 + 64
                    && st.y >= cam_y - 64
                    && st.y < cam_y + 320 + 64
            })
            .map(|(i, _)| i)
            .collect();
        self.pickups
            .spawn(x, y, crate::pickups::DAMAGE, value, &mut self.rng);
        self.pickups.assign_damage_targets(&targets);
    }

    /// One 40 ms tick. `dir`: -1 left, 0 none, +1 right.
    /// `attack`: fire key pressed this frame.
    /// Grapple target (Phase 8, e.java case 6): the
    /// nearest weakened enemy in reach. The player
    /// must be idle/walking (0/9) and grounded; the
    /// enemy is alive, its feet are at the player's
    /// feet (`gb == dU || gb+1 == dU`), it is within
    /// reach (the `e.a(...)` rect overlap, here
    /// |dx| < 48 and |dy| < 16), it is weakened
    /// (`dX <= 0`) or (kind 13 and action 12), and
    /// the tiles between the player and the enemy
    /// in the player's row are clear of flag 1
    /// (`bl5`).
    fn grapple_target(&self) -> Option<usize> {
        if !(self.player.action == 0 || self.player.action == 9) {
            return None;
        }
        if !self.flags.landing_solid(
            self.player.tile_x(),
            self.player.walk_tile_y() + 1,
            self.player.drop_platform_row,
        ) {
            return None;
        }
        let (px, py) = (self.player.ga, self.player.gb);
        let (ptx, pty) = (self.player.tile_x(), self.player.walk_tile_y());
        let mut best: Option<(i32, usize)> = None;
        for (i, st) in self.enemy_states.iter().enumerate() {
            if st.dead || st.dying {
                continue;
            }
            if st.y != py && st.y != py + 1 {
                continue;
            }
            if (st.x - px).abs() >= 48 || (st.y - py).abs() >= 16 {
                continue;
            }
            let weakened = st.dx <= 0;
            let kind13_action12 = st.enemy.kind == 13 && st.action == 12;
            if !weakened && !kind13_action12 {
                continue;
            }
            let etx = st.x / 16;
            let (lo, hi) = if ptx <= etx { (ptx, etx) } else { (etx, ptx) };
            let clear = (lo..=hi).all(|tx| self.flags.get(tx, pty) & 1 == 0);
            if !clear {
                continue;
            }
            let dist = (st.x - px).abs();
            if best.map_or(true, |(d, _)| dist < d) {
                best = Some((dist, i));
            }
        }
        best.map(|(_, i)| i)
    }

    /// Random prompt `B()` (e.java:6642-6648): a
    /// button 0-4, different from the current one
    /// (re-rolls until it differs).
    fn random_prompt(&mut self, current: i32) -> i32 {
        loop {
            let n = (self.rng.next() % 5) as i32;
            if n != current {
                return n;
            }
        }
    }

    /// Set the slot-131 bar (shared with the boss
    /// bar) to a grapple prompt action.
    fn set_grapple_bar(&mut self, prompt: i32) {
        if let Some(bb) = &mut self.boss_bar {
            let action = GRAPPLE_PROMPT_ACTIONS[prompt as usize];
            bb.slot = crate::anim::AnimSlot::new(&bb.anim, action);
        }
    }

    /// Start a grapple or finisher/throw (Phase 8,
    /// e.java case 6), by kind class (`dQ`):
    /// kind 11 + health < half -> finisher action
    /// 12; kind 12 + enemy action 12/10 -> throw
    /// action 117, else the grapple mini-game (fQ
    /// 2, player action 88); kind 13 + enemy
    /// action 12 -> throw action 113, else the
    /// grapple (fQ 3, player action 91). The player
    /// snaps to the enemy's x; the enemy faces the
    /// player. `dM`/`dN` (main/cutscene enemy) are
    /// not ported, so the throw/finisher branches
    /// use only the action and health conditions.
    fn start_grapple(&mut self, enemy: usize) {
        let st = &self.enemy_states[enemy];
        let kind = st.enemy.kind;
        let (ehealth, emax, eaction) = (st.health, st.max_health, st.action);
        let ex = st.x;
        self.player.ga = ex;
        self.player.facing_right = ex >= self.player.ga;
        self.enemy_states[enemy].facing_right = self.player.facing_right;
        match kind {
            11 if ehealth > 0 && ehealth < emax >> 1 => {
                self.player.set_action(&self.player_anim, 12);
            }
            12 if eaction == 12 || eaction == 10 => {
                self.player.set_action(&self.player_anim, 117);
            }
            12 => {
                let f_t = self.random_prompt(-1);
                self.grapple = Some(GrappleState {
                    enemy,
                    f_q: 2,
                    f_s: 0,
                    f_t,
                });
                self.set_grapple_bar(f_t);
                self.player.set_action(&self.player_anim, 88);
            }
            13 if eaction == 12 => {
                self.player.set_action(&self.player_anim, 113);
            }
            13 => {
                let f_t = self.random_prompt(-1);
                self.grapple = Some(GrappleState {
                    enemy,
                    f_q: 3,
                    f_s: 0,
                    f_t,
                });
                self.set_grapple_bar(f_t);
                self.player.set_action(&self.player_anim, 91);
            }
            _ => {}
        }
    }

    /// Mash (Phase 8, e.java:5131-5163): while the
    /// grapple is active, the pressed key (`s`) is
    /// checked against the required button `f_t`
    /// (0 Up, 1 Left, 2 Attack, 3 Right, 4 Down).
    /// Correct -> `f_s += 1`, a new `f_t`, and the
    /// bar shows the new prompt. Wrong (and not a
    /// neutral key 0/27/29/4/9) -> `f_s = -1`
    /// (the grapple is broken). The bar advances
    /// (`t(131)`) each tick — but only here when
    /// the boss bar is NOT latched, because the
    /// latched boss-bar pulse (below) already
    /// advances slot 131 each tick; advancing in
    /// both places would double the rate.
    fn tick_grapple_mash(&mut self) {
        let Some(gr) = &mut self.grapple else {
            return;
        };
        if !self.script.boss_latched() {
            if let Some(bb) = &mut self.boss_bar {
                bb.slot.advance(&bb.anim, 40);
            }
        }
        if !(gr.f_s >= 0 && gr.f_s < gr.f_q) {
            return;
        }
        let key = self.key_code;
        let want = match gr.f_t {
            0 => 1,
            1 => 2,
            2 => 8,
            3 => 5,
            _ => 6,
        };
        if key == want {
            let cur = gr.f_t;
            let next = self.random_prompt(cur);
            let gr = self.grapple.as_mut().unwrap();
            gr.f_t = next;
            gr.f_s += 1;
            self.set_grapple_bar(next);
        } else if key != 0 && key != 27 && key != 29 && key != 4 && key != 9 {
            self.grapple.as_mut().unwrap().f_s = -1;
        }
    }

    /// Resolve (Phase 8, e.java:5238-5259): called
    /// when the grapple action (88/91) ends.
    /// `f_s >= f_q` -> success: the enemy dies (its
    /// orbs spawn here, as the original's `b()`
    /// call does) and the player plays 89 (kind 12)
    /// or 92 (kind 13). Otherwise the grapple fails
    /// and the player plays 90/93. Clears the
    /// grapple and restores the boss-bar slot to the
    /// latched action when latched. (The original
    /// sets `ek = 1` and lets the finisher's hit
    /// land the kill; the port kills outright here
    /// because the finisher has no hit frame.)
    fn resolve_grapple(&mut self) {
        let Some(gr) = self.grapple.take() else {
            return;
        };
        let kind = self.enemy_states[gr.enemy].enemy.kind;
        let success = gr.f_s >= gr.f_q;
        if success {
            let (x, y, em) = {
                let st = &mut self.enemy_states[gr.enemy];
                if let Some(anim) = st.anim_set.clone() {
                    st.apply_damage(&anim, i32::MAX / 2, false);
                } else {
                    st.health = 0;
                    st.dying = true;
                }
                (st.x, st.y, st.enemy.em)
            };
            let drop_kind = if kind == 12 {
                crate::pickups::MAGIC
            } else {
                crate::pickups::GOLD
            };
            self.pickups.spawn(x, y - 16, drop_kind, em, &mut self.rng);
            let action = if kind == 13 { 92 } else { 89 };
            self.player.set_action(&self.player_anim, action);
        } else {
            let action = if kind == 13 { 93 } else { 90 };
            self.player.set_action(&self.player_anim, action);
        }
        if self.script.boss_latched() {
            if let Some(bb) = &mut self.boss_bar {
                let action = self.script.boss_action();
                bb.slot = crate::anim::AnimSlot::new(&bb.anim, action as usize);
            }
        }
    }

    pub fn tick(&mut self, res: &mut Resources, dir: i32, attack: bool) {
        // A showing intro dialog freezes the world
        // (the original's state 1 -> 107 path shows
        // the dialog before gameplay state 100).
        if self.intro_text.is_some() {
            return;
        }
        // An open pause menu (Phase 5, rest, state
        // 102) freezes the world; the menu handles
        // its own input (Game routes it there).
        if self.menu.is_some() {
            return;
        }
        // A playing cutscene (Phase 4, state 105):
        // advance the cutscene actions only — the
        // world is frozen (no input, no enemy AI).
        if self.cutscene_active {
            self.tick_cutscene();
            return;
        }
        // A showing script freezes the world (the
        // original's dialog states do not run z()).
        if self.script.active() {
            return;
        }
        // Upgrade interstitial (Phase 7, state 104):
        // the world is frozen while it plays; only
        // the interstitial itself advances (its
        // timer, the max growth, the slot-136 icon).
        if self.upgrade.is_some() {
            self.tick_upgrade();
            return;
        }
        // Grapple mash (Phase 8): the pressed key
        // (`key_code`, consumed here) is checked
        // against the required button while the
        // grapple is active.
        self.tick_grapple_mash();
        self.key_code = 0;
        // Climb turn (slice 21): Left/Right pressed
        // opposite to the facing while holding (33)
        // flips the player around the wall (44).
        if self.player.action == 33 && dir != 0 {
            self.player.climb_turn(&self.player_anim, dir);
        }
        // Block stance (slice 18): Down held starts
        // it from idle/walk (or resumes it after a
        // blocked-hit reaction ends in idle);
        // releasing Down ends it. Attacking or
        // airborne presses do not start it.
        // Down held while holding on a wall (slice
        // 21) climbs down (34) or drops (26) instead.
        if self.block_held {
            // Lever pull (slice 23, Phase 5): Down
            // pressed near a type-34 lever toggles its
            // doors and plays the pull animation
            // (action 15 pull / 17 un-pull). Fires
            // only on the press edge, while grounded.
            let down_edge = !self.prev_block_held;
            if down_edge && (self.player.action == 0 || self.player.action == 9) {
                // Grapple (Phase 8, e.java case 6):
                // Down on a weakened enemy in reach
                // starts the finisher/throw/grapple
                // instead of blocking or pulling a
                // lever. Takes priority over both.
                if let Some(enemy) = self.grapple_target() {
                    self.start_grapple(enemy);
                } else if let Some((lx, action)) = doors::check_lever(
                    &mut self.levers,
                    &mut self.doors,
                    &mut self.flags,
                    self.player.ga,
                    self.player.gb,
                ) {
                    self.player.ga = lx;
                    self.player.action = action;
                    self.player.slot = anim::AnimSlot::new(&self.player_anim, action);
                }
            }
            if self.player.action == 37 {
                // Drop from a ledge hang (slice 23):
                // Down while hanging (37) lets go (26).
                self.player.drop_from_hang(&self.player_anim);
            } else if self.player.action == 33 {
                self.player.climb_down(&self.player_anim, &self.flags);
            } else if self.player.drop_through(&self.player_anim, &self.flags) {
                // Drop through a one-way platform
                // (slice 23): Down on a flag-8192 tile
                // falls through instead of blocking.
            } else if !self.player.blocking || self.player.action == 0 {
                self.player.start_block(&self.player_anim);
            }
        } else if self.player.blocking {
            self.player.stop_block(&self.player_anim);
        }
        self.prev_block_held = self.block_held;
        if attack && !self.player.blocking {
            // Fire while grabbing (Phase 6, e.java:5095-5097):
            // the attack case falls through to
            // `if (hB != 21) break; s(0);` — the
            // grab releases to idle, no swing.
            if self.player.action == 21 {
                self.player.action = 0;
                self.player.slot = anim::AnimSlot::new(&self.player_anim, 0);
                self.player.grabbed_box = None;
            } else if self.player.is_artemis() && self.magic <= 0 {
                // `A()` (slice 17): Artemis with no
                // magic left unequips instead of swinging.
                self.player.unequip_all();
            } else if self.player.is_magic_mode3() {
                // Phase 3: burst magic (hd). Gate:
                // magic > 256 and fewer than 10 active
                // damage orbs (fG < 10).
                self.try_fire_mode3();
            } else if self.player.is_magic_mode2() {
                // Phase 3: channeled magic (hc). Gate:
                // magic > (gV+1)*4. Starts action 94;
                // the channel drains below.
                if self.magic > self.player.mode2_start_gate() {
                    self.player.attack(&self.player_anim);
                }
            } else {
                self.player.attack(&self.player_anim);
            }
        }
        // Mode-2 channel (Phase 3): while in action
        // 94, drain 12 magic per tick while attack
        // is pressed; end the cast (idle + blades)
        // when attack is released or magic runs out.
        // Approximation: the port's attack is a
        // per-tick press, not a held key, so the
        // channel continues only while attack presses
        // keep arriving each tick.
        if self.player.action == 94 {
            if attack && self.magic > 0 {
                self.magic = (self.magic
                    - crate::player::Player::MODE2_DRAIN_PER_TICK)
                    .max(0);
            } else {
                self.player.end_magic_cast(&self.player_anim);
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
        // The last enemy that landed an unblocked hit
        // this tick (Phase 6): its facing gives the
        // hurt reaction's `gf` (`!ej`, the direction
        // the player must face to look at the
        // attacker, e.java:6623).
        let mut last_attacker_facing: Option<bool> = None;
        // The grappled enemy (Phase 8) is held (`ge`):
        // it does not act or deal damage while the
        // mini-game runs, or it would interrupt the
        // grapple action (88/91) with a hurt reaction.
        let grappled = self.grapple.as_ref().map(|gr| gr.enemy);
        for (i, st) in self.enemy_states.iter_mut().enumerate() {
            // Freeze guard (dS, e.java:4354): while a
            // grapple is in progress, every enemy
            // except the grappled one is frozen to
            // idle — `set_action` enforces it.
            st.frozen = grappled.is_some() && grappled != Some(i);
            if grappled == Some(i) {
                continue;
            }
            if let Ok(anim_chunk) = res.chunk(st.enemy.anim_id) {
                if let Ok(anim_set) = anim::load_anim_set(&anim_chunk) {
                    let dmg = st.tick(&anim_set, &self.flags, px, py, &mut self.rng);
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
                            self.player.slot = anim::AnimSlot::new(
                                &self.player_anim,
                                crate::player::Player::anim_action(97, self.player.weapon_mode),
                            );
                        } else {
                            self.player.health -= dmg;
                            // The original clamps health at 0
                            // at both damage sites (e.java:5807
                            // and 6618: `if (hj < 0) hj = 0`).
                            // Without this a dead player keeps
                            // taking damage into the negatives
                            // while action 52 plays out.
                            if self.player.health < 0 {
                                self.player.health = 0;
                            }
                            last_attacker_facing = Some(!st.facing_right);
                        }
                    }
                }
            }
        }
        // Enemy respawn (Phase 7, e.java:6334-6367):
        // dead enemies count their `er` timer down
        // and respawn at their spawn point when it
        // runs out (gated on respawn count, orb
        // count, distance from the player, and a
        // clear tile above the spawn). In the Arena
        // (`iN`) the count is ignored — waves are
        // endless.
        self.check_enemy_respawn();
        // Hurt reactions (Phase 6, e.java:5378-5382):
        // the deferred check after the damage lands.
        // Guards: grounded (not 24/25/26, not
        // climbing 33-38), not in 27/28/29 or the
        // finishers 60/77, not grabbing a box
        // (21/22/23, the original's `gk`), not
        // blocking (the 97 path above already ran),
        // alive. `ge` (grapple) is not ported.
        let damage = self.prev_player_health - self.player.health;
        if damage > 0 {
            let airborne = matches!(self.player.action, 24 | 25 | 26);
            let climbing = matches!(self.player.action, 33 | 34 | 35 | 36 | 37 | 38);
            let grabbing = matches!(self.player.action, 21 | 22 | 23);
            let excluded = matches!(self.player.action, 27 | 28 | 29 | 60 | 77);
            if !airborne && !climbing && !grabbing && !excluded && !self.player.blocking && self.player.health > 0 {
                if let Some(facing) = last_attacker_facing {
                    self.player
                        .hurt_reaction(&self.player_anim, damage, facing);
                }
            }
        }
        // Collect death drops first: the hit loop holds
        // a borrow of enemy_states, so drops spawn after.
        let mut killed_drops: Vec<(i32, i32, u32, i32)> = Vec::new();
        // Hit once per swing, at frame >= 2, for any
        // ground or air swing (slice 16; the original's
        // ht[] hitboxes are still approximated by the
        // front-32px rule below). The heavy flag is
        // true in a finisher (60/77) — selects the
        // heavy stagger variants (Phase 6).
        if (crate::player::Player::is_ground_attack(self.player.action)
            || crate::player::Player::is_air_attack(self.player.action))
            && self.player.slot.frame >= 2
            && !self.player.attack_hit_done
        {
            self.player.attack_hit_done = true;
            let swing_damage = self.player.swing_damage;
            let heavy = matches!(self.player.action, 60 | 77);
            for st in &mut self.enemy_states {
                if st.dead || st.dying {
                    continue;
                }
                // dX gate (Phase 6, e.java:5863/5880):
                // while dX > 500 the melee hit does
                // not apply at all.
                if st.dx > 500 {
                    continue;
                }
                let in_front = if self.player.facing_right {
                    st.x - self.player.ga >= 0 && st.x - self.player.ga <= 32
                } else {
                    self.player.ga - st.x >= 0 && self.player.ga - st.x <= 32
                };
                if in_front && (st.y - self.player.gb).abs() < 16 {
                    // The kind-13 stagger variant
                    // (Phase 8, e.java:6463-6464) keys
                    // on the action the enemy is
                    // playing when hit, so capture it
                    // before apply_damage overwrites it.
                    let pre_action = st.action;
                    let pre_kind = st.enemy.kind;
                    let killed = if let Ok(anim_chunk) = res.chunk(st.enemy.anim_id) {
                        if let Ok(anim_set) = anim::load_anim_set(&anim_chunk) {
                            st.apply_damage(&anim_set, swing_damage, heavy)
                        } else {
                            false
                        }
                    } else {
                        false
                    };
                    // Stagger-13 orb (e.java:6464): a
                    // kind-13 enemy hit while playing
                    // action 12 staggers with 13 and
                    // drops a health orb (`b(..., 1,
                    // em)`) at its centre height.
                    if !killed && pre_kind == 13 && pre_action == 12 && st.action == 13 {
                        let (ox, oy, em) = (st.x, st.y - st.ef / 2, st.enemy.em);
                        self.pickups
                            .spawn(ox, oy, crate::pickups::HEALTH, em, &mut self.rng);
                    }
                    self.combo_hits += 1;
                    self.combo_timer_ms = 2000;
                    // Arena kill count `dI` (Phase 5,
                    // rest): incremented on every enemy
                    // death, in any mode.
                    if killed {
                        self.kills += 1;
                    }
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
        // Mode-2 hit (Phase 6, e.java:6417-6419): while
        // the player channels action 94, an enemy in
        // the swing's front-32px range takes no damage
        // — instead its dX grows by `(fK[gV] << 5) +
        // 40` per tick. The dX > 500 gate (above)
        // stops further hits once staggered. This
        // runs every tick of the channel (the
        // original's hit loop runs per tick while
        // the attack is active).
        if self.player.action == 94 {
            let add = (crate::player::Player::F_K
                [(self.player.magic_level_mode2 as usize).min(4)]
                << 5)
                + 40;
            for st in &mut self.enemy_states {
                if st.dead || st.dying || st.dx > 500 {
                    continue;
                }
                let in_front = if self.player.facing_right {
                    st.x - self.player.ga >= 0 && st.x - self.player.ga <= 32
                } else {
                    self.player.ga - st.x >= 0 && self.player.ga - st.x <= 32
                };
                if in_front && (st.y - self.player.gb).abs() < 16 {
                    st.dx += add;
                }
            }
        }
        let w = self.parsed.level.pixel_width() as i32;
        let h = self.parsed.level.pixel_height() as i32;
        // Box push/pull start (slice 22): a fresh
        // Left/Right press while grabbing (21) starts
        // the push (22, facing direction) or pull (23,
        // opposite) and runs u() at once; failure
        // returns to the grab pose. Only when the grab
        // was already held before this tick — a press
        // that walks the player into the box does not
        // also push it.
        let was_grabbing = self.player.action == 21;
        if was_grabbing && dir != 0 && dir != self.prev_dir {
            self.start_box_push(dir);
        }
        self.prev_dir = dir;
        self.player.tick(&self.player_anim, &self.flags, w, h, dir);
        // Grapple resolved (Phase 8): the grapple
        // action (88/91) has ended; success or fail
        // is decided by the mashes done.
        if self.grapple.is_some()
            && matches!(self.player.action, 88 | 91)
            && self.player.slot.repeat == 0
        {
            self.resolve_grapple();
        }
        // Death finished (Phase 7): the action-52
        // animation has ended; the caller (Game)
        // shows the death screen (state 108).
        if self.player.death_finished {
            self.death_finished = true;
        }
        // Box slots + slide (the original's per-frame
        // box update); the grab check runs after the
        // player tick, when the walk has stopped at
        // the box face.
        for b in &mut self.boxes {
            b.advance(&self.box_anim);
        }
        boxes::check_grab(&self.boxes, &self.flags, &mut self.player, &self.player_anim, dir);
        // Type-9 switch (slice 23): standing on a
        // flag-256 tile toggles the matching doors.
        // The player's tile is (tile_x, gb/16).
        doors::check_switch(
            &self.switches,
            &mut self.doors,
            &mut self.flags,
            self.player.tile_x(),
            self.player.gb / 16,
        );
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
        // Stagger-indicator pulse (Phase 6, the
        // original's `b(137, L)` per staggered
        // enemy): advance 40 ms per enemy with
        // dX > 0, so the shared slot animates.
        if let Some(stagger) = &mut self.stagger_bar {
            let BarState { anim, slot, .. } = stagger;
            let n = self.enemy_states.iter().filter(|st| !st.dead && !st.dying && st.dx > 0).count();
            for _ in 0..n {
                slot.advance(anim, 40);
            }
        }
        // Boss-bar pulse (Phase 2, `t(131)`): the
        // slot advances each tick while the `@`
        // latch holds.
        if self.script.boss_latched() {
            if let Some(bb) = &mut self.boss_bar {
                let BarState { anim, slot, .. } = bb;
                slot.advance(anim, 40);
            }
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
            // Flag-4 respawn (Phase 4, type-14):
            // crossing a flag-4 tile refreshes
            // off-screen enemies to new spawn points.
            self.check_respawn(tx, ty);
            // Checkpoint (Phase 7, type-25, the
            // `g()` flag-1024 handler): standing on
            // a flag-1024 tile sets the respawn point
            // (fY/fZ) to this tile and the facing
            // (hv) to the current facing.
            if self.flags.get(tx, ty) & 1024 != 0 {
                self.checkpoint_x = tx * 16;
                self.checkpoint_y = ty * 16;
                self.checkpoint_facing = self.player.facing_right;
            }
            // Type-28 rectangle door trigger (Phase 5):
            // standing on a flag-32 tile inside a
            // rect toggles its door (up to count times).
            doors::check_rect_trigger(
                &mut self.rect_triggers,
                &mut self.doors,
                &mut self.flags,
                tx,
                ty,
            );
            // Type-4 collectibles (Phase 5): touching
            // an uncollected item picks it up (orb
            // bundles spawn orbs; upgrades raise the
            // max and refill). The player plays the
            // pickup pose (action 31).
            if let Some(out) = crate::collectibles::check_collectible(
                &mut self.collectibles,
                self.player.ga,
                self.player.gb,
            ) {
                self.player.action = 31;
                self.player.slot = anim::AnimSlot::new(&self.player_anim, 31);
                match out {
                    crate::collectibles::CollectOutcome::Orbs { kind, value, x, y } => {
                        self.pickups.spawn(x, y, kind, value, &mut self.rng);
                    }
                    crate::collectibles::CollectOutcome::HealthUpgrade => {
                        // Phase 7, state 104: the level
                        // rises now, but the max grows
                        // gradually in the interstitial
                        // (and the `he` bit is set at
                        // collection, e.java:5442).
                        self.health_level += 1;
                        self.pending_he_health = Some(true);
                        self.start_upgrade(true);
                    }
                    crate::collectibles::CollectOutcome::MagicUpgrade => {
                        self.magic_level += 1;
                        self.pending_he_health = Some(false);
                        self.start_upgrade(false);
                    }
                }
            }
        }
        // End of tick (Phase 6): record the player's
        // health for next tick's hurt check (`hk`).
        self.prev_player_health = self.player.health;
    }

    /// Start the upgrade interstitial (Phase 7,
    /// state 104): `health` selects the Gorgon Eye
    /// (string 147, slot-136 action 0) or the
    /// Phoenix Feather (string 148, action 1). The
    /// target max is the new level's max; the
    /// current max grows toward it by 3 per tick
    /// inside `tick_upgrade`.
    fn start_upgrade(&mut self, health: bool) {
        let target_max = if health {
            self.health_level * 192 + 1024
        } else {
            self.magic_level * 192 + 1024
        };
        self.upgrade = Some(UpgradeState {
            health,
            timer: 192,
            target_max,
        });
        if let Some(bar) = &mut self.upgrade_bar {
            let BarState { anim, slot, .. } = bar;
            let action = if health { 0 } else { 1 };
            *slot = crate::anim::AnimSlot::new(anim, action);
        }
    }

    /// One interstitial tick (Phase 7, e.java:936-960,
    /// the state-104 update): the max grows by 3
    /// toward the target and the current health /
    /// magic grows by max/64 (clamped); the slot-136
    /// icon advances 40 ms; the timer drops by 3.
    /// At 0 the interstitial ends — health clamps
    /// to the max, magic refills to the max — and
    /// the world resumes.
    fn tick_upgrade(&mut self) {
        let Some(up) = &mut self.upgrade else {
            return;
        };
        if up.health {
            if self.player.max_health < up.target_max {
                self.player.max_health = (self.player.max_health + 3).min(up.target_max);
            }
            self.player.health =
                (self.player.health + self.player.max_health / 64).min(self.player.max_health);
        } else {
            if self.max_magic < up.target_max {
                self.max_magic = (self.max_magic + 3).min(up.target_max);
            }
            self.magic = (self.magic + self.max_magic / 64).min(self.max_magic);
        }
        if let Some(bar) = &mut self.upgrade_bar {
            let BarState { anim, slot, .. } = bar;
            slot.advance(anim, 40);
        }
        up.timer -= 3;
        if up.timer <= 0 {
            if up.health {
                if self.player.health > self.player.max_health {
                    self.player.health = self.player.max_health;
                }
            } else {
                self.magic = self.max_magic;
            }
            self.upgrade = None;
        }
    }

    /// True while the upgrade interstitial (state
    /// 104) is showing: the caller (Game) routes
    /// no gameplay input while it is up.
    pub fn upgrade_active(&self) -> bool {
        self.upgrade.is_some()
    }

    /// Mark already-collected upgrades (Phase 7,
    /// the `he` bitmask at type-4 load, e.java:3288):
    /// an upgrade item whose bit is set in `he`
    /// (`1 << bC` for health, `1 << bC << 10` for
    /// magic) starts collected, so it cannot be
    /// granted twice. Called by Game after load,
    /// with the save's `he`.
    pub fn mark_collected_upgrades(&mut self, he: i32) {
        let bit = 1 << self.level_index;
        for it in &mut self.collectibles {
            if it.kind != 3 {
                continue;
            }
            let health_bit = it.value > 0;
            let set = if health_bit {
                he & bit == bit
            } else {
                he & (bit << 10) == (bit << 10)
            };
            if set {
                it.collected = true;
            }
        }
    }

    /// Respawn at the checkpoint (Phase 7,
    /// e.java:1257-1279, the death screen's
    /// Attack/Select in a level): position and
    /// facing restore to the checkpoint, health
    /// and magic refill to the maxima, the block
    /// clears, the player idles, and the camera
    /// recentres on the checkpoint. Called by Game
    /// when leaving the death screen.
    pub fn respawn_at_checkpoint(&mut self) {
        self.player.ga = self.checkpoint_x;
        self.player.gb = self.checkpoint_y;
        self.player.facing_right = self.checkpoint_facing;
        self.player.blocking = false;
        self.player.action = 0;
        self.player.slot = crate::anim::AnimSlot::new(&self.player_anim, 0);
        self.player.death_finished = false;
        self.death_finished = false;
        self.player.health = self.player.max_health;
        self.magic = self.max_magic;
        self.prev_player_health = self.player.health;
        // Camera: `iQ = ga - 120 - ((hu ? 1 : -1) *
        // 240 >> 2)`, `iR = gb - 266` — the port's
        // update_camera target for the new spot.
        let target_x = self.player.ga - 120
            + if self.player.facing_right { 60 } else { -60 };
        let target_y = self.player.gb - 266;
        let w = self.parsed.level.pixel_width() as i32;
        let h = self.parsed.level.pixel_height() as i32;
        self.player.snap_camera(
            target_x.clamp(0, (w - 240).max(0)),
            target_y.clamp(0, (h - 320).max(0)),
        );
    }

    /// Flag-4 respawn (Phase 4, type-14): when the
    /// player stands on a flag-4 tile, find the
    /// trigger whose column covers the tile, then
    /// for each enemy with a table entry (eE != 0)
    /// update its spawn point (dV/dW) and, if the
    /// enemy is off-screen, respawn it there at full
    /// health and idle. Clears flag 4 for the
    /// trigger's tiles (single-use per trigger).
    fn check_respawn(&mut self, tx: i32, ty: i32) {
        if self.flags.get(tx, ty) & 4 == 0 {
            return;
        }
        let Some(ti) = self
            .respawn_triggers
            .iter()
            .position(|t| t.tile_x == tx && ty >= t.tile_y && ty <= t.tile_y + t.len as i32)
        else {
            return;
        };
        let trigger = &self.respawn_triggers[ti];
        let (cam_x, cam_y) = (self.player.cam_x, self.player.cam_y);
        // The original's flag-4 handler resets every
        // enemy's respawn counter (`ec = ed`, Phase 7,
        // e.java:4049) before the per-enemy refresh.
        for st in &mut self.enemy_states {
            st.enemy.ec = st.ed;
        }
        for st in &mut self.enemy_states {
            if st.dead || st.dying {
                continue;
            }
            let dy = st.enemy.dy as usize;
            let Some(&(nx, ny, _eg)) = trigger.entries.get(dy) else {
                continue;
            };
            if nx == 0 {
                continue;
            }
            // Update the spawn point (dV/dW).
            st.enemy.x = nx;
            st.enemy.y = ny;
            // Respawn only when off-screen (the
            // original's viewport-rect test, 368x448
            // expanded by 64).
            let in_view = nx >= cam_x - 64
                && nx <= cam_x + 240 + 64
                && ny >= cam_y - 64
                && ny <= cam_y + 320 + 64;
            if !in_view {
                st.x = nx;
                st.y = ny;
                st.health = st.max_health;
                st.frozen = false;
                if let Some(anim_set) = st.anim_set.clone() {
                    st.set_action(&anim_set, 0);
                }
            }
        }
        // Clear flag 4 for the trigger's tiles.
        for y in trigger.tile_y..=trigger.tile_y + trigger.len as i32 {
            self.flags.clear_flag(trigger.tile_x, y, 4);
        }
    }

    /// Enemy respawn (Phase 7, e.java:6334-6367): for
    /// each dead enemy (`dT < 0`), count the `er`
    /// timer down by 40 per tick; when it reaches 0
    /// and the gates hold (respawn count left, or
    /// the Arena, where the count is ignored; fewer
    /// than 10 damage orbs; the spawn point more
    /// than 32 px from the player in either axis)
    /// the enemy respawns at its spawn point at full
    /// health, playing its spawn-in action (27 kind
    /// 11, 14 kind 12, 18 kind 13; Phase 8) — if
    /// the tile above the spawn is clear (neither
    /// flag 1 nor flag 2); otherwise the timer
    /// resets to 1500 ms and retries. The respawn
    /// also sets flag 2 above the spawn.
    fn check_enemy_respawn(&mut self) {
        let (ga, gb) = (self.player.ga, self.player.gb);
        let orbs = self.pickups.orbs.len();
        for st in &mut self.enemy_states {
            if !st.dead {
                continue;
            }
            // Levels stop respawning when the count
            // is exhausted; the Arena ignores it.
            if !self.arena && st.enemy.ec <= 0 {
                continue;
            }
            st.er -= 40;
            if st.er > 0 {
                continue;
            }
            if orbs >= 10 {
                continue;
            }
            let (sx, sy) = (st.enemy.x, st.enemy.y);
            if (sx - ga).abs() <= 32 && (sy - gb).abs() <= 32 {
                continue;
            }
            let above = self.flags.get(sx / 16, sy / 16 - 1);
            if above & 1 == 0 && above & 2 == 0 {
                st.enemy.ec -= 1;
                st.er = st.enemy.et;
                st.x = sx;
                st.y = sy;
                st.health = st.max_health;
                // Spawn-in (Phase 8, e.java:6350-6367):
                // the enemy returns playing its
                // spawn-in action (27 kind 11, 14
                // kind 12, 18 kind 13), not idle.
                // The one-shot (27) runs to its end;
                // the loops (14/18) run 12 ticks.
                let action = st.spawn_in_action();
                st.frozen = false;
                if let Some(anim_set) = st.anim_set.clone() {
                    st.set_action(&anim_set, action);
                }
                st.spawn_in = if action == 27 { 40 } else { 12 };
                st.dead = false;
                st.dying = false;
                self.flags.set_flag(sx / 16, sy / 16 - 1, 2);
            } else {
                st.er = 1500;
            }
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
        // Boxes (slice 22): drawn after the statics,
        // before the enemies, at their px position
        // with their slot's current frame.
        for b in &self.boxes {
            b.draw(&g, fb, &self.box_anim, &self.box_sprites, self.player.cam_x, self.player.cam_y);
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
        // Stagger indicators (Phase 6): slot 137
        // above each enemy with dX > 0.
        if let Some(stagger) = &self.stagger_bar {
            enemies::draw_stagger_indicators(
                &g,
                fb,
                &stagger.anim,
                &stagger.sprites,
                stagger.slot.visual,
                &self.enemy_states,
                self.player.cam_x,
                self.player.cam_y,
            );
        }
        // Collectibles (Phase 8): slots 121+n, set
        // 5120, action `ft` (0 gold, 1 health, 2
        // magic, 3 upgrade), drawn at the item's
        // px position. Collected items are skipped
        // (the original sets `fp` and draws the
        // +4 action only while the pickup anim
        // plays — the port draws nothing once
        // collected).
        if let Some((anim, sprites)) = &self.collectible_sets {
            for it in &self.collectibles {
                if it.collected {
                    continue;
                }
                let action = it.kind as usize;
                if let Ok(frames) = anim::frames_for_timeline_frame(anim, action, 0, 0) {
                    for fr in frames {
                        if let Some(frame) = sprites.frame(fr.sprite_idx) {
                            midp::draw_sprite_frame(
                                &g,
                                fb,
                                &sprites.image,
                                frame,
                                it.x + fr.dx - self.player.cam_x,
                                it.y + fr.dy - self.player.cam_y,
                                fr.transform,
                            );
                        }
                    }
                }
            }
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
            max_magic: self.max_magic,
            gold: self.gold,
            combo_hits: self.combo_hits,
            weapon_mode: self.player.weapon_mode,
        };
        hud::draw_hud(&g, fb, res, font, &hud_state);
        // Arena kills HUD (Phase 5, rest, e.java
        // ~7750): "Kills: N" at screen (120, 306),
        // only in Arena mode.
        if self.arena {
            let text = format!("Kills: {}", self.kills);
            font.draw(&g, fb, &text, 120, 306, 0);
        }
        // Boss bar (Phase 2, `d(Graphics)` state 100):
        // drawn at screen (12, F - 25) = (12, 295)
        // when the `@` latch holds, the player's
        // facing `hu` equals `aS`, and the player is
        // inside the viewport (`e.a(ga, gb, iQ, iR,
        // E, F)`). The `!fR` guard (Phase 8): while
        // a grapple is active the boss bar is hidden
        // and slot 131 shows the grapple prompt
        // instead. The bar is a HUD element: screen
        // coordinates, not camera-translated.
        if self.grapple.is_none()
            && self.script.boss_latched()
            && self.script.boss_facing_right() == self.player.facing_right
            && self.player.ga >= self.player.cam_x
            && self.player.ga < self.player.cam_x + 240
            && self.player.gb >= self.player.cam_y
            && self.player.gb < self.player.cam_y + 320
        {
            if let Some(bb) = &self.boss_bar {
                if let Ok(frames) = anim::frames_for_timeline_frame(
                    &bb.anim,
                    self.script.boss_action() as usize,
                    bb.slot.visual,
                    0,
                ) {
                    for fr in frames {
                        if let Some(frame) = bb.sprites.frame(fr.sprite_idx) {
                            midp::draw_sprite_frame(
                                &g,
                                fb,
                                &bb.sprites.image,
                                frame,
                                12 + fr.dx,
                                295 + fr.dy,
                                fr.transform,
                            );
                        }
                    }
                }
            }
        }
        // Grapple prompt bar (Phase 8, e.java:2046):
        // while the grapple is active and mashes
        // remain (`fS >= 0 && fS < fQ`), slot 131
        // shows the prompt for the required button
        // above the grappled enemy's head
        // (`dT[ge] - iQ, dU[ge] - ef - ef/4 - iR`).
        if let Some(gr) = &self.grapple {
            if gr.f_s >= 0 && gr.f_s < gr.f_q {
                if let Some(st) = self.enemy_states.get(gr.enemy) {
                    if let Some(bb) = &self.boss_bar {
                        let bx = st.x - self.player.cam_x;
                        let by = st.y - st.ef - (st.ef >> 2) - self.player.cam_y;
                        if let Ok(frames) = anim::frames_for_timeline_frame(
                            &bb.anim,
                            bb.slot.action,
                            bb.slot.visual,
                            0,
                        ) {
                            for fr in frames {
                                if let Some(frame) = bb.sprites.frame(fr.sprite_idx) {
                                    midp::draw_sprite_frame(
                                        &g,
                                        fb,
                                        &bb.sprites.image,
                                        frame,
                                        bx + fr.dx,
                                        by + fr.dy,
                                        fr.transform,
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
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
        // Upgrade interstitial (Phase 7, state 104):
        // framed box + slot-136 icon + two centred
        // lines, on top of the frozen world.
        if self.upgrade.is_some() {
            self.draw_upgrade(fb, font);
        }
    }

    /// Draw the upgrade interstitial (Phase 7,
    /// e.java:2050-2075): the framed box
    /// (fill 855564, borders 4269080/5911589),
    /// the `cq` corner ornaments (sprite set 1030,
    /// frames 0/2/1/3), the slot-136 icon at
    /// (120, 108), and two centred lines — the
    /// found title (147/148) at y=186 and the
    /// outcome (151, or 158/189 while fw < 96)
    /// at y=238. Screen coordinates, not
    /// camera-translated.
    fn draw_upgrade(&self, fb: &mut Framebuffer, font: &Font) {
        let Some(up) = &self.upgrade else {
            return;
        };
        let g = Graphics::default();
        let mut gg = g;
        // Fill (13, 14, 12) = 855564.
        gg.set_color_rgb(13, 14, 12);
        gg.fill_rect(fb, 15, 80, 210, 186);
        // Border 4269080 = (65, 36, 24).
        gg.set_color_rgb(65, 36, 24);
        draw_rect_outline(&gg, fb, 14, 79, 211, 187);
        // Border 5911589 = (90, 52, 37).
        gg.set_color_rgb(90, 52, 37);
        draw_rect_outline(&gg, fb, 13, 78, 213, 189);
        // Corner ornaments `cq` (sprite set 1030).
        if let Some(corners) = &self.upgrade_corners {
            for (frame_idx, (x, y)) in [
                (0, (12, 78)),
                (2, (228, 78)),
                (1, (12, 268)),
                (3, (228, 268)),
            ] {
                if let Some(frame) = corners.frame(frame_idx) {
                    midp::draw_sprite_frame(&g, fb, &corners.image, frame, x, y, 0);
                }
            }
        }
        // Slot-136 icon (anim set 1026, sprite set
        // 1038) at (120, 108).
        if let Some(bar) = &self.upgrade_bar {
            let action = if up.health { 0 } else { 1 };
            if let Ok(frames) = anim::frames_for_timeline_frame(&bar.anim, action, bar.slot.visual, 0) {
                for fr in frames {
                    if let Some(frame) = bar.sprites.frame(fr.sprite_idx) {
                        midp::draw_sprite_frame(
                            &g,
                            fb,
                            &bar.sprites.image,
                            frame,
                            120 + fr.dx,
                            108 + fr.dy,
                            fr.transform,
                        );
                    }
                }
            }
        }
        // Two centred lines (anchor 1 = HCENTER).
        let title = self.strings.get(if up.health { 147 } else { 148 });
        if let Some(title) = title {
            font.draw(&g, fb, title, 120, 186, 1);
        }
        let sub_idx = if up.timer < 96 {
            if up.health { 158 } else { 189 }
        } else {
            151
        };
        if let Some(sub) = self.strings.get(sub_idx) {
            font.draw(&g, fb, sub, 120, 238, 1);
        }
    }
}

/// Draw a 1 px rectangle outline with `fill_rect`
/// (the port's Graphics has no draw_rect): the
/// MIDP drawRect spans x..x+w-1, y..y+h-1.
fn draw_rect_outline(g: &Graphics, fb: &mut Framebuffer, x: i32, y: i32, w: i32, h: i32) {
    g.fill_rect(fb, x, y, w, 1);
    g.fill_rect(fb, x, y + h - 1, w, 1);
    g.fill_rect(fb, x, y, 1, h);
    g.fill_rect(fb, x + w - 1, y, 1, h);
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
    fn drain_script(gp: &mut super::Gameplay, res: &mut crate::resources::Resources) {
        // Drain dialogs. A `!` cutscene (Phase 4)
        // starts on advance; tick until it ends
        // (it advances the script itself via C()).
        let mut guard = 0;
        while gp.script.active() && guard < 1000 {
            guard += 1;
            gp.script_advance();
            let mut cguard = 0;
            while gp.cutscene_active && cguard < 1000 {
                cguard += 1;
                gp.tick(res, 0, false);
            }
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
        drain_script(gp, res);
    }

    /// Camera-shake regression (v1.4.1): standing
    /// still at (658,560) in 9217, the vertical
    /// camera zones at y=416 used to make the whole
    /// screen shake up and down — cam_y swung
    /// 273..304 on an 8-tick cycle. Root cause: the
    /// base camera measured its delta from the
    /// FINAL camera (base + zone offset), so it
    /// compensated for the offset and sat `zone_j`
    /// above the target, putting the zone points on
    /// the very edge of the base viewport; the zone
    /// flickered in and out as the base wobbled
    /// across the edge, pumping the offset up and
    /// letting it decay. The original measures from
    /// the base camera (`o()`: n6 - iQ + aI), so the
    /// base converges on the target and the zone
    /// stays steadily in view. With that fixed,
    /// cam_y settles at 304 and zone_j at the snap
    /// value 122 (416 - 294) and neither moves.
    #[test]
    fn camera_does_not_shake_under_vertical_zones() {
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 9217, false).unwrap();
        gp.dismiss_intro();
        drain_script(&mut gp, &mut res);
        gp.player.ga = 658;
        gp.player.gb = 560;
        gp.player.facing_right = true;
        // Settle: the base camera slews to the new
        // target and the zone offset grows to its
        // snap value (a few ticks each).
        for _ in 0..30 {
            t(&mut gp, &mut res, 0, false);
        }
        let mut min_cy = i32::MAX;
        let mut max_cy = i32::MIN;
        let mut min_zj = i32::MAX;
        let mut max_zj = i32::MIN;
        for _ in 0..40 {
            t(&mut gp, &mut res, 0, false);
            min_cy = min_cy.min(gp.player.cam_y);
            max_cy = max_cy.max(gp.player.cam_y);
            min_zj = min_zj.min(gp.player.cam_zone_j);
            max_zj = max_zj.max(gp.player.cam_zone_j);
        }
        assert_eq!(max_cy - min_cy, 0, "cam_y must not move once settled (was {min_cy}..{max_cy})");
        assert_eq!(max_zj - min_zj, 0, "zone_j must not move once settled (was {min_zj}..{max_zj})");
        assert_eq!(gp.player.cam_zone_j, 122, "zone offset is the snap value 416 - 294");
    }

    /// Phase 2: the boss bar latches on an `@`
    /// trigger, animates while latched, and clears
    /// when the player leaves the trigger tiles.
    /// Teleports to the `@21` wall-face trigger at
    /// tile (135, 28) in level 9217.
    #[test]
    fn boss_bar_latches_and_animates() {
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 9217, false).unwrap();
        gp.dismiss_intro();
        assert!(gp.boss_bar.is_some(), "boss-bar sets load");
        assert!(!gp.script.boss_latched());
        // Teleport onto the `@21` wall-face tile,
        // facing right (aS=true for `@21`).
        gp.player.ga = 135 * 16;
        gp.player.gb = 28 * 16;
        gp.player.facing_right = true;
        t(&mut gp, &mut res, 0, false);
        assert!(gp.script.boss_latched());
        assert_eq!(gp.script.boss_action(), 2, "aT = 1st digit of `@21`");
        assert!(gp.script.boss_facing_right(), "aS = 2nd digit '1'");
        // The slot is (re)set to action 2 on latch.
        let bb = gp.boss_bar.as_ref().unwrap();
        assert_eq!(bb.slot.action, 2);
        // The bar animates (t(131)) while latched:
        // action 2 is a 2-frame loop (t0 200 ms,
        // t1 100 ms), so after 6 ticks (240 ms) the
        // visual has moved from frame 0 to frame 1.
        let v0 = bb.slot.visual;
        assert_eq!(v0, 0, "action 2 starts on timeline frame 0");
        for _ in 0..6 {
            t(&mut gp, &mut res, 0, false);
        }
        assert!(gp.script.boss_latched(), "still on the trigger tile");
        let v1 = gp.boss_bar.as_ref().unwrap().slot.visual;
        assert_eq!(v1, 1, "boss-bar slot advances to frame 1 after 240 ms");
        // Draw the latched bar: smoke test (no
        // panic) and the bar changes pixels in its
        // screen band (y 270-300, x 0-80). The
        // camera lags a teleport, so snap it to
        // the player first — otherwise the
        // viewport guard (`e.a(ga, gb, iQ, iR, E,
        // F)`) correctly hides the bar.
        gp.player.cam_x = gp.player.ga - 120;
        gp.player.cam_y = gp.player.gb - 160;
        let mut fb = crate::midp::Framebuffer::new(240, 320);
        let font_set = res.sprite_set(1031, None).expect("font 1031");
        let font = crate::font::Font::from_sprite_set(font_set);
        gp.draw(&mut fb, &mut res, &font);
        // Same scene, same camera, player turned
        // away (aS=true needs facing right): the
        // bar hides. The band is fully opaque
        // ground, so compare pixel values, not
        // non-zero counts.
        gp.player.facing_right = false;
        let mut fb2 = crate::midp::Framebuffer::new(240, 320);
        gp.draw(&mut fb2, &mut res, &font);
        let changed = (270..300)
            .flat_map(|y| (0..80).map(move |x| (x, y)))
            .filter(|&(x, y)| fb.pixels[y * 240 + x] != fb2.pixels[y * 240 + x])
            .count();
        assert!(changed > 0, "latched bar changes pixels in its screen band");
        // Step off the trigger tiles: latch clears.
        gp.player.ga = 100 * 16;
        gp.player.gb = 35 * 16;
        gp.player.facing_right = true;
        t(&mut gp, &mut res, 0, false);
        assert!(!gp.script.boss_latched());
    }

    /// HD render (v1.5.0): the same scene rendered from HD sheets
    /// must be 960x1280, drawn (not blank), and laid out exactly like
    /// the SD render — downsampling the HD frame by 4 must reproduce
    /// the SD frame's drawn silhouette. This catches a wrong render
    /// scale, a missing HD sheet, or a frame drawn at the wrong place.
    #[test]
    fn hd_render_matches_sd_layout() {
        // SD reference.
        let mut res_sd = crate::resources::Resources::new("../extracted");
        let mut gp_sd = super::Gameplay::load(&mut res_sd, 9217, false).unwrap();
        gp_sd.dismiss_intro();
        drain_script(&mut gp_sd, &mut res_sd);
        let font_sd = crate::font::Font::from_sprite_set(
            res_sd.sprite_set(1031, None).expect("font 1031"),
        );
        let mut fb_sd = crate::midp::Framebuffer::game();
        gp_sd.draw(&mut fb_sd, &mut res_sd, &font_sd);
        // HD render of the same state.
        let mut res_hd = crate::resources::Resources::new("../extracted");
        assert!(res_hd.enable_hd(), "hd/ should exist");
        let mut gp_hd = super::Gameplay::load(&mut res_hd, 9217, false).unwrap();
        gp_hd.dismiss_intro();
        drain_script(&mut gp_hd, &mut res_hd);
        let font_hd = crate::font::Font::from_sprite_set(
            res_hd.sprite_set(1031, None).expect("font 1031"),
        );
        let mut fb_hd = crate::midp::Framebuffer::game_scaled(res_hd.render_scale());
        assert_eq!((fb_hd.width, fb_hd.height), (960, 1280));
        gp_hd.draw(&mut fb_hd, &mut res_hd, &font_hd);
        let drawn_hd = fb_hd.pixels.iter().filter(|&&p| p != 0xFF00_0000).count();
        assert!(drawn_hd > 100_000, "HD frame should be mostly drawn, got {drawn_hd}");
        // Silhouette agreement after downsampling by 4.
        let (mut agree, mut total) = (0usize, 0usize);
        for y in 0..320 {
            for x in 0..240 {
                let sd_drawn = fb_sd.pixels[y * 240 + x] != 0xFF00_0000;
                let hd_drawn = fb_hd.pixels[(y * 4) * 960 + (x * 4)] != 0xFF00_0000;
                total += 1;
                if sd_drawn == hd_drawn {
                    agree += 1;
                }
            }
        }
        let pct = agree * 100 / total;
        assert!(pct >= 90, "HD/SD silhouette agreement {pct}%");
    }

    /// Phase 8: collectibles are drawn (set 5120,
    /// slots 121+n, action `ft`) at their position
    /// while uncollected, and hidden once collected.
    #[test]
    fn collectibles_draw_at_their_position() {
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 19456, false).unwrap();
        gp.dismiss_intro();
        assert!(gp.collectible_sets.is_some(), "set 5120 loads");
        // Health upgrade at tile (91,12) -> px
        // (1456,192). Teleport there and snap the
        // camera so the item is on screen.
        let it = gp
            .collectibles
            .iter()
            .find(|c| c.kind == 3 && c.value > 0)
            .expect("19456 has a health upgrade");
        let (ix, iy) = (it.x, it.y);
        gp.player.ga = ix;
        gp.player.gb = iy;
        gp.player.cam_x = ix - 120;
        gp.player.cam_y = iy - 160;
        let font_set = res.sprite_set(1031, None).expect("font 1031");
        let font = crate::font::Font::from_sprite_set(font_set);
        let mut fb = crate::midp::Framebuffer::new(240, 320);
        gp.draw(&mut fb, &mut res, &font);
        // Same scene with the item collected: the
        // icon disappears. Compare the whole frame:
        // the icon's anim offset (dx 32, dy -24)
        // puts it off-centre from the item.
        let mut gp2 = super::Gameplay::load(&mut res, 19456, false).unwrap();
        gp2.dismiss_intro();
        for c in gp2.collectibles.iter_mut() {
            if c.x == ix && c.y == iy {
                c.collected = true;
            }
        }
        gp2.player.ga = ix;
        gp2.player.gb = iy;
        gp2.player.cam_x = ix - 120;
        gp2.player.cam_y = iy - 160;
        let mut fb2 = crate::midp::Framebuffer::new(240, 320);
        gp2.draw(&mut fb2, &mut res, &font);
        let changed = fb
            .pixels
            .iter()
            .zip(fb2.pixels.iter())
            .filter(|(a, b)| a != b)
            .count();
        assert!(changed > 0, "uncollected item changes pixels in the frame");
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

    /// Slice 21, Gameplay routing: Up double-jumps
    /// onto the climb column (tile 156) and attaches;
    /// a further Up climbs (35); Down held climbs
    /// down (34). Teleports to the temple floor.
    #[test]
    fn gameplay_climb_attach_up_down() {
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 9217, false).unwrap();
        gp.dismiss_intro();
        gp.player.ga = 2440;
        gp.player.gb = 528;
        gp.player.facing_right = true;
        assert!(gp.jump(), "first jump must start");
        for _ in 0..12 {
            t(&mut gp, &mut res, 1, false);
        }
        assert_eq!(gp.player.action, 24);
        assert!(gp.jump(), "double jump must start");
        let mut attached = false;
        for _ in 0..60 {
            t(&mut gp, &mut res, 1, false);
            if gp.player.action == 33 {
                attached = true;
                break;
            }
        }
        assert!(attached, "never attached: action {}", gp.player.action);
        assert_eq!(gp.player.ga, 156 * 16 - 10);
        // Up while holding climbs one cycle (-19 px).
        let start_gb = gp.player.gb;
        assert!(gp.jump(), "climb-up refused");
        assert_eq!(gp.player.action, 35);
        let mut held = false;
        for _ in 0..30 {
            t(&mut gp, &mut res, 0, false);
            if gp.player.action == 33 {
                held = true;
                break;
            }
        }
        assert!(held, "climb-up never held: action {}", gp.player.action);
        assert_eq!(gp.player.gb, start_gb - 19);
        // Down held climbs down one cycle (+20 px).
        gp.block_held = true;
        let start_gb = gp.player.gb;
        let mut down = false;
        for _ in 0..30 {
            t(&mut gp, &mut res, 0, false);
            if gp.player.action == 34 {
                down = true;
                break;
            }
        }
        assert!(down, "Down held never started climb-down: action {}", gp.player.action);
        let mut held = false;
        for _ in 0..30 {
            t(&mut gp, &mut res, 0, false);
            if gp.player.action == 33 {
                held = true;
                break;
            }
        }
        assert!(held, "climb-down never held: action {}", gp.player.action);
        assert_eq!(gp.player.gb, start_gb + 20);
        gp.block_held = false;
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
        // One Select reaches the '!' cutscene step
        // (Phase 4); tick until it ends (C()), then
        // the '*' fires and the level completes.
        gp.script_advance();
        assert!(gp.cutscene_active);
        let mut guard = 0;
        while gp.cutscene_active && guard < 1000 {
            guard += 1;
            gp.tick(&mut res, 0, false);
        }
        assert!(!gp.cutscene_active, "cutscene should end");
        assert_eq!(gp.level_complete, Some(1));
        // Neighbour tiles of the same script are spent.
        gp.player.ga = 214 * 16;
        gp.player.gb = 31 * 16;
        gp.tick(&mut res, 0, false);
        assert!(!gp.script.active());
    }

    #[test]
    fn cutscene_plays_ay_action_then_ends() {
        // Phase 4: a `!` step plays the type-6 aY
        // table (state 105). Level 9217's aY[0] is
        // action 96 (12 frames, repeat 3); the
        // cutscene ends when it finishes (next aY
        // is 0, no aZ target), and the script
        // advances past the `!` step.
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 9217, false).unwrap();
        gp.dismiss_intro();
        // The spawn tutorial opens with a `!` step
        // (string 209), so the first tick starts it.
        gp.tick(&mut res, 0, false);
        assert!(gp.script.active());
        assert!(gp.cutscene_active, "cutscene should start on the `!` step");
        assert_eq!(gp.player.action, 96, "aY[0] = action 96");
        // Tick until the cutscene ends (action 96
        // is ~3 s at 40 ms/tick, so allow 200).
        let mut guard = 0;
        while gp.cutscene_active && guard < 200 {
            guard += 1;
            gp.tick(&mut res, 0, false);
        }
        assert!(!gp.cutscene_active, "cutscene should end after action 96");
        // The script advanced past the `!` step to
        // the next dialog (string 210).
        assert!(gp.script.active());
        assert_eq!(
            gp.script.dialog_text(),
            Some("God of War, we call on you!  Grant us strength!  ")
        );
    }

    #[test]
    fn flag4_respawn_refreshes_offscreen_enemy() {
        // Phase 4, type-14: crossing a flag-4 tile
        // updates off-screen enemies' spawn points
        // and respawns them there at full health.
        // Level 9217's trigger 1 is at tile
        // (139, 18), len 8; its dY=0 entry is
        // spawn (2400, 416).
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 9217, false).unwrap();
        gp.dismiss_intro();
        // Pick the enemy with dY=0 (spawn 336,560)
        // and wound it.
        let st = gp.enemy_states.iter_mut().find(|s| s.enemy.dy == 0).unwrap();
        st.health = 1;
        let old_spawn = (st.enemy.x, st.enemy.y);
        assert_eq!(old_spawn, (336, 560));
        // Teleport the player onto the flag-4
        // trigger tile (139, 18) and tick. The
        // camera is set far away so the enemy is
        // off-screen and actually respawns.
        gp.player.ga = 139 * 16;
        gp.player.gb = 18 * 16;
        gp.player.cam_x = 139 * 16 - 120;
        gp.player.cam_y = 18 * 16 - 266;
        gp.tick(&mut res, 0, false);
        let st = gp.enemy_states.iter().find(|s| s.enemy.dy == 0).unwrap();
        assert_eq!((st.enemy.x, st.enemy.y), (2400, 416), "spawn point updated from the type-14 table");
        assert_ne!((st.enemy.x, st.enemy.y), old_spawn);
        // Off-screen, so it respawned there at full
        // health and idle.
        assert_eq!((st.x, st.y), (2400, 416));
        assert_eq!(st.health, st.max_health);
        assert_eq!(st.action, 0);
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
        // Away from the spawn tutorial (its first
        // step is a `!` cutscene, Phase 4).
        gp.player.ga = 416;
        // Phase 6: the nearby enemy would hit the
        // player mid-combo and the hurt reaction
        // would interrupt the swing (as in the
        // original) — this test is about the chain,
        // so the enemy deals no damage here.
        for st in &mut gp.enemy_states {
            st.el = 0;
        }
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
        // Away from the spawn tutorial (its first
        // step is a `!` cutscene, Phase 4).
        gp.player.ga = 416;
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
        // Phase 6: the enemy deals no damage here
        // (its hit would play the hurt reaction and
        // interrupt the swing, as in the original) —
        // this test is about the damage numbers.
        for st in &mut gp.enemy_states {
            st.el = 0;
        }
        // Follow the enemy's stagger slide (Phase 6):
        // each hit slides it back ~26 px, so the
        // player steps up to stay in the 32 px
        // swing range.
        let follow = |gp: &mut super::Gameplay| {
            let x = gp
                .enemy_states
                .iter()
                .find(|st| st.enemy.x == 448)
                .map(|st| st.x)
                .unwrap();
            gp.player.ga = x - 16;
            gp.player.facing_right = true;
        };
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
            follow(&mut gp);
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
        // Phase 6: the nearby enemy would hit the
        // player mid-combo and the hurt reaction
        // would interrupt the swing (as in the
        // original) — this test is about the chain,
        // so the enemy deals no damage here.
        for st in &mut gp.enemy_states {
            st.el = 0;
        }
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
        // Fire in a magic mode starts the cast
        // (Phase 3), not a blades swing.
        gp.magic = 500;
        gp.player.weapon_mode = 2;
        assert!(gp.player.attack(&gp.player_anim));
        assert_eq!(gp.player.action, 94);
        gp.player.end_magic_cast(&gp.player_anim);
        gp.player.weapon_mode = 3;
        assert!(gp.player.attack(&gp.player_anim));
        assert_eq!(gp.player.action, 95);
        gp.player.end_magic_cast(&gp.player_anim);
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
    fn mode3_cast_spawns_damage_orbs_and_costs_256() {
        // Mode 3 (hd, Phase 3): fire with magic >
        // 256 spawns type-3 orbs totalling fL[gW]
        // (128 at gW 0), costs a flat 256, plays
        // action 95, and resets to blades (A()).
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 9217, false).unwrap();
        gp.dismiss_intro();
        gp.magic = 500;
        gp.player.weapon_mode = 3;
        // Stand by the 448 enemy so it is in the
        // viewport and gets targeted; tick first
        // so the camera catches up to the teleport.
        gp.player.ga = 416;
        for _ in 0..20 {
            t(&mut gp, &mut res, 0, false);
        }
        t(&mut gp, &mut res, 0, true);
        assert_eq!(gp.player.action, 95, "mode 3 should play action 95");
        assert_eq!(gp.player.weapon_mode, 0, "cast resets to blades");
        assert_eq!(gp.magic, 244, "flat 256 cost (500 - 256)");
        assert_eq!(gp.pickups.damage_orb_count(), 9, "128 -> 9 orbs");
        // The orbs home on the 448 enemy and deal
        // their total 128 damage (13 hp -> dead).
        let health = |gp: &super::Gameplay| {
            gp.enemy_states
                .iter()
                .find(|st| st.enemy.x == 448)
                .map(|st| st.health)
                .unwrap()
        };
        let start = health(&gp);
        for _ in 0..400 {
            t(&mut gp, &mut res, 0, false);
            if gp.pickups.damage_orb_count() == 0 {
                break;
            }
        }
        assert_eq!(gp.pickups.damage_orb_count(), 0, "orbs never landed");
        assert!(health(&gp) < start, "damage orbs dealt no damage");
    }

    #[test]
    fn mode3_gate_blocks_low_magic_and_full_orbs() {
        // Gate: magic <= 256, or 10+ active damage
        // orbs, falls back to a blades swing and
        // resets to blades (the original's s(54)).
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 9217, false).unwrap();
        gp.dismiss_intro();
        // Away from the spawn tutorial (its first
        // step is a `!` cutscene, Phase 4).
        gp.player.ga = 416;
        gp.magic = 256;
        gp.player.weapon_mode = 3;
        t(&mut gp, &mut res, 0, true);
        assert_eq!(gp.player.weapon_mode, 0, "failed gate resets to blades");
        assert_eq!(gp.player.action, 54, "failed gate falls to a swing");
        assert_eq!(gp.pickups.damage_orb_count(), 0);
        assert_eq!(gp.magic, 256, "no cost on a failed gate");
    }

    #[test]
    fn mode2_cast_channels_and_drains() {
        // Mode 2 (hc, Phase 3): fire with magic >
        // (gV+1)*4 starts action 94; each tick
        // with attack pressed drains 12; a tick
        // without attack ends the cast (idle +
        // blades). The hit itself applies no
        // damage (dX stagger is Phase 6).
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 9217, false).unwrap();
        gp.dismiss_intro();
        gp.magic = 100;
        gp.player.weapon_mode = 2;
        // Away from spawn (a spawn script `^` step
        // would otherwise reset the weapon mode).
        gp.player.ga = 416;
        t(&mut gp, &mut res, 0, true);
        assert_eq!(gp.player.action, 94, "mode 2 should play action 94");
        assert_eq!(gp.magic, 88, "12 drained on the fire tick");
        assert_eq!(gp.player.weapon_mode, 2, "channel keeps the mode");
        t(&mut gp, &mut res, 0, true);
        assert_eq!(gp.magic, 76, "12 drained per channel tick");
        assert_eq!(gp.player.action, 94, "still channeling");
        // Release attack: the cast ends.
        t(&mut gp, &mut res, 0, false);
        assert_eq!(gp.player.action, 0, "cast ends on release");
        assert_eq!(gp.player.weapon_mode, 0, "cast end resets to blades");
        assert_eq!(gp.magic, 76, "no drain after the cast ends");
    }

    #[test]
    fn mode2_cast_ends_when_magic_runs_out() {
        // Channel until magic hits 0: the cast
        // ends (idle + blades) instead of going
        // negative.
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 9217, false).unwrap();
        gp.dismiss_intro();
        gp.magic = 30;
        gp.player.weapon_mode = 2;
        // Away from spawn (a spawn script `^` step
        // would otherwise reset the weapon mode).
        gp.player.ga = 416;
        t(&mut gp, &mut res, 0, true);
        assert_eq!(gp.player.action, 94);
        assert_eq!(gp.magic, 18);
        t(&mut gp, &mut res, 0, true);
        assert_eq!(gp.magic, 6);
        assert_eq!(gp.player.action, 94);
        t(&mut gp, &mut res, 0, true);
        assert_eq!(gp.magic, 0, "drain clamps at 0");
        // The cast ends on the tick after magic
        // reaches 0 (the drain tick itself still
        // channels).
        t(&mut gp, &mut res, 0, true);
        assert_eq!(gp.player.action, 0, "cast ends at 0 magic");
        assert_eq!(gp.player.weapon_mode, 0);
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
        // Phase 6: Down enters the block with the
        // enter pose 27, which becomes the hold
        // pose 3 when it ends while Down is held.
        t(&mut gp, &mut res, 0, false);
        assert!(gp.player.blocking);
        assert!(matches!(gp.player.action, 27 | 3));
        for _ in 0..10 {
            t(&mut gp, &mut res, 0, false);
            if gp.player.action == 3 {
                break;
            }
        }
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
        // Phase 6: releasing Down plays the exit
        // pose 28 before returning to idle.
        gp.block_held = false;
        for _ in 0..10 {
            t(&mut gp, &mut res, 0, false);
            if gp.player.action == 0 {
                break;
            }
        }
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
        // Away from the spawn tutorial (its first
        // step is a `!` cutscene, Phase 4).
        gp.player.ga = 416;
        gp.block_held = true;
        // Phase 6: Down enters with pose 27, then
        // the hold pose 3.
        t(&mut gp, &mut res, 0, false);
        assert!(gp.player.blocking);
        assert!(matches!(gp.player.action, 27 | 3));
        for _ in 0..10 {
            t(&mut gp, &mut res, 0, false);
            if gp.player.action == 3 {
                break;
            }
        }
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
        // Phase 6: releasing Down plays the exit
        // pose 28 before returning to idle.
        gp.block_held = false;
        for _ in 0..10 {
            t(&mut gp, &mut res, 0, false);
            if gp.player.action == 0 {
                break;
            }
        }
        assert_eq!(gp.player.action, 0);
        assert!(!gp.player.blocking);
    }

    #[test]
    fn enemy_hit_plays_hurt_reaction() {
        // Phase 6 (e.java:5378-5382): an unblocked
        // enemy hit plays the hurt reaction — the
        // player at 416 faces the 448 enemy (to the
        // right), so the small hurt (5) plays.
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 9217, false).unwrap();
        gp.dismiss_intro();
        gp.player.ga = 416;
        gp.player.facing_right = true;
        let hp0 = gp.player.health;
        let mut hurt = false;
        for _ in 0..200 {
            t(&mut gp, &mut res, 0, false);
            if gp.player.action == 5 || gp.player.action == 7 || gp.player.action == 100 {
                hurt = true;
                break;
            }
        }
        assert!(hurt, "enemy hit never played a hurt reaction");
        assert_eq!(gp.player.action, 5, "facing the attacker should play 5");
        assert!(gp.player.health < hp0, "the hit should deal damage");
    }

    #[test]
    fn mode2_hit_builds_dx_and_gate_blocks_swings() {
        // Phase 6 (e.java:6417-6419, 5863/5880):
        // a mode-2 channel hit adds dX instead of
        // damage, and while dX > 500 melee swings
        // do not apply.
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 9217, false).unwrap();
        gp.dismiss_intro();
        gp.player.ga = 416;
        gp.player.facing_right = true;
        gp.player.weapon_mode = 2;
        gp.magic = 500;
        // Stand next to the 448 enemy and channel.
        let idx = gp
            .enemy_states
            .iter()
            .position(|s| s.enemy.x == 448)
            .unwrap();
        gp.player.magic_level_mode2 = 0;
        t(&mut gp, &mut res, 0, true);
        assert_eq!(gp.player.action, 94, "mode 2 should start the channel");
        // Channel a few ticks: dX grows by
        // (fK[0] << 5) + 40 = 104 per tick.
        for _ in 0..3 {
            t(&mut gp, &mut res, 0, true);
        }
        assert!(
            gp.enemy_states[idx].dx > 0,
            "mode-2 hits should build dX, got {}",
            gp.enemy_states[idx].dx
        );
        // Gate: with dX > 500, a blades swing
        // deals no damage.
        gp.enemy_states[idx].dx = 600;
        let hp = gp.enemy_states[idx].health;
        gp.player.weapon_mode = 0;
        gp.player.action = 54;
        gp.player.slot = super::anim::AnimSlot::new(&gp.player_anim, 54);
        gp.player.attack_hit_done = false;
        gp.player.swing_damage = 2;
        gp.player.slot.frame = 2;
        t(&mut gp, &mut res, 0, false);
        assert_eq!(
            gp.enemy_states[idx].health, hp,
            "swing must not damage a dX > 500 enemy"
        );
    }

    #[test]
    fn fire_while_grabbing_releases_to_idle() {
        // Phase 6 (e.java:5095-5097): attack while
        // grabbing a box releases to idle, no swing.
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 9217, false).unwrap();
        gp.dismiss_intro();
        // Walk to the crate and grab it (slice 22).
        gp.player.ga = 2048 - 32;
        gp.player.gb = 560;
        gp.player.facing_right = true;
        let mut grabbed = false;
        for _ in 0..40 {
            t(&mut gp, &mut res, 1, false);
            if gp.player.action == 21 {
                grabbed = true;
                break;
            }
        }
        assert!(grabbed, "never grabbed the crate");
        assert!(gp.player.grabbed_box.is_some());
        t(&mut gp, &mut res, 0, true);
        assert_eq!(gp.player.action, 0, "attack while grabbing should release to idle");
        assert_eq!(gp.player.grabbed_box, None);
    }

    #[test]
    fn up_while_blocking_unblocks_without_jumping() {
        // Phase 6 (e.java:4799-4810): Up while
        // blocking exits the stance to idle and
        // does NOT jump.
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 9217, false).unwrap();
        gp.dismiss_intro();
        gp.player.ga = 416;
        gp.block_held = true;
        for _ in 0..10 {
            t(&mut gp, &mut res, 0, false);
            if gp.player.action == 3 {
                break;
            }
        }
        assert_eq!(gp.player.action, 3);
        assert!(gp.player.blocking);
        let gb = gp.player.gb;
        assert!(!gp.jump(), "Up while blocking must not jump");
        assert_eq!(gp.player.action, 0);
        assert!(!gp.player.blocking);
        assert_eq!(gp.player.gb, gb, "no jump arc should start");
    }

    #[test]
    fn air_attack_from_jump_then_falls() {
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 9217, false).unwrap();
        gp.dismiss_intro();
        // Away from the spawn tutorial (its first
        // step is a `!` cutscene, Phase 4).
        gp.player.ga = 416;
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

    /// Slice 22, Gameplay routing: walk right into the
    /// box at tile 128 (grab 21), release, fresh press
    /// right pushes it 2 tiles (eQ 130), and the box
    /// slides to its new tile while the player idles
    /// (a 2-tile push leaves it out of reach).
    /// Slice 23: the wall crossing in Gameplay — push
    /// the crate to the wall, jump onto it, double jump
    /// to the roof ledge, and mount onto the roof
    /// (past the ga=2161 wall). The roof-to-temple-
    /// floor route past the hanging pillar is a
    /// level-navigation detail (double-jump onto the
    /// row-20 structure, then drop); the level-end
    /// flow is verified separately below.
    #[test]
    /// Slice 23: the temple-floor run to the level
    /// end. From the temple floor (3200,528), walk
    /// right to the lever at tile (206,32); Down
    /// pulls it, opening the pillar door (cS=6) at
    /// (213,32); then walk through to the level-end
    /// trigger at tile 214 (cn=175) and
    /// `level_complete` is set.
    #[test]
    fn lever_pull_plays_action_15_then_returns_to_idle() {
        // Phase 5: pulling the temple lever plays
        // the pull animation (15) and the pillar
        // door opens; the animation plays out to idle.
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 9217, false).unwrap();
        gp.dismiss_intro();
        gp.player.ga = 206 * 16 + 8;
        gp.player.gb = 32 * 16;
        gp.player.action = 0;
        // Press Down (block_held) — the lever fires
        // on the press edge, while grounded.
        gp.block_held = true;
        gp.tick(&mut res, 0, false);
        assert_eq!(gp.player.action, 15, "pull plays action 15");
        let door = gp.doors.iter().find(|d| d.id == 6).unwrap();
        assert!(door.open, "pillar door opens on the pull");
        // Tick until the 13-frame pull finishes.
        let mut n = 0;
        while gp.player.action != 0 && n < 100 {
            gp.tick(&mut res, 0, false);
            n += 1;
        }
        assert_eq!(gp.player.action, 0, "pull returns to idle");
    }

    #[test]
    fn rect_trigger_toggles_door_up_to_count() {
        // Phase 5, type-28: level 9217 rect 1 is
        // tile (81,28) w=22 h=8, door 5, count 1.
        // Standing on a flag-32 tile inside it
        // toggles door 5 once, then never again.
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 9217, false).unwrap();
        gp.dismiss_intro();
        let trig = gp.rect_triggers.iter().find(|t| t.door_id == 5).unwrap();
        assert_eq!((trig.x, trig.y, trig.w, trig.h, trig.count), (81, 28, 22, 8, 1));
        assert!(gp.flags.get(90, 30) & 32 != 0, "flag 32 set inside the rect");
        let door_before = gp.doors.iter().find(|d| d.id == 5).map(|d| d.open);
        gp.player.ga = 90 * 16;
        gp.player.gb = 30 * 16;
        gp.tick(&mut res, 0, false);
        let door_after = gp.doors.iter().find(|d| d.id == 5).map(|d| d.open);
        assert_ne!(door_before, door_after, "door 5 toggles on entry");
        // Staying in the rect (cooldown + count 1)
        // does not toggle it again.
        for _ in 0..60 {
            gp.tick(&mut res, 0, false);
        }
        let door_later = gp.doors.iter().find(|d| d.id == 5).map(|d| d.open);
        assert_eq!(door_after, door_later, "count-1 trigger fires only once");
    }

    #[test]
    fn collectible_orb_bundle_spawns_orbs() {
        // Phase 5, type-4: level 9217's gold bundle
        // at tile (189,8) is ft=0 value 200. Touching
        // it spawns gold orbs totalling 200 and plays
        // the pickup pose (31).
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 9217, false).unwrap();
        gp.dismiss_intro();
        gp.player.ga = 189 * 16;
        gp.player.gb = 8 * 16 + 8;
        gp.player.action = 0;
        gp.tick(&mut res, 0, false);
        assert_eq!(gp.player.action, 31, "pickup pose plays");
        let total: i32 = gp.pickups.orbs.iter().filter(|o| o.kind == crate::pickups::GOLD).map(|o| o.value).sum();
        assert_eq!(total, 200, "gold orbs total 200");
        // The item is now collected; touching again
        // spawns nothing more.
        let before = gp.pickups.orbs.len();
        gp.player.action = 0;
        gp.tick(&mut res, 0, false);
        assert_eq!(gp.pickups.orbs.len(), before, "collected item does not re-spawn");
    }

    #[test]
    fn pause_menu_freezes_world_and_kills_count() {
        // Phase 5, rest: opening the pause menu
        // freezes the world (tick is a no-op), and
        // killing an enemy increments the kill count.
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 9217, false).unwrap();
        gp.dismiss_intro();
        assert_eq!(gp.kills, 0);
        gp.open_menu();
        assert!(gp.menu_open());
        let (ga, gb) = (gp.player.ga, gp.player.gb);
        gp.tick(&mut res, 1, false);
        assert_eq!((gp.player.ga, gp.player.gb), (ga, gb), "world frozen while menu open");
        // Close the menu and kill the dY=0 enemy
        // (spawn 336,560) with a swing: stand next
        // to it and attack (the helper ticks with
        // attack=true, which starts action 54).
        gp.menu = None;
        let idx = gp.enemy_states.iter().position(|s| s.enemy.dy == 0).unwrap();
        gp.player.ga = 336 - 16;
        gp.player.gb = 560;
        gp.player.facing_right = true;
        // Weaken it first so one 2-damage swing kills.
        gp.enemy_states[idx].health = 2;
        t(&mut gp, &mut res, 0, true);
        assert_eq!(gp.player.action, 54, "swing starts");
        // Tick until the swing lands (the hit check
        // runs while the swing is active).
        for _ in 0..10 {
            t(&mut gp, &mut res, 0, false);
            if gp.enemy_states[idx].dead || gp.enemy_states[idx].dying {
                break;
            }
        }
        assert!(gp.enemy_states[idx].dead || gp.enemy_states[idx].dying, "enemy killed by the swing");
        assert_eq!(gp.kills, 1, "kill count increments on death");
    }

    #[test]
    fn collectible_upgrade_raises_max_and_refills() {
        // Phase 5, type-4: level 19456's health
        // upgrade at tile (91,12) is ft=3 fs=1.
        // Touching it raises the level at once and
        // starts the state-104 interstitial (Phase 7),
        // which grows the max by 3 per tick to 1216
        // and refills health to the new max when it
        // ends (~64 ticks), not instantly.
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 19456, false).unwrap();
        gp.dismiss_intro();
        assert_eq!(gp.player.max_health, 1024);
        gp.player.health = 500;
        gp.player.ga = 91 * 16;
        gp.player.gb = 12 * 16 + 8;
        gp.player.action = 0;
        gp.tick(&mut res, 0, false);
        assert_eq!(gp.health_level, 1);
        assert!(gp.upgrade_active(), "interstitial starts on collection");
        assert_eq!(gp.player.max_health, 1024, "max grows gradually, not at once");
        for _ in 0..70 {
            gp.tick(&mut res, 0, false);
        }
        assert!(!gp.upgrade_active(), "interstitial ends after ~64 ticks");
        assert_eq!(gp.player.max_health, 1216, "ho 0->1 raises max to 1216");
        assert_eq!(gp.player.health, 1216, "health refills to the new max");
    }

    #[test]
    fn gameplay_temple_floor_to_level_end() {
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 9217, false).unwrap();
        gp.dismiss_intro();
        // Teleport to the temple floor, facing right.
        gp.player.ga = 3200;
        gp.player.gb = 528;
        gp.player.facing_right = true;
        gp.player.action = 0;
        gp.player.slot = crate::anim::AnimSlot::new(&gp.player_anim, 0);
        let mut ticks = 0;
        let mut pulled = false;
        // Walk right to the lever (206,32) at
        // ga=3296; press Down there (one tick with
        // block_held) to pull it, then keep walking.
        while ticks < 400 && gp.level_complete.is_none() {
            ticks += 1;
            if !pulled && gp.player.ga >= 206 * 16 {
                pulled = true;
                gp.block_held = true;
                gp.tick(&mut res, 0, false);
                gp.block_held = false;
                continue;
            }
            gp.tick(&mut res, 1, false);
            // Drain any script dialog that fires
            // (and any `!` cutscene, Phase 4).
            drain_script(&mut gp, &mut res);
        }
        assert!(pulled, "the lever pull should have fired");
        assert!(gp.doors.iter().find(|d| d.id == 6).map(|d| d.open).unwrap_or(false), "pillar door should be open");
        assert!(gp.level_complete.is_some(), "level should complete; ga={} gb={} action={}", gp.player.ga, gp.player.gb, gp.player.action);
        assert_eq!(gp.level_complete, Some(1), "next level index 1");
    }

    fn gameplay_wall_crossing_mounts_roof() {
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 9217, false).unwrap();
        gp.dismiss_intro();
        gp.player.ga = 1900;
        gp.player.gb = 560;
        gp.player.facing_right = true;
        // Walk to the crate and grab it.
        for _ in 0..60 {
            t(&mut gp, &mut res, 1, false);
            if gp.player.action == 21 {
                break;
            }
        }
        assert_eq!(gp.player.action, 21, "never grabbed the crate");
        // Push it to the wall (128 -> 130 -> 132 -> 134):
        // after each push the player idles out of reach,
        // so walk back to the crate and grab it again.
        for want in [130, 132, 134] {
            t(&mut gp, &mut res, 0, false);
            t(&mut gp, &mut res, 1, false);
            assert_eq!(gp.player.action, 22, "push to {} never started", want);
            assert_eq!(gp.boxes[1].e_q, want, "box eQ {}", gp.boxes[1].e_q);
            for _ in 0..60 {
                t(&mut gp, &mut res, 0, false);
                if gp.player.action == 0 && gp.boxes[1].x == want * 16 {
                    break;
                }
            }
            assert_eq!(gp.boxes[1].x, want * 16, "box never slid to {}", want);
            if want == 134 {
                break;
            }
            // Walk back to the crate and grab it again.
            let mut grabbed = false;
            for _ in 0..60 {
                t(&mut gp, &mut res, 1, false);
                if gp.player.action == 21 {
                    grabbed = true;
                    break;
                }
            }
            assert!(grabbed, "never re-grabbed for push to {}", want + 2);
        }
        // Jump out of the grab onto the crate top.
        // After the last push the player idles out of
        // reach, so walk back and grab the crate again.
        let mut grabbed = false;
        for _ in 0..60 {
            t(&mut gp, &mut res, 1, false);
            if gp.player.action == 21 {
                grabbed = true;
                break;
            }
        }
        assert!(grabbed, "never re-grabbed the crate at the wall: ga={} action={}", gp.player.ga, gp.player.action);
        gp.jump();
        let mut on_box = false;
        for _ in 0..40 {
            t(&mut gp, &mut res, 1, false);
            if gp.player.action == 0 && gp.player.gb == 528 {
                on_box = true;
                break;
            }
        }
        assert!(on_box, "never landed on the crate: ga={} gb={} action={}", gp.player.ga, gp.player.gb, gp.player.action);
        // Double jump from the crate top to the roof ledge.
        gp.jump();
        for _ in 0..12 {
            t(&mut gp, &mut res, 1, false);
        }
        gp.jump();
        let mut hung = false;
        for _ in 0..60 {
            t(&mut gp, &mut res, 1, false);
            if gp.player.action == 37 {
                hung = true;
                break;
            }
        }
        assert!(hung, "never grabbed the roof ledge: ga={} gb={} action={}", gp.player.ga, gp.player.gb, gp.player.action);
        assert_eq!(gp.player.gb, 26 * 16 + crate::player::PLAYER_GD);
        // Mount onto the roof.
        gp.jump();
        assert_eq!(gp.player.action, 38, "mount never started: {}", gp.player.action);
        for _ in 0..40 {
            t(&mut gp, &mut res, 0, false);
            if gp.player.action == 0 {
                break;
            }
        }
        assert_eq!(gp.player.action, 0, "mount never finished");
        assert_eq!(gp.player.gb, 26 * 16, "not on the roof: gb={}", gp.player.gb);
        assert!(gp.player.ga > 136 * 16, "not past the wall: ga={}", gp.player.ga);
    }

    #[test]
    fn gameplay_box_grab_push() {
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 9217, false).unwrap();
        gp.dismiss_intro();
        gp.player.ga = 1900;
        gp.player.gb = 560;
        gp.player.facing_right = true;
        let mut grabbed = false;
        for _ in 0..60 {
            t(&mut gp, &mut res, 1, false);
            if gp.player.action == 21 {
                grabbed = true;
                break;
            }
        }
        assert!(
            grabbed,
            "never grabbed: ga {} action {}",
            gp.player.ga, gp.player.action
        );
        assert_eq!(gp.player.grabbed_box, Some(1), "the crate is item 1");
        assert_eq!(gp.boxes[1].e_q, 128);
        // Release the direction (so the next press is
        // a fresh edge), then press right to push.
        t(&mut gp, &mut res, 0, false);
        assert_eq!(gp.player.action, 21, "grab must hold through the release");
        t(&mut gp, &mut res, 1, false);
        assert_eq!(
            gp.player.action, 22,
            "fresh press pushes: action {}",
            gp.player.action
        );
        assert_eq!(gp.boxes[1].e_q, 130, "push moves 2 tiles: eQ {}", gp.boxes[1].e_q);
        // The push plays out (~25 ticks); the box
        // slides to its new tile (its action runs a
        // tick longer than the player's push) and the
        // player idles (a 2-tile push leaves the box
        // out of reach).
        for _ in 0..60 {
            t(&mut gp, &mut res, 0, false);
            if gp.player.action == 0 && gp.boxes[1].x == 130 * 16 {
                break;
            }
        }
        assert_eq!(
            gp.player.action, 0,
            "after the push the player idles: {}",
            gp.player.action
        );
        assert_eq!(gp.boxes[1].x, 130 * 16, "box slid to its new tile: {}", gp.boxes[1].x);
    }

    /// Phase 7: death on the ground plays action 52
    /// and, when its animation ends, sets
    /// `death_finished` (the caller shows state
    /// 108). The player is killed by setting health
    /// to 0 on a floor tile away from the spawn
    /// (the spawn-area prayer script would
    /// otherwise overwrite the action with its
    /// cutscene).
    #[test]
    fn death_grounded_plays_52_then_finishes() {
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 9217, false).unwrap();
        gp.dismiss_intro();
        gp.player.ga = 100 * 16;
        gp.player.gb = 35 * 16;
        gp.player.health = 0;
        t(&mut gp, &mut res, 0, false);
        assert_eq!(gp.player.action, 52, "grounded death plays action 52");
        assert!(!gp.death_finished, "not finished on the first tick");
        let mut finished = false;
        for _ in 0..100 {
            t(&mut gp, &mut res, 0, false);
            if gp.death_finished {
                finished = true;
                break;
            }
        }
        assert!(finished, "action 52 ends within 100 ticks (~4 s)");
        assert!(gp.player.death_finished);
    }

    /// Phase 7: death in the air plays the fall
    /// (action 26) first, then 52 on landing. The
    /// player is teleported above the floor with
    /// health 0.
    #[test]
    fn death_airborne_falls_then_plays_52() {
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 9217, false).unwrap();
        gp.dismiss_intro();
        gp.player.ga = 100 * 16;
        gp.player.gb = 30 * 16;
        gp.player.health = 0;
        t(&mut gp, &mut res, 0, false);
        assert_eq!(gp.player.action, 26, "airborne death falls first");
        let mut played_52 = false;
        for _ in 0..100 {
            t(&mut gp, &mut res, 0, false);
            if gp.player.action == 52 {
                played_52 = true;
                break;
            }
        }
        assert!(played_52, "52 plays after landing");
    }

    /// Phase 7: standing on a flag-1024 tile
    /// (type-25) sets the checkpoint. Level 9217
    /// has a type-25 column at tile x=70, y 28-33.
    #[test]
    fn checkpoint_updates_on_flag_1024() {
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 9217, false).unwrap();
        gp.dismiss_intro();
        assert_eq!((gp.checkpoint_x, gp.checkpoint_y), (48, 560), "checkpoint starts at spawn");
        gp.player.ga = 70 * 16;
        gp.player.gb = 28 * 16;
        t(&mut gp, &mut res, 0, false);
        assert_eq!(gp.checkpoint_x, 70 * 16, "checkpoint x follows the flag tile");
        assert_eq!(gp.checkpoint_y, 28 * 16, "checkpoint y follows the flag tile");
        assert!(gp.checkpoint_facing, "facing recorded from the player");
    }

    /// Phase 7: a dead Arena enemy respawns after
    /// its timer runs out — the Arena ignores the
    /// respawn count, so it comes back even with
    /// `ec` exhausted. The player is moved far
    /// away so the distance gate holds.
    #[test]
    fn arena_enemy_respawns_after_timer() {
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 9216, true).unwrap();
        let st = &mut gp.enemy_states[0];
        st.dead = true;
        st.enemy.ec = 0;
        st.er = 40;
        let (sx, sy) = (st.enemy.x, st.enemy.y);
        gp.player.ga = 0;
        gp.player.gb = 0;
        t(&mut gp, &mut res, 0, false);
        let st = &gp.enemy_states[0];
        assert!(!st.dead, "Arena enemy respawns when the timer runs out");
        assert_eq!(st.health, st.max_health, "respawned at full health");
        assert_eq!((st.x, st.y), (sx, sy), "respawned at its spawn point");
        // Phase 8: the enemy returns playing its
        // spawn-in action (27 kind 11, 14 kind 12,
        // 18 kind 13), not idle.
        let kind = st.enemy.kind;
        let want = match kind {
            12 => 14,
            13 => 18,
            _ => 27,
        };
        assert_eq!(st.action, want, "respawned with the spawn-in action");
        assert!(st.spawn_in > 0, "spawn-in timer runs");
        // The spawn-in plays out and the enemy goes
        // idle (the 1-frame loops exit on the timer;
        // the one-shot exits on its repeat end).
        let mut guard = 0;
        while gp.enemy_states[0].spawn_in > 0 && guard < 100 {
            guard += 1;
            t(&mut gp, &mut res, 0, false);
        }
        assert_eq!(gp.enemy_states[0].action, 0, "spawn-in ends in idle");
        assert_eq!(gp.enemy_states[0].spawn_in, 0, "spawn-in timer clears");
    }

    /// Phase 7: a dead level enemy with an
    /// exhausted respawn count stays dead.
    #[test]
    fn level_enemy_no_respawn_when_ec_exhausted() {
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 9217, false).unwrap();
        gp.dismiss_intro();
        let st = &mut gp.enemy_states[0];
        st.dead = true;
        st.enemy.ec = 0;
        st.er = 40;
        t(&mut gp, &mut res, 0, false);
        assert!(gp.enemy_states[0].dead, "exhausted count: no respawn");
    }

    /// Phase 7: a dead level enemy with count left
    /// respawns after its timer. The player is
    /// moved far away so the distance gate holds.
    #[test]
    fn level_enemy_respawns_with_count_left() {
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 9217, false).unwrap();
        gp.dismiss_intro();
        let st = &mut gp.enemy_states[0];
        st.dead = true;
        st.enemy.ec = 2;
        st.er = 40;
        gp.player.ga = 0;
        gp.player.gb = 0;
        t(&mut gp, &mut res, 0, false);
        let st = &gp.enemy_states[0];
        assert!(!st.dead, "count left: respawns");
        assert_eq!(st.enemy.ec, 1, "respawn consumes one count");
    }

    /// Phase 7: the state-104 interstitial grows
    /// the max by 3 per tick and ends after 64
    /// ticks with the max up by 192 and health
    /// refilled. Level 19456 has a health upgrade;
    /// the player teleports onto it.
    #[test]
    fn upgrade_interstitial_grows_max_and_ends() {
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 19456, false).unwrap();
        let (ux, uy) = gp
            .collectibles
            .iter()
            .find(|it| it.kind == 3 && it.value > 0)
            .map(|it| (it.x, it.y))
            .expect("19456 has a health upgrade");
        gp.player.ga = ux;
        gp.player.gb = uy + 8;
        t(&mut gp, &mut res, 0, false);
        assert!(gp.upgrade_active(), "collecting the upgrade starts the interstitial");
        assert_eq!(gp.health_level, 1, "level rises at collection");
        assert_eq!(gp.pending_he_health, Some(true), "`he` bit pending for the save");
        assert_eq!(gp.player.max_health, 1024, "max not yet grown on the first tick");
        for _ in 0..70 {
            t(&mut gp, &mut res, 0, false);
        }
        assert!(!gp.upgrade_active(), "interstitial ends after ~64 ticks");
        assert_eq!(gp.player.max_health, 1216, "max grew by 192");
        assert_eq!(gp.player.health, 1216, "health refilled to the new max");
    }

    /// Phase 7: `mark_collected_upgrades` starts an
    /// already-collected upgrade as collected, so
    /// it cannot be granted twice.
    #[test]
    fn he_bit_marks_upgrade_collected() {
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 19456, false).unwrap();
        // 19456 is level index 1: health bit 1<<1.
        gp.mark_collected_upgrades(1 << 1);
        let health = gp
            .collectibles
            .iter()
            .find(|it| it.kind == 3 && it.value > 0)
            .expect("19456 has a health upgrade");
        assert!(health.collected, "health upgrade starts collected with the bit set");
        let magic = gp
            .collectibles
            .iter()
            .find(|it| it.kind == 3 && it.value == 0)
            .expect("19456 has a magic upgrade");
        assert!(!magic.collected, "magic upgrade untouched by the health bit");
    }

    /// Phase 8: Down on a weakened kind-12 enemy in
    /// reach starts the grapple (player action 88,
    /// fQ 2, fS 0, the slot-131 bar set to a prompt).
    #[test]
    fn grapple_starts_on_weakened_kind12() {
        let mut res = crate::resources::Resources::new("../extracted");
        // 19456 has kind-12 enemies; 9217 has none
        // (8 kind 11, 1 kind 13).
        let mut gp = super::Gameplay::load(&mut res, 19456, false).unwrap();
        gp.dismiss_intro();
        // Find a kind-12 enemy and stand next to it.
        let idx = gp
            .enemy_states
            .iter()
            .position(|st| st.enemy.kind == 12)
            .expect("19456 has a kind-12 enemy");
        let (ex, ey) = (gp.enemy_states[idx].x, gp.enemy_states[idx].y);
        gp.enemy_states[idx].dx = 0;
        {
            let anim_set = gp.enemy_states[idx].anim_set.clone().unwrap();
            gp.enemy_states[idx].set_action(&anim_set, 0);
        }
        gp.player.ga = ex - 20;
        gp.player.gb = ey;
        gp.player.action = 0;
        gp.player.slot = crate::anim::AnimSlot::new(&gp.player_anim, 0);
        // Down press (edge): block_held false -> true.
        gp.block_held = true;
        t(&mut gp, &mut res, 0, false);
        assert!(gp.grapple.is_some(), "grapple starts on a weakened enemy");
        let gr = gp.grapple.as_ref().unwrap();
        assert_eq!(gr.enemy, idx);
        assert_eq!(gr.f_q, 2, "kind 12 needs 2 mashes");
        assert_eq!(gr.f_s, 0);
        assert_eq!(gp.player.action, 88, "kind-12 grapple plays action 88");
        assert!(gp.boss_bar.is_some(), "slot-131 bar loads");
        assert_eq!(
            gp.boss_bar.as_ref().unwrap().slot.action,
            super::GRAPPLE_PROMPT_ACTIONS[gr.f_t as usize],
            "bar shows the prompt for the required button"
        );
    }

    /// Phase 8: pressing the required button mashes
    /// (f_s += 1, new f_t, bar updates); a wrong
    /// button breaks the grapple (f_s = -1).
    #[test]
    fn grapple_mash_correct_advances_wrong_breaks() {
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 19456, false).unwrap();
        gp.dismiss_intro();
        let idx = gp
            .enemy_states
            .iter()
            .position(|st| st.enemy.kind == 12)
            .expect("19456 has a kind-12 enemy");
        let (ex, ey) = (gp.enemy_states[idx].x, gp.enemy_states[idx].y);
        gp.enemy_states[idx].dx = 0;
        {
            let anim_set = gp.enemy_states[idx].anim_set.clone().unwrap();
            gp.enemy_states[idx].set_action(&anim_set, 0);
        }
        gp.player.ga = ex - 20;
        gp.player.gb = ey;
        gp.block_held = true;
        t(&mut gp, &mut res, 0, false);
        assert!(gp.grapple.is_some());
        // Correct button for the current f_t.
        let f_t = gp.grapple.as_ref().unwrap().f_t;
        let want = match f_t {
            0 => 1,
            1 => 2,
            2 => 8,
            3 => 5,
            _ => 6,
        };
        gp.key_code = want;
        t(&mut gp, &mut res, 0, false);
        assert_eq!(gp.grapple.as_ref().unwrap().f_s, 1, "correct mash counts");
        // A wrong button breaks the grapple.
        let f_t = gp.grapple.as_ref().unwrap().f_t;
        let wrong = match f_t {
            0 => 2,
            1 => 1,
            2 => 1,
            3 => 1,
            _ => 1,
        };
        assert_ne!(wrong, match f_t { 0 => 1, 1 => 2, 2 => 8, 3 => 5, _ => 6 });
        gp.key_code = wrong;
        t(&mut gp, &mut res, 0, false);
        assert_eq!(gp.grapple.as_ref().unwrap().f_s, -1, "wrong button breaks it");
    }

    /// Phase 8: mashing f_q times then letting the
    /// grapple action end kills the enemy (orbs
    /// spawn) and plays the success pose 89.
    #[test]
    fn grapple_success_kills_enemy() {
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 19456, false).unwrap();
        gp.dismiss_intro();
        let idx = gp
            .enemy_states
            .iter()
            .position(|st| st.enemy.kind == 12)
            .expect("19456 has a kind-12 enemy");
        let (ex, ey) = (gp.enemy_states[idx].x, gp.enemy_states[idx].y);
        gp.enemy_states[idx].dx = 0;
        {
            let anim_set = gp.enemy_states[idx].anim_set.clone().unwrap();
            gp.enemy_states[idx].set_action(&anim_set, 0);
        }
        gp.player.ga = ex - 20;
        gp.player.gb = ey;
        gp.block_held = true;
        t(&mut gp, &mut res, 0, false);
        assert!(gp.grapple.is_some());
        let f_q = gp.grapple.as_ref().unwrap().f_q;
        let orbs_before = gp.pickups.orbs.len();
        for _ in 0..f_q {
            let f_t = gp.grapple.as_ref().unwrap().f_t;
            let want = match f_t {
                0 => 1,
                1 => 2,
                2 => 8,
                3 => 5,
                _ => 6,
            };
            gp.key_code = want;
            t(&mut gp, &mut res, 0, false);
        }
        assert_eq!(gp.grapple.as_ref().unwrap().f_s, f_q, "all mashes done");
        // Let the grapple action (88) run to its end.
        gp.block_held = false;
        let mut guard = 0;
        while gp.grapple.is_some() && guard < 200 {
            guard += 1;
            t(&mut gp, &mut res, 0, false);
        }
        assert!(gp.grapple.is_none(), "grapple resolves when action 88 ends");
        assert_eq!(gp.player.action, 89, "success plays action 89");
        assert!(
            gp.enemy_states[idx].dying || gp.enemy_states[idx].dead,
            "enemy dies on a successful grapple"
        );
        assert!(
            gp.pickups.orbs.len() > orbs_before,
            "success spawns the enemy's orbs"
        );
    }

    /// Phase 8: a broken grapple (wrong button) plays
    /// the fail pose 90 and leaves the enemy alive.
    #[test]
    fn grapple_failure_plays_90_and_spares_enemy() {
        let mut res = crate::resources::Resources::new("../extracted");
        let mut gp = super::Gameplay::load(&mut res, 19456, false).unwrap();
        gp.dismiss_intro();
        let idx = gp
            .enemy_states
            .iter()
            .position(|st| st.enemy.kind == 12)
            .expect("19456 has a kind-12 enemy");
        let (ex, ey) = (gp.enemy_states[idx].x, gp.enemy_states[idx].y);
        gp.enemy_states[idx].dx = 0;
        {
            let anim_set = gp.enemy_states[idx].anim_set.clone().unwrap();
            gp.enemy_states[idx].set_action(&anim_set, 0);
        }
        gp.player.ga = ex - 20;
        gp.player.gb = ey;
        gp.block_held = true;
        t(&mut gp, &mut res, 0, false);
        assert!(gp.grapple.is_some());
        // Break it with a wrong button.
        let f_t = gp.grapple.as_ref().unwrap().f_t;
        let want = match f_t {
            0 => 1,
            1 => 2,
            2 => 8,
            3 => 5,
            _ => 6,
        };
        let wrong = if want == 1 { 2 } else { 1 };
        gp.key_code = wrong;
        t(&mut gp, &mut res, 0, false);
        assert_eq!(gp.grapple.as_ref().unwrap().f_s, -1);
        gp.block_held = false;
        let mut guard = 0;
        while gp.grapple.is_some() && guard < 200 {
            guard += 1;
            t(&mut gp, &mut res, 0, false);
        }
        assert!(gp.grapple.is_none(), "grapple resolves when action 88 ends");
        assert_eq!(gp.player.action, 90, "failure plays action 90");
        assert!(
            !gp.enemy_states[idx].dying && !gp.enemy_states[idx].dead,
            "enemy survives a failed grapple"
        );
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





