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

mod crc;
mod font;
mod game;
mod menu;
mod midp;
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
    if args.get(2).map(|s| s.as_str()) == Some("--game") {
        // --game <out-prefix> [comma-separated keys: up,down,left,right,select,exit]
        // Boots (slice 1), draws LanguageSelect, applies the keys, draws
        // the resulting state. Example:
        //   --game ../game-frame down,down,select
        let prefix = args.get(3).map(|s| s.as_str()).unwrap_or("../game-frame");
        let mut g = game::Game::new(&target);
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
                        game::State::MainMenu if g.cam_x == 0 && g.cam_y == 0 => break,
                        game::State::Playing | game::State::Exited => break,
                        _ => g.tick(40),
                    }
                    if g.state == game::State::MainMenu && g.cam_x == 0 && g.cam_y == 0 {
                        break;
                    }
                }
            }
        }
        // Let any in-flight camera transition settle (audio prompt ->
        // main menu is ~30 frames to centre + ~10 countdown + ~30 back).
        for _ in 0..200 {
            match g.state {
                game::State::MainMenu if g.cam_x == 0 && g.cam_y == 0 => break,
                game::State::Playing | game::State::Exited => break,
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
