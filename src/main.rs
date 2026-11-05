//! gow-betrayal-pc — RP pack inspector + MIDP shim render test.
//!
//! Usage:
//!   cargo run --release -- <path-to-directory-with-RP-files>
//!   cargo run --release -- <dir> --render-test <out.png>
//!
//! The first mode is the step-1 inspector (RP packs + RP10 strings).
//! The second mode is the step-2 MIDP shim proof: it loads RP13, decodes
//! its PNG sheet with the std-only PNG decoder, draws the sheet, one
//! sprite frame (normal and flipped), and a palette-variant sheet onto a
//! 240x320 framebuffer, and writes the result as a PNG. No game logic yet
//! — `e.java` translation is step 3.
//!
//! Build with the cargo in ~/.cargo/bin (not on the default PATH here):
//!   ~/.cargo/bin/cargo run --release -- ../extracted --render-test ../test-frame.png

mod pickups;
mod script;
mod boxes;
mod doors;
#[cfg(feature = "window")]
mod play_window;
mod anim;
mod crc;
mod midi;
mod enemies;
mod entities;
mod font;
mod hud;
mod gameplay;
mod window;
mod save;
mod audio;
mod game;
mod level;
mod menu;
mod midp;
mod player;
mod png;
mod resources;
mod rp;
mod sprite;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use midp::{Framebuffer, Graphics, Image};
use rp::RpPack;

fn load_pack(dir: &Path, name: &str) -> RpPack {
    let data = fs::read(dir.join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
    rp::parse_pack(&data).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn inspect_rp(path: &Path) {
    let data = match fs::read(path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{}: cannot read: {e}", path.display());
            return;
        }
    };
    match rp::parse_pack(&data) {
        Ok(pack) => {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            println!(
                "{}: chunks={} sum={} flags={:?}",
                name,
                pack.sizes.len(),
                pack.sizes.iter().sum::<usize>(),
                &pack.flags[..pack.flags.len().min(8)]
            );
            if name == "RP10" {
                if let Some(chunk0) = pack.chunk(0) {
                    match rp::parse_string_table(chunk0) {
                        Ok(strings) => {
                            println!("  RP10 strings ({}):", strings.len());
                            for (i, s) in strings.iter().take(20).enumerate() {
                                println!("    [{i}] {s:?}");
                            }
                        }
                        Err(e) => eprintln!("  RP10 string table: {e}"),
                    }
                }
            }
        }
        Err(e) => eprintln!("{}: {e}", path.display()),
    }
}

fn render_test(dir: &Path, out: &Path) {
    // RP13: def chunk 1 -> image chunk 2, palette keys chunks 3/4/5.
    let pack13 = load_pack(dir, "RP13");
    assert_eq!(pack13.flags[1], 254, "RP13 chunk 1 should be a sprite def");
    assert_eq!(pack13.flags[2], 253, "RP13 chunk 2 should be a PNG");
    let png_bytes = pack13.chunk(2).unwrap().to_vec();
    let sheet = Image::from_png(&png_bytes).expect("decode RP13 sheet");
    println!(
        "RP13 sheet: {}x{}, def frames:",
        sheet.width, sheet.height
    );
    let def = sprite::parse_sprite_def(pack13.chunk(1).unwrap()).expect("parse RP13 def");
    println!("  image_chunk={} frames={}", def.image_chunk, def.frames.len());
    assert_eq!(def.image_chunk, 2);

    let mut fb = Framebuffer::game();
    fb.clear(0xFF20_2020);
    let mut g = Graphics::default();
    g.set_clip_full(240, 320);

    // Sheet itself, top-left.
    g.draw_region(&mut fb, &sheet, 0, 0, sheet.width as i32, sheet.height as i32, 0, 20, 10, 20);
    // Largest-area sprite frame, normal and X-flipped, below the sheet.
    // (Frame 0 of this def is only 30x2, so it would be invisible.)
    if let Some((fi, frame)) = def
        .frames
        .iter()
        .enumerate()
        .max_by_key(|(_, f)| f.w * f.h)
    {
        println!(
            "  largest frame {fi}: anchor=({},{}) src=({},{}) size={}x{}",
            frame.anchor_x, frame.anchor_y, frame.src_x, frame.src_y, frame.w, frame.h
        );
        midp::draw_sprite_frame(&g, &mut fb, &sheet, frame, 20, 110, 0);
        midp::draw_sprite_frame(&g, &mut fb, &sheet, frame, 180, 110, 2);
    }
    // Palette variant 0 (key chunk 3), below the frames.
    let variant_png = sprite::apply_palette_key(&png_bytes, pack13.chunk(3).unwrap())
        .expect("apply RP13 palette key 3");
    let variant = Image::from_png(&variant_png).expect("decode RP13 variant");
    g.draw_region(
        &mut fb,
        &variant,
        0,
        0,
        variant.width as i32,
        variant.height as i32,
        0,
        20,
        200,
        20,
    );
    let rgba = fb.to_rgba();
    let png_out = png::encode_png_rgba(fb.width, fb.height, &rgba);
    fs::write(out, &png_out).unwrap_or_else(|e| panic!("write {}: {e}", out.display()));
    println!("wrote {} ({} bytes, 240x320)", out.display(), png_out.len());
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let target = args.get(1).map(PathBuf::from).unwrap_or_else(|| {
        eprintln!("usage: gow-betrayal-pc <dir-with-RP-files> [--render-test <out.png>]");
        std::process::exit(2);
    });
    if !target.is_dir() {
        eprintln!(
            "Note: direct jar reading needs unzip first (jars are zips).\n\
             Run: unzip -o <jar> -d extracted && cargo run --release -- extracted"
        );
        std::process::exit(2);
    }
    if args.get(2).map(|s| s.as_str()) == Some("--play-window") {
        // --play-window [factor] [save-dir]
        // Live OS window + keyboard (slice 4a part 2). Requires the
        // window feature: cargo run --release --features window.
        let factor: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(3);
        let save_dir = args.get(4).map(|s| s.as_str());
        #[cfg(feature = "window")]
        {
            std::process::exit(play_window::run(&target, factor, save_dir));
        }
        #[cfg(not(feature = "window"))]
        {
            eprintln!("--play-window needs the window feature: rebuild with --features window");
            std::process::exit(2);
        }
    }
    if args.get(2).map(|s| s.as_str()) == Some("--entities") {
        // --entities <level-id> [out-prefix]
        let level_id: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(9217);
        let mut res = resources::Resources::new(&target);
        let chunk = res.chunk(level_id).expect("level chunk");
        let parsed = entities::parse_level_entities(level_id, &chunk).expect("parse entities");
        println!(
            "level {}: {}x{} tiles, {} groups, {} entities, bits {}/{} ({} left)",
            parsed.level.id,
            parsed.level.width_tiles,
            parsed.level.height_tiles,
            parsed.groups.len(),
            parsed.total_entities(),
            parsed.bits_consumed,
            parsed.chunk_bits,
            parsed.chunk_bits - parsed.bits_consumed
        );
        for g in &parsed.groups {
            let first = g.positions.first().copied().unwrap_or((0, 0));
            let last = g.positions.last().copied().unwrap_or((0, 0));
            println!(
                "  type {:2} count {:4} first=({},{}) last=({},{}) extra_values={}",
                g.kind,
                g.positions.len(),
                first.0,
                first.1,
                last.0,
                last.1,
                g.extra.len()
            );
        }
        if let Some(spawn) = parsed.group(26) {
            let pos = spawn.positions.first().copied().unwrap_or((0, 0));
            let cam = entities::start_camera(&parsed.level, pos);
            println!(
                "  player spawn: {:?}, facing_right={}, start camera: {:?}",
                pos, parsed.level.facing_right, cam
            );
        }
        for g in &parsed.groups {
            if let Some(name) = entities::kind_name(g.kind) {
                println!("  type {:2}: {}", g.kind, name);
            }
        }
        return;
    }
    if args.get(2).map(|s| s.as_str()) == Some("--scene") {
        // --scene <level-id> [cam_x cam_y] [out-prefix]
        // Tiles (3a) + type-23 static sprites (3b-3), at a camera.
        let level_id: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(9217);
        let cam_x: i32 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(0);
        let cam_y: i32 = args.get(5).and_then(|s| s.parse().ok()).unwrap_or(0);
        let prefix = args.get(6).map(|s| s.as_str()).unwrap_or("../scene-frame");
        let mut res = resources::Resources::new(&target);
        let chunk = res.chunk(level_id).expect("level chunk");
        let parsed = entities::parse_level_entities(level_id, &chunk).expect("parse entities");
        let tileset = res.image(2058, None).expect("tileset 2058");
        let mut fb = midp::Framebuffer::game();
        level::draw_view(&parsed.level, &tileset, &mut fb, cam_x, cam_y);
        // Static sprites: anim set 3073, sprite set 3076 (both RP3,
        // base palette — bC=0 -> bG[0]=-1, no key, as in case 4).
        let anim_chunk = res.chunk(3073).expect("anim set 3073");
        let anim_set = anim::load_anim_set(&anim_chunk).expect("load anim set");
        let sprite_set = res.sprite_set(3076, None).expect("sprite set 3076");
        println!(
            "sprite set 3076: {} frames, anim set 3073: {} actions",
            sprite_set.frames.len(),
            anim_set.b.len()
        );
        let g = midp::Graphics::default();
        if let Some(group) = parsed.group(23) {
            anim::draw_static_sprites(
                &g,
                &mut fb,
                &sprite_set,
                &anim_set,
                &group.positions,
                &group.extra,
                cam_x,
                cam_y,
            );
            println!("drew {} static sprite placements", group.positions.len());
        }
        // Enemies (3d-1): type 11/12/13 at spawn, before the player.
        let enemy_list = enemies::enemies(&parsed);
        let (drawn, skipped) = enemies::draw_enemies(&g, &mut fb, &mut res, &enemy_list, cam_x, cam_y);
        println!("enemies: {} drawn, {} skipped, {} total", drawn, skipped, enemy_list.len());
        for e in &enemy_list {
            println!(
                "  enemy {}: type {} at ({},{}) facing_right={} health={} anim={} sprite={} action={}",
                e.index, e.kind, e.x, e.y, e.facing_right, e.health, e.anim_id, e.sprite_id, e.action
            );
        }
        // Player (3c-1): slot 135, anim set 28672, action 0 (idle,
        // from s(0) in t()), sprite set 28673 (fH), at spawn (ga, gb),
        // transform = facing_right ? 0 : 2, as in s(Graphics).
        // Drawn after tiles + static sprites, as in d(Graphics).
        let player_anim_chunk = res.chunk(28672).expect("player anim set 28672");
        let player_anim = anim::load_anim_set(&player_anim_chunk).expect("load player anim");
        let player_sprites = res.sprite_set(28673, None).expect("player sprite set 28673");
        if let Some(spawn_group) = parsed.group(26) {
            if let Some(&(ga, gb)) = spawn_group.positions.first() {
                let player_transform = if parsed.level.facing_right { 0 } else { 2 };
                let frames = anim::frames_for_action_transform(&player_anim, 0, player_transform)
                    .expect("player action 0");
                println!(
                    "player: spawn ({},{}), anim 28672 action 0, {} frames, sprite set 28673: {} frames, transform {}",
                    ga, gb, frames.len(), player_sprites.frames.len(), player_transform
                );
                for fr in frames {
                    if let Some(frame) = player_sprites.frame(fr.sprite_idx) {
                        midp::draw_sprite_frame(
                            &g,
                            &mut fb,
                            &player_sprites.image,
                            frame,
                            ga + fr.dx - cam_x,
                            gb + fr.dy - cam_y,
                            fr.transform,
                        );
                    }
                }
            }
        }
        let rgba = fb.to_rgba();
        fs::write(
            format!("{prefix}.png"),
            png::encode_png_rgba(fb.width, fb.height, &rgba),
        )
        .expect("write scene");
        println!("wrote {prefix}.png (camera {cam_x},{cam_y})");
        return;
    }
    if args.get(2).map(|s| s.as_str()) == Some("--audio") {
        // --audio [out-prefix]
        // Extracts all 11 MIDI chunks (RP1 30-40) as .mid files
        // and validates each, plus reports the iM slot mapping.
        let prefix = args.get(3).map(|s| s.as_str()).unwrap_or("../audio");
        let mut res = resources::Resources::new(&target);
        for &id in audio::MIDI_RESOURCE_IDS.iter() {
            match res.chunk(id) {
                Ok(chunk) => match audio::validate_midi(&chunk) {
                    Ok((fmt, ntrk, div)) => {
                        let out = format!("{prefix}-{id}.mid");
                        fs::write(&out, &chunk).expect("write midi");
                        let slot = audio::SLOTS.iter().find(|(_, rid, _)| *rid == id).map(|(s, _, _)| *s);
                        let parsed = midi::parse_song(&chunk)
                            .map(|s| format!("{} events, {} ms", s.events.len(), s.duration_ms))
                            .unwrap_or_else(|e| format!("parse FAILED: {e}"));
                        println!("midi {id}: format {fmt}, tracks {ntrk}, division {div}, {len} bytes, slot {slot:?}, {parsed}, wrote {out}", len = chunk.len());
                    }
                    Err(e) => println!("midi {id}: INVALID: {e}"),
                },
                Err(e) => println!("midi {id}: FAILED: {e:?}"),
            }
        }
        return;
    }
    if args.get(2).map(|s| s.as_str()) == Some("--levels") {
        // --levels [out-prefix]
        // Loads every level (bB) + Arena through Gameplay::load
        // and renders the start-camera frame for each. This is
        // the all-levels proof: parse, spawn, enemies, draw.
        let prefix = args.get(3).map(|s| s.as_str()).unwrap_or("../level-all");
        let mut res = resources::Resources::new(&target);
        let mut ids: Vec<u32> = gameplay::LEVEL_IDS.to_vec();
        ids.push(gameplay::ARENA_ID);
        for id in ids {
            let arena = id == gameplay::ARENA_ID;
            match gameplay::Gameplay::load(&mut res, id, arena) {
                Ok(gp) => {
                    let font_set = res.sprite_set(1031, None).expect("font 1031");
                    let font = font::Font::from_sprite_set(font_set);
                    let mut fb = midp::Framebuffer::game();
                    gp.draw(&mut fb, &mut res, &font);
                    let rgba = fb.to_rgba();
                    let out = format!("{prefix}-{id}.png");
                    fs::write(&out, png::encode_png_rgba(fb.width, fb.height, &rgba))
                        .expect("write level frame");
                    let dead = gp.enemy_states.iter().filter(|st| st.dead).count();
                    println!(
                        "level {id}: {}x{} tiles, spawn=({},{}) cam=({},{}) groups={} enemies={} (dead {}) level_index={} tileset_key={:?} sprite_key={:?} wrote {out}",
                        gp.parsed.level.width_tiles,
                        gp.parsed.level.height_tiles,
                        gp.player.ga,
                        gp.player.gb,
                        gp.player.cam_x,
                        gp.player.cam_y,
                        gp.parsed.groups.len(),
                        gp.enemy_states.len(),
                        dead,
                        gp.level_index,
                        gameplay::palette_key(2068, gp.level_index),
                        gameplay::palette_key(3084, gp.level_index)
                    );
                }
                Err(e) => {
                    println!("level {id}: FAILED: {e}");
                }
            }
        }
        return;
    }
    if args.get(2).map(|s| s.as_str()) == Some("--play") {
        // --play <level-id> <keys> [out-prefix]
        // keys: comma-separated left,right,tick (40 ms each).
        // Walk (3c-2) + camera follow, rendered at the final tick.
        let level_id: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(9217);
        let keys: Vec<String> = args
            .get(4)
            .map(|k| k.split(',').map(|x| x.trim().to_string()).collect())
            .unwrap_or_default();
        let prefix = args.get(5).map(|s| s.as_str()).unwrap_or("../play-frame");
        let mut res = resources::Resources::new(&target);
        let chunk = res.chunk(level_id).expect("level chunk");
        let parsed = entities::parse_level_entities(level_id, &chunk).expect("parse entities");
        let footprints = entities::load_footprints(&mut res, &parsed).expect("load footprints");
        let mut flags = player::TileFlags::build(&parsed, &footprints);
        // Type-27 boxes (slice 22): anim 6144 + sprite
        // 6146, base palette in --play (no key).
        let box_anim_chunk = res.chunk(boxes::BOX_ANIM_SET).expect("box anim set 6144");
        let box_anim = anim::load_anim_set(&box_anim_chunk).expect("load box anim");
        let box_sprites = res
            .sprite_set(boxes::BOX_SPRITE_SET, None)
            .expect("box sprite set 6146");
        let mut play_boxes = boxes::load_boxes(&mut res, &parsed, None).expect("load boxes");
        // Type-9 switches and type-35 doors (slice 23).
        let play_switches = doors::load_switches(&parsed);
        let mut play_doors = doors::load_doors(&mut res, &parsed, None).expect("load doors");
        // Type-34 levers (slice 23).
        let mut play_levers = doors::load_levers(&parsed);
        let mut prev_block_held = false;
        let player_anim_chunk = res.chunk(28672).expect("player anim set 28672");
        let player_anim = anim::load_anim_set(&player_anim_chunk).expect("load player anim");
        let player_sprites = res.sprite_set(28673, None).expect("player sprite set 28673");
        let spawn = parsed.group(26).and_then(|g| g.positions.first().copied()).expect("spawn");
        let (start_cam_x, start_cam_y) = entities::start_camera(&parsed.level, spawn);
        let mut pl = player::Player::new(
            &player_anim,
            spawn,
            parsed.level.facing_right,
            start_cam_x,
            start_cam_y,
        );
        // Type-3 camera zones (slice 23).
        pl.set_camera_zones(entities::camera_zones(&parsed));
        let enemy_list0 = enemies::enemies(&parsed);
        let mut enemy_states: Vec<enemies::EnemyState> = Vec::new();
        for e in enemy_list0 {
            if let Ok(anim_chunk) = res.chunk(e.anim_id) {
                if let Ok(anim_set) = anim::load_anim_set(&anim_chunk) {
                    let mut st = enemies::EnemyState::new(e.clone(), &anim_set);
                    if let Ok(sprites) = res.sprite_set(e.sprite_id, None) {
                        st.ef = enemies::enemy_bbox_height(&anim_set, &sprites, e.action);
                    }
                    enemy_states.push(st);
                }
            }
        }
        // Weakened-marker sets (slice 14).
        let mut bar_state: Option<(
            anim::AnimSet,
            resources::SpriteSet,
            anim::AnimSlot,
        )> = match (
            res.chunk(1027).ok().and_then(|c| anim::load_anim_set(&c).ok()),
            res.sprite_set(1040, None).ok(),
        ) {
            (Some(anim_set), Some(sprites)) => {
                let slot = anim::AnimSlot::new(&anim_set, 10);
                Some((anim_set, sprites, slot))
            }
            _ => None,
        };
        // Boss-bar sets (Phase 2): same anim set
        // 1027 + sprite set 1040 as the marker, but
        // slot 131 with the latched action (set on
        // latch; initialised to action 2).
        let mut boss_bar_state: Option<(
            anim::AnimSet,
            resources::SpriteSet,
            anim::AnimSlot,
        )> = match (
            res.chunk(1027).ok().and_then(|c| anim::load_anim_set(&c).ok()),
            res.sprite_set(1040, None).ok(),
        ) {
            (Some(anim_set), Some(sprites)) => {
                let slot = anim::AnimSlot::new(&anim_set, 2);
                Some((anim_set, sprites, slot))
            }
            _ => None,
        };
        let mut held = 0;
        let mut prev_held = 0;
        let mut block_held = false;
        let mut combo_hits = 0;
        let mut combo_timer_ms = 0;
        let mut pickups = pickups::Pickups::default();
        let mut gold = 0;
        let mut magic = 0;
        let mut orb_rng = pickups::OrbRng(0x9E3779B9);
        // Script triggers (type 8) + English strings.
        let mut script_triggers = Vec::new();
        if let Some(g8) = parsed.group(8) {
            for (i, &(px, py)) in g8.positions.iter().enumerate() {
                script_triggers.push(script::ScriptTrigger {
                    tile_x: px / 16,
                    tile_y: py / 16,
                    start: g8.extra.get(i * 2).copied().unwrap_or(0) as usize,
                    len: g8.extra.get(i * 2 + 1).copied().unwrap_or(0) as usize,
                });
            }
        }
        let script_strings = res.strings(10240).unwrap_or_default();
        // Level intro dialog (slice 15): shown before
        // gameplay when the level header Q > 0.
        let mut intro_text = if parsed.level.q > 0 {
            script_strings.get(parsed.level.q as usize).cloned()
        } else {
            None
        };
        let mut script_engine = script::ScriptEngine::new(script_triggers, script_strings);
        let mut play_completed = false;
        for key in &keys {
            let mut attack_pressed = false;
            match key.as_str() {
                "left" => held = -1,
                "right" => held = 1,
                "unhold" => {
                    // Release the held direction (so the
                    // next left/right is a fresh press;
                    // used to push a grabbed box, slice 22).
                    held = 0;
                }
                "down" => {
                    // Hold-to-block (slice 18).
                    block_held = true;
                    println!("player blocks");
                }
                "release" => {
                    // Release Down: end the block.
                    block_held = false;
                    println!("player stops blocking");
                }
                "up" => {
                    if let Ok(anim_chunk) = res.chunk(28672) {
                        if let Ok(anim_set) = anim::load_anim_set(&anim_chunk) {
                            // Up while holding on a wall climbs
                            // up (slice 21); otherwise it jumps.
                            if pl.action == 33 {
                                if pl.climb_up(&anim_set, &flags) {
                                    println!("player climbs up (action 35)");
                                } else {
                                    println!("climb up blocked");
                                }
                            } else if pl.jump(&anim_set) {
                                println!("player jumps (action {})", pl.action);
                            }
                        }
                    }
                }
                "attack" => {
                    attack_pressed = true;
                    // No attack while blocking (slice 18).
                    if pl.blocking {
                        println!("blocked stance, attack ignored");
                        continue;
                    }
                    // `A()` (slice 17): Artemis with no
                    // magic left unequips instead.
                    if pl.is_artemis() && magic <= 0 {
                        pl.unequip_all();
                        println!("magic spent, back to blades");
                        continue;
                    }
                    // Phase 3: magic modes.
                    if pl.is_magic_mode3() {
                        if magic > player::Player::MODE3_COST
                            && pickups.damage_orb_count() < 10
                        {
                            // Fire: action 95, spawn
                            // type-3 orbs, 256 cost,
                            // reset to blades (A()).
                            if let Ok(anim_chunk) = res.chunk(28672) {
                                if let Ok(anim_set) =
                                    anim::load_anim_set(&anim_chunk)
                                {
                                    pl.attack(&anim_set);
                                }
                            }
                            let g_w = pl.magic_level_mode3.clamp(0, 4) as usize;
                            let value = player::Player::F_L[g_w];
                            let targets: Vec<usize> = enemy_states
                                .iter()
                                .enumerate()
                                .filter(|(_, st)| {
                                    !st.dead
                                        && st.x >= pl.cam_x - 64
                                        && st.x < pl.cam_x + 240 + 64
                                        && st.y >= pl.cam_y - 64
                                        && st.y < pl.cam_y + 320 + 64
                                })
                                .map(|(i, _)| i)
                                .collect();
                            pickups.spawn(
                                pl.ga,
                                pl.gb - 34 + 8,
                                pickups::DAMAGE,
                                value,
                                &mut orb_rng,
                            );
                            pickups.assign_damage_targets(&targets);
                            magic = (magic - player::Player::MODE3_COST).max(0);
                            pl.unequip_all();
                            println!(
                                "mode 3 cast: action 95, {} damage orbs (value {}), magic {}",
                                pickups.damage_orb_count(),
                                value,
                                magic
                            );
                        } else {
                            // Gate failed: blades + swing.
                            pl.unequip_all();
                            if let Ok(anim_chunk) = res.chunk(28672) {
                                if let Ok(anim_set) =
                                    anim::load_anim_set(&anim_chunk)
                                {
                                    pl.attack(&anim_set);
                                }
                            }
                            println!("mode 3 gate failed, back to blades");
                        }
                        continue;
                    }
                    if pl.is_magic_mode2() {
                        if magic > pl.mode2_start_gate() {
                            if let Ok(anim_chunk) = res.chunk(28672) {
                                if let Ok(anim_set) =
                                    anim::load_anim_set(&anim_chunk)
                                {
                                    pl.attack(&anim_set);
                                    println!("mode 2 cast: action 94 (channel)");
                                }
                            }
                        } else {
                            println!("mode 2 gate failed (magic {})", magic);
                        }
                        continue;
                    }
                    if let Ok(anim_chunk) = res.chunk(28672) {
                        if let Ok(anim_set) = anim::load_anim_set(&anim_chunk) {
                            if pl.attack(&anim_set) {
                                println!(
                                    "player attacks (action {} combo {} dmg {} buffered {})",
                                    pl.action,
                                    pl.combo_step,
                                    pl.swing_damage,
                                    pl.attack_buffered
                                );
                            }
                        }
                    }
                }
                "cycle" => {
                    // Left softkey / '*' (slice 17).
                    if pl.cycle_weapon() {
                        println!("weapon cycled to mode {}", pl.weapon_mode);
                    } else {
                        println!("weapon cycle blocked (attacking)");
                    }
                }
                "select" => {
                    // An intro dialog dismisses first.
                    if intro_text.is_some() {
                        intro_text = None;
                        println!("intro dismissed, gameplay starts");
                        continue;
                    }
                    // Advance the active script (Select key).
                    match script_engine.advance() {
                        script::ScriptEvent::Dialog(text) => {
                            println!("script dialog: {text}");
                        }
                        script::ScriptEvent::Ended => {
                            println!("script ended, gameplay resumes");
                        }
                        script::ScriptEvent::LevelComplete => {
                            play_completed = true;
                            println!(
                                "LEVEL COMPLETE: next level index {}",
                                gameplay::level_index(level_id) + 1
                            );
                        }
                        // A latch never fires from advance
                        // (only from check_trigger).
                        script::ScriptEvent::BossBarLatched => {}
                        script::ScriptEvent::None => {}
                    }
                    if let Some(mode) = script_engine.take_weapon_switch() {
                        pl.weapon_mode = mode;
                        println!("script weapon switch: mode {mode}");
                    }
                }
                "tick" => {
                    // A showing intro or script dialog
                    // freezes the world.
                    if intro_text.is_some() || script_engine.active() {
                        continue;
                    }
                    if combo_timer_ms > 0 {
                        combo_timer_ms -= 40;
                        if combo_timer_ms <= 0 {
                            combo_hits = 0;
                        }
                    }
                    // Climb turn (slice 21): held Left/Right
                    // opposite to the facing while holding
                    // (33) flips the player (44).
                    if pl.action == 33 && held != 0 {
                        pl.climb_turn(&player_anim, held);
                    }
                    // Block stance (slice 18): Down held
                    // starts it, release ends it. Down
                    // held while holding on a wall
                    // (slice 21) climbs down (34) or
                    // drops (26) instead.
                    if block_held {
                        // Lever pull (slice 23): Down
                        // pressed near a type-34 lever
                        // toggles its doors.
                        if !prev_block_held && (pl.action == 0 || pl.action == 9) {
                            if let Some(lx) = doors::check_lever(&mut play_levers, &mut play_doors, &mut flags, pl.ga, pl.gb) {
                                pl.ga = lx;
                                println!("lever pulled, doors toggled");
                            }
                        }
                        if pl.action == 37 {
                            // Drop from a ledge hang
                            // (slice 23): Down lets go.
                            pl.drop_from_hang(&player_anim);
                        } else if pl.action == 33 {
                            pl.climb_down(&player_anim, &flags);
                        } else if pl.drop_through(&player_anim, &flags) {
                            // Drop through a one-way
                            // platform (slice 23).
                        } else if !pl.blocking || pl.action == 0 {
                            pl.start_block(&player_anim);
                        }
                    } else if pl.blocking {
                        pl.stop_block(&player_anim);
                    }
                    prev_block_held = block_held;
                    for st in &mut enemy_states {
                        if let Ok(anim_chunk) = res.chunk(st.enemy.anim_id) {
                            if let Ok(anim_set) = anim::load_anim_set(&anim_chunk) {
                                let dmg = st.tick(&anim_set, &flags, pl.ga, pl.gb);
                                if dmg > 0 {
                                    if pl.blocking {
                                        // Blocked hit: no damage,
                                        // block-impact reaction.
                                        pl.facing_right = !st.facing_right;
                                        pl.action = 97;
                                        pl.slot = anim::AnimSlot::new(&player_anim, 97);
                                        println!(
                                            "enemy {} hit blocked (player health {}/{})",
                                            st.enemy.index, pl.health, pl.max_health
                                        );
                                    } else {
                                        pl.health -= dmg;
                                        println!(
                                            "enemy {} hits player for {} (player health {}/{})",
                                            st.enemy.index, dmg, pl.health, pl.max_health
                                        );
                                    }
                                }
                            }
                        }
                    }
                    // Player attack hit: once per swing, at frame >= 2.
                    if (player::Player::is_ground_attack(pl.action)
                        || player::Player::is_air_attack(pl.action))
                        && pl.slot.frame >= 2
                        && !pl.attack_hit_done
                    {
                        pl.attack_hit_done = true;
                        let swing_damage = pl.swing_damage;
                        for st in &mut enemy_states {
                            if st.dead || st.dying {
                                continue;
                            }
                            let in_front = if pl.facing_right {
                                st.x - pl.ga >= 0 && st.x - pl.ga <= 32
                            } else {
                                pl.ga - st.x >= 0 && pl.ga - st.x <= 32
                            };
                            if in_front && (st.y - pl.gb).abs() < 16 {
                                let killed = if let Ok(anim_chunk) = res.chunk(st.enemy.anim_id) {
                                    if let Ok(anim_set) = anim::load_anim_set(&anim_chunk) {
                                        st.apply_damage(&anim_set, swing_damage)
                                    } else {
                                        false
                                    }
                                } else {
                                    false
                                };
                                combo_hits += 1;
                                combo_timer_ms = 2000;
                                // Artemis drains magic per hit
                                // (slice 17): `fJ * 3`, clamped at 0.
                                if pl.is_artemis() {
                                    magic = (magic - swing_damage * 3).max(0);
                                }
                                println!(
                                    "player hits enemy {} for {} (health {}){}",
                                    st.enemy.index,
                                    swing_damage,
                                    st.health,
                                    if killed { " - DIED" } else { "" }
                                );
                                if killed {
                                    let drop_kind = match st.enemy.kind {
                                        12 => pickups::MAGIC,
                                        _ => pickups::GOLD,
                                    };
                                    pickups.spawn(
                                        st.x,
                                        st.y - 16,
                                        drop_kind,
                                        st.enemy.em,
                                        &mut orb_rng,
                                    );
                                }
                            }
                        }
                    }
                    // Box push/pull start (slice 22): a
                    // fresh Left/Right press while grabbing
                    // (21) starts the push (22) or pull
                    // (23) and runs u() at once; failure
                    // returns to the grab pose.
                    let was_grabbing = pl.action == 21;
                    if was_grabbing && held != 0 && held != prev_held {
                        if let Some(gd) = pl.grabbed_box {
                            let push = (held > 0) == pl.facing_right;
                            let action = if push { 22 } else { 23 };
                            pl.push_pull = if push { 1 } else { 0 };
                            pl.action = action;
                            pl.slot = anim::AnimSlot::new(&player_anim, action);
                            let (gw, gx) = (pl.tile_x(), pl.walk_tile_y());
                            let facing = pl.facing_right;
                            if boxes::try_push(
                                &mut play_boxes,
                                gd,
                                &mut flags,
                                facing,
                                action,
                                gw,
                                gx,
                                &box_anim,
                            ) {
                                println!(
                                    "player {}s box {} (action {})",
                                    if push { "push" } else { "pull" },
                                    gd,
                                    action
                                );
                            } else {
                                pl.action = 21;
                                pl.slot = anim::AnimSlot::new(&player_anim, 21);
                                println!("box push failed, still grabbing");
                            }
                        }
                    }
                    prev_held = held;
                    let step = pl.tick(
                        &player_anim,
                        &flags,
                        parsed.level.pixel_width() as i32,
                        parsed.level.pixel_height() as i32,
                        held,
                    );
                    // Box slots + slide, then the grab
                    // check (after the walk stopped at
                    // the box face).
                    for b in &mut play_boxes {
                        b.advance(&box_anim);
                    }
                    if boxes::check_grab(&play_boxes, &flags, &mut pl, &player_anim, held) {
                        println!("player grabs box {}", pl.grabbed_box.unwrap());
                    }
                    // Type-9 switch (slice 23): standing
                    // on a flag-256 tile toggles doors.
                    if doors::check_switch(&play_switches, &mut play_doors, &mut flags, pl.tile_x(), pl.gb / 16) {
                        println!("switch triggered, doors toggled");
                    }
                    // Pickups home on the player's new position.
                    let enemy_pos: Vec<(i32, i32, bool)> = enemy_states
                        .iter()
                        .map(|st| (st.x, st.y, !st.dead))
                        .collect();
                    // Mode-2 channel (Phase 3): while in
                    // action 94, drain 12 magic per tick
                    // while attack is pressed; end the
                    // cast when attack is released or
                    // magic runs out.
                    if pl.action == 94 {
                        if attack_pressed && magic > 0 {
                            magic = (magic
                                - player::Player::MODE2_DRAIN_PER_TICK)
                                .max(0);
                        } else {
                            if let Ok(anim_chunk) = res.chunk(28672) {
                                if let Ok(anim_set) =
                                    anim::load_anim_set(&anim_chunk)
                                {
                                    pl.end_magic_cast(&anim_set);
                                    println!(
                                        "mode 2 cast ended ({})",
                                        if magic <= 0 {
                                            "magic spent"
                                        } else {
                                            "attack released"
                                        }
                                    );
                                }
                            }
                        }
                    }
                    for ev in pickups.update((pl.ga, pl.gb), &enemy_pos) {
                        match ev {
                            pickups::PickupEvent::Collected { kind, value } => match kind {
                                pickups::GOLD => gold += value,
                                pickups::HEALTH => {
                                    pl.health = (pl.health + value).min(pl.max_health)
                                }
                                pickups::MAGIC => magic = (magic + value).min(1024),
                                _ => {}
                            },
                            pickups::PickupEvent::HitEnemy { index, value } => {
                                // Phase 3: a damage orb hit
                                // its target enemy.
                                if let Some(st) = enemy_states.get_mut(index) {
                                    if st.dead || st.dying {
                                        continue;
                                    }
                                    let killed =
                                        if let Ok(anim_chunk) =
                                            res.chunk(st.enemy.anim_id)
                                        {
                                            if let Ok(anim_set) =
                                                anim::load_anim_set(&anim_chunk)
                                            {
                                                st.apply_damage(&anim_set, value)
                                            } else {
                                                false
                                            }
                                        } else {
                                            false
                                        };
                                    println!(
                                        "damage orb hits enemy {} for {} (health {}){}",
                                        st.enemy.index,
                                        value,
                                        st.health,
                                        if killed { " - DIED" } else { "" }
                                    );
                                    if killed {
                                        let drop_kind = match st.enemy.kind {
                                            12 => pickups::MAGIC,
                                            _ => pickups::GOLD,
                                        };
                                        pickups.spawn(
                                            st.x,
                                            st.y - 16,
                                            drop_kind,
                                            st.enemy.em,
                                            &mut orb_rng,
                                        );
                                    }
                                }
                            }
                        }
                    }
                    // Weakened-marker pulse (t(132)).
                    if let Some((bar_anim, _, bar_slot)) = &mut bar_state {
                        bar_slot.advance(bar_anim, 40);
                    }
                    // Boss-bar pulse (Phase 2, t(131)):
                    // the slot advances each tick while
                    // the `@` latch holds.
                    if script_engine.boss_latched() {
                        if let Some((boss_anim, _, boss_slot)) = &mut boss_bar_state {
                            boss_slot.advance(boss_anim, 40);
                        }
                    }
                    // Script trigger check on the new tile.
                    if !play_completed {
                        match script_engine.check_trigger(pl.ga / 16, pl.gb / 16) {
                            script::ScriptEvent::Dialog(text) => {
                                println!("script dialog: {text}");
                            }
                            script::ScriptEvent::LevelComplete => {
                                play_completed = true;
                                println!(
                                    "LEVEL COMPLETE: next level index {}",
                                    gameplay::level_index(level_id) + 1
                                );
                            }
                            script::ScriptEvent::BossBarLatched => {
                                // Phase 2: (re)set the boss-bar
                                // slot (131) to the latched
                                // action `aT`.
                                if let Some((boss_anim, _, boss_slot)) = &mut boss_bar_state {
                                    let action = script_engine.boss_action() as usize;
                                    *boss_slot = anim::AnimSlot::new(boss_anim, action);
                                }
                                println!(
                                    "boss bar latched: action={} facing_right={}",
                                    script_engine.boss_action(),
                                    script_engine.boss_facing_right()
                                );
                            }
                            _ => {}
                        }
                        if let Some(mode) = script_engine.take_weapon_switch() {
                            pl.weapon_mode = mode;
                            println!("script weapon switch: mode {mode}");
                        }
                    }
                    println!(
                        "tick: ga={} gb={} action={} visual={} cam=({},{}) step={}",
                        pl.ga, pl.gb, pl.action, pl.slot.visual, pl.cam_x, pl.cam_y, step
                    );
                }
                _ => {}
            }
        }
        // A final tick with no input settles idle, unless keys ended on tick
        // or a script dialog is showing (frozen world).
        if keys.last().map(|k| k.as_str()) != Some("tick")
            && !script_engine.active()
            && intro_text.is_none()
        {
            pl.tick(
                &player_anim,
                &flags,
                parsed.level.pixel_width() as i32,
                parsed.level.pixel_height() as i32,
                0,
            );
        }
        let tileset = res.image(2058, None).expect("tileset 2058");
        let mut fb = midp::Framebuffer::game();
        level::draw_view(&parsed.level, &tileset, &mut fb, pl.cam_x, pl.cam_y);
        let anim_chunk = res.chunk(3073).expect("anim set 3073");
        let anim_set = anim::load_anim_set(&anim_chunk).expect("load anim set");
        let sprite_set = res.sprite_set(3076, None).expect("sprite set 3076");
        let g = midp::Graphics::default();
        if let Some(group) = parsed.group(23) {
            anim::draw_static_sprites(
                &g,
                &mut fb,
                &sprite_set,
                &anim_set,
                &group.positions,
                &group.extra,
                pl.cam_x,
                pl.cam_y,
            );
        }
        // Boxes (slice 22): after the statics, before
        // the enemies, at their px position.
        for b in &play_boxes {
            b.draw(&g, &mut fb, &box_anim, &box_sprites, pl.cam_x, pl.cam_y);
        }
        let (drawn, skipped) = enemies::draw_enemy_states(&g, &mut fb, &mut res, &enemy_states, pl.cam_x, pl.cam_y);
        println!("enemies: {} drawn, {} skipped, {} total", drawn, skipped, enemy_states.len());
        if let Some((bar_anim, bar_sprites, bar_slot)) = &bar_state {
            enemies::draw_weakened_markers(
                &g,
                &mut fb,
                bar_anim,
                bar_sprites,
                bar_slot.visual,
                &enemy_states,
                pl.cam_x,
                pl.cam_y,
            );
        }
        // Boss bar (Phase 2): screen (12, 295) when
        // the `@` latch holds, the player's facing
        // equals `aS`, and the player is in the
        // viewport.
        if script_engine.boss_latched()
            && script_engine.boss_facing_right() == pl.facing_right
            && pl.ga >= pl.cam_x
            && pl.ga < pl.cam_x + 240
            && pl.gb >= pl.cam_y
            && pl.gb < pl.cam_y + 320
        {
            if let Some((boss_anim, boss_sprites, boss_slot)) = &boss_bar_state {
                if let Ok(frames) = anim::frames_for_timeline_frame(
                    boss_anim,
                    script_engine.boss_action() as usize,
                    boss_slot.visual,
                    0,
                ) {
                    for fr in frames {
                        if let Some(frame) = boss_sprites.frame(fr.sprite_idx) {
                            midp::draw_sprite_frame(
                                &g,
                                &mut fb,
                                &boss_sprites.image,
                                frame,
                                12 + fr.dx,
                                295 + fr.dy,
                                fr.transform,
                            );
                        }
                    }
                }
                println!(
                    "boss bar: action={} visual={}",
                    script_engine.boss_action(),
                    boss_slot.visual
                );
            }
        }
        for st in &enemy_states {
            println!(
                "  enemy {}: type {} at ({},{}) facing_right={} action={} visual={} health={}/{} ef={}",
                st.enemy.index, st.enemy.kind, st.x, st.y, st.facing_right, st.action, st.slot.visual, st.health, st.max_health, st.ef
            );
        }
        for (i, o) in pickups.orbs.iter().enumerate() {
            println!(
                "  orb {}: kind={} value={} at ({},{}) v=({},{}) age={}",
                i, o.kind, o.value, o.x, o.y, o.vx, o.vy, o.age_ms
            );
        }
        for b in &play_boxes {
            println!(
                "  box {}: eN={} at ({},{}) tile ({},{}) footprint {}x{} slot_action={}",
                b.index, b.e_n, b.x, b.y, b.e_q, b.e_r, b.e_u, b.e_v, b.slot.action
            );
        }
        let player_transform = if pl.facing_right { 0 } else { 2 };
        let frames = anim::frames_for_timeline_frame(&player_anim, pl.action, pl.slot.visual, player_transform)
            .expect("player frames");
        for fr in frames {
            if let Some(frame) = player_sprites.frame(fr.sprite_idx) {
                midp::draw_sprite_frame(
                    &g,
                    &mut fb,
                    &player_sprites.image,
                    frame,
                    pl.ga + fr.dx - pl.cam_x,
                    pl.gb + fr.dy - pl.cam_y,
                    fr.transform,
                );
            }
        }
        pickups.draw(&g, &mut fb, &mut res, pl.cam_x, pl.cam_y);
        // HUD (3e): health/magic bars, gold, bottom bar.
        let font_set = res.sprite_set(1031, None).expect("font sprite set 1031");
        let font = font::Font::from_sprite_set(font_set);
        let hud_state = hud::HudState {
            health: pl.health,
            max_health: pl.max_health,
            magic,
            max_magic: 1024,
            gold,
            combo_hits,
        };
        hud::draw_hud(&g, &mut fb, &mut res, &font, &hud_state);
        // Script dialog on top, if one is showing.
        if let Some(text) = script_engine.dialog_text() {
            gameplay::draw_dialog_box(&mut fb, &font, text);
        }
        // Level intro dialog on top of that.
        if let Some(text) = &intro_text {
            gameplay::draw_dialog_box(&mut fb, &font, text);
        }
        let rgba = fb.to_rgba();
        fs::write(
            format!("{prefix}.png"),
            png::encode_png_rgba(fb.width, fb.height, &rgba),
        )
        .expect("write play frame");
        println!(
            "player final: ga={} gb={} action={} visual={} cam=({},{}) health={}/{} gold={} orbs={} completed={} intro={} wrote {prefix}.png",
            pl.ga, pl.gb, pl.action, pl.slot.visual, pl.cam_x, pl.cam_y, pl.health, pl.max_health, gold, pickups.orbs.len(), play_completed, intro_text.is_some()
        );
        return;
    }
    if args.get(2).map(|s| s.as_str()) == Some("--level") {
        // --level <level-id> [cam_x cam_y] [out-prefix]
        // Parses the level tile map, draws the 240x320 first-screen
        // viewport (camera 0,0 by default) and the full level image.
        let level_id: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(9217);
        let cam_x: i32 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(0);
        let cam_y: i32 = args.get(5).and_then(|s| s.parse().ok()).unwrap_or(0);
        let prefix = args.get(6).map(|s| s.as_str()).unwrap_or("../level-frame");
        let mut res = resources::Resources::new(&target);
        let chunk = res.chunk(level_id).expect("level chunk");
        let lvl = level::parse_level(level_id, &chunk).expect("parse level");
        println!(
            "level {}: {}x{} tiles, {}x{} px, palette {}, q={}, hs={}",
            lvl.id,
            lvl.width_tiles,
            lvl.height_tiles,
            lvl.pixel_width(),
            lvl.pixel_height(),
            lvl.palette.len(),
            lvl.q,
            lvl.hs
        );
        // Tileset 2058, base palette (bC = 0 -> bG[0] = -1, no key).
        let tileset = res.image(2058, None).expect("tileset 2058");
        println!("tileset {}x{}", tileset.width, tileset.height);
        let mut fb = midp::Framebuffer::game();
        level::draw_view(&lvl, &tileset, &mut fb, cam_x, cam_y);
        let rgba = fb.to_rgba();
        fs::write(
            format!("{prefix}-view.png"),
            png::encode_png_rgba(fb.width, fb.height, &rgba),
        )
        .expect("write level view");
        let full = level::draw_full(&lvl, &tileset);
        let mut rgba_full = Vec::with_capacity(full.width * full.height * 4);
        for &p in &full.pixels {
            rgba_full.push(((p >> 16) & 0xFF) as u8);
            rgba_full.push(((p >> 8) & 0xFF) as u8);
            rgba_full.push((p & 0xFF) as u8);
            rgba_full.push(((p >> 24) & 0xFF) as u8);
        }
        fs::write(
            format!("{prefix}-full.png"),
            png::encode_png_rgba(full.width, full.height, &rgba_full),
        )
        .expect("write level full");
        println!("wrote {prefix}-view.png and {prefix}-full.png");
        return;
    }
    if args.get(2).map(|s| s.as_str()) == Some("--window") {
        // --window <out-prefix> [factor] [keys]
        // Real-time version of --game: same scripted keys, but
        // each tick waits for its 40 ms frame deadline, and the
        // final frame is written at integer scale (default 3x).
        let prefix = args.get(3).map(|s| s.as_str()).unwrap_or("../window-frame");
        let factor: usize = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(3);
        let keys: Vec<String> = args
            .get(5)
            .map(|k| k.split(',').map(|x| x.trim().to_string()).collect())
            .unwrap_or_default();
        let mut g = game::Game::new(&target);
        if let Some(dir) = args.get(6) {
            g.set_save_dir(dir);
        }
        g.boot().expect("boot");
        let mut timer = window::FrameTimer::new(window::FRAME_MS);
        let start = std::time::Instant::now();
        for key in &keys {
            let input = match key.as_str() {
                "up" => game::Input::Up,
                "down" => game::Input::Down,
                "left" => game::Input::Left,
                "right" => game::Input::Right,
                "select" => game::Input::Select,
                "exit" => game::Input::Exit,
                "attack" => game::Input::Attack,
                "tick" => {
                    g.tick(40);
                    timer.wait();
                    continue;
                }
                other => {
                    eprintln!("unknown key {other:?}, skipping");
                    continue;
                }
            };
            g.update(Some(input));
            timer.wait();
            // Settle menu camera transitions in real time too.
            for _ in 0..200 {
                match g.state {
                    game::State::MainMenu if g.cam_x == 0 && g.cam_y == 0 => break,
                    game::State::Playing | game::State::Exited | game::State::HelpAbout => break,
                    _ => {
                        g.tick(40);
                        timer.wait();
                    }
                }
            }
        }
        let mut fb = midp::Framebuffer::game();
        g.draw(&mut fb);
        let scaled = window::scale_nearest(&fb, factor);
        let rgba = scaled.to_rgba();
        fs::write(
            format!("{prefix}.png"),
            png::encode_png_rgba(scaled.width, scaled.height, &rgba),
        )
        .expect("write window frame");
        println!(
            "window: state={:?} frames={} elapsed_ms={} max_drift_ms={} size={}x{} wrote {prefix}.png",
            g.state,
            timer.frames,
            start.elapsed().as_millis(),
            timer.max_drift_ms,
            scaled.width,
            scaled.height
        );
        return;
    }
    if args.get(2).map(|s| s.as_str()) == Some("--game") {
        // --game <out-prefix> [comma-separated keys: up,down,left,right,select,exit]
        // Boots (slice 1), draws LanguageSelect, applies the keys, draws
        // the resulting state. Example:
        //   --game ../game-frame down,down,select
        let prefix = args.get(3).map(|s| s.as_str()).unwrap_or("../game-frame");
        let mut g = game::Game::new(&target);
        if let Some(dir) = args.get(5) {
            g.set_save_dir(dir);
        }
        g.boot().expect("boot");
        let mut fb = midp::Framebuffer::game();
        g.draw(&mut fb);
        let rgba = fb.to_rgba();
        fs::write(
            format!("{prefix}-language.png"),
            png::encode_png_rgba(fb.width, fb.height, &rgba),
        )
        .expect("write language frame");
        println!("state={:?} lang_index={} lang_name={}", g.state, g.lang_index, g.string(game::LANG_NAME_INDICES[g.lang_index]));
        if let Some(keys) = args.get(4) {
            for key in keys.split(',') {
                let input = match key.trim() {
                    "up" => game::Input::Up,
                    "down" => game::Input::Down,
                    "left" => game::Input::Left,
                    "right" => game::Input::Right,
                    "select" => game::Input::Select,
                    "exit" => game::Input::Exit,
                    "attack" => game::Input::Attack,
                    "tick" => {
                        // One 40 ms frame: camera + transition, no input.
                        g.tick(40);
                        continue;
                    }
                    other => {
                        eprintln!("unknown key {other:?}, skipping");
                        continue;
                    }
                };
                g.update(Some(input));
                // Settle any camera transition this input started, so
                // the next key lands in the state it is meant for
                // (audio prompt -> main menu is ~70 frames).
                for _ in 0..200 {
                    match g.state {
                        game::State::Playing | game::State::Exited | game::State::HelpAbout => break,
                        game::State::MainMenu if !g.cam_moving && g.cam_x == 0 && g.cam_y == 0 => break,
                        _ => g.tick(40),
                    }
                }
            }
        }
        // Let any in-flight camera transition settle (audio prompt ->
        // main menu is ~30 frames to centre + ~10 countdown + ~30 back).
        for _ in 0..200 {
            match g.state {
                game::State::Playing | game::State::Exited | game::State::HelpAbout => break,
                game::State::MainMenu if !g.cam_moving && g.cam_x == 0 && g.cam_y == 0 => break,
                _ => g.tick(40),
            }
        }
        let mut fb2 = midp::Framebuffer::game();
        g.draw(&mut fb2);
        let rgba2 = fb2.to_rgba();
        fs::write(
            format!("{prefix}-after.png"),
            png::encode_png_rgba(fb2.width, fb2.height, &rgba2),
        )
        .expect("write after frame");
        println!(
            "after: state={:?} lang_index={} strings[0]={:?} audio={:?}",
            g.state,
            g.lang_index,
            g.string(0),
            g.audio_enabled
        );
        for line in &g.dialog_lines {
            println!("  dialog: {:?}", line.text);
        }
        return;
    }
    if args.get(2).map(|s| s.as_str()) == Some("--render-test") {
        let out = args.get(3).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("test-frame.png"));
        render_test(&target, &out);
        return;
    }
    if args.get(2).map(|s| s.as_str()) == Some("--decode-all") {
        // Decode every flag-253 PNG in every pack; print pack/chunk/dims.
        // Used to cross-check the Rust PNG decoder against frames.json.
        let mut entries: Vec<PathBuf> = fs::read_dir(&target)
            .expect("read directory")
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.file_name()
                    .map(|n| n.to_string_lossy().starts_with("RP"))
                    .unwrap_or(false)
            })
            .collect();
        entries.sort();
        let mut total = 0;
        for p in entries {
            let pack = load_pack(&target, p.file_name().unwrap().to_str().unwrap());
            for (i, &flag) in pack.flags.iter().enumerate() {
                if flag == 253 {
                    let img = Image::from_png(pack.chunk(i).unwrap())
                        .unwrap_or_else(|e| panic!("{} chunk {i}: {e}", p.display()));
                    println!("{} {} {}x{}", p.file_name().unwrap().to_string_lossy(), i, img.width, img.height);
                    total += 1;
                }
            }
        }
        println!("decoded {total} PNG sheets");
        return;
    }
    if args.get(2).map(|s| s.as_str()) == Some("--defs-all") {
        let mut entries: Vec<PathBuf> = fs::read_dir(&target)
            .expect("read directory")
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.file_name()
                    .map(|n| n.to_string_lossy().starts_with("RP"))
                    .unwrap_or(false)
            })
            .collect();
        entries.sort();
        let mut defs = 0;
        let mut frames = 0;
        for p in entries {
            let pack = load_pack(&target, p.file_name().unwrap().to_str().unwrap());
            for (i, &flag) in pack.flags.iter().enumerate() {
                if flag == 254 {
                    let def = sprite::parse_sprite_def(pack.chunk(i).unwrap())
                        .unwrap_or_else(|e| panic!("{} chunk {i}: {e}", p.display()));
                    println!("{} {} image={} frames={}", p.file_name().unwrap().to_string_lossy(), i, def.image_chunk, def.frames.len());
                    defs += 1;
                    frames += def.frames.len();
                }
            }
        }
        println!("parsed {defs} defs, {frames} frames total");
        return;
    }
    let mut entries: Vec<PathBuf> = fs::read_dir(&target)
        .expect("read directory")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().starts_with("RP"))
                .unwrap_or(false)
        })
        .collect();
    entries.sort();
    for p in entries {
        inspect_rp(&p);
    }
    let (file, chunk) = rp::split_resource_id(10240);
    assert_eq!((file, chunk), (10, 0));
    println!("resource id 10240 -> RP{file} chunk {chunk} (English strings) — ok");
    println!("Native framebuffer for the port: 240x320 (hardcoded in e.java).");
}
