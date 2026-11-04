//! MIDP shim for the PC port: Image, Graphics, sprite drawing (`c.a`),
//! RecordStore, and the game-loop timing from `a.run()`.
//!
//! The game logic (`e.java`, 240x320, integer coordinates) runs against
//! this shim instead of `javax.microedition.lcdui.*`. The shim is std-only
//! and backend-free: a `Framebuffer` is just pixels, so the port's window
//! layer (SDL/winit on Windows, a PNG dump here) sits on top of it.
//!
//! Pixels are ARGB (`0xAARRGGBB`). A pixel with alpha 0 is transparent and
//! is never drawn. `Graphics` clip coordinates are absolute framebuffer
//! coordinates; `translate` shifts drawing coordinates. This matches how
//! the game's renderer (`b.java`) uses setClip + manually adjusted
//! destinations; it is a documented simplification, not a claim about
//! every MIDP edge case.
//!
//! MIDP drawRegion transforms (only 0-3 occur for sprite frames via `c`,
//! but all 8 are implemented):
//!   0 TRANS_NONE, 1 TRANS_MIRROR_ROT180 (flip Y), 2 TRANS_MIRROR (flip X),
//!   3 TRANS_ROT180 (flip both), 4 TRANS_MIRROR_ROT270,
//!   5 TRANS_ROT90, 6 TRANS_ROT270, 7 TRANS_MIRROR_ROT90.
//! Anchor 20 = TOP (16) | LEFT (4), the anchor the game passes.

use crate::png::decode_png;
use crate::sprite::Frame;
use std::fs;
use std::path::{Path, PathBuf};

pub const GAME_WIDTH: usize = 240;
pub const GAME_HEIGHT: usize = 320;

/// Game-loop timing from `a.run()`: minimum frame time 10 ms, delta
/// clamped to at most 100 ms. `a.m` = 100, `a.l` = 10 in the original.
pub const MIN_FRAME_MS: u128 = 10;
pub const MAX_DELTA_MS: u128 = 100;

#[derive(Debug, Clone)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    /// ARGB pixels, row-major.
    pub pixels: Vec<u32>,
}

impl Image {
    pub fn from_png(png_bytes: &[u8]) -> Result<Image, String> {
        let d = decode_png(png_bytes).map_err(|e| e.to_string())?;
        Ok(Image {
            width: d.width,
            height: d.height,
            pixels: d.pixels,
        })
    }
    pub fn from_argb(width: usize, height: usize, pixels: Vec<u32>) -> Result<Image, String> {
        if pixels.len() != width * height {
            return Err(format!(
                "image size mismatch: {} pixels for {}x{}",
                pixels.len(),
                width,
                height
            ));
        }
        Ok(Image {
            width,
            height,
            pixels,
        })
    }
    pub fn pixel(&self, x: usize, y: usize) -> u32 {
        self.pixels[y * self.width + x]
    }
}

#[derive(Debug, Clone)]
pub struct Framebuffer {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u32>,
}

impl Framebuffer {
    pub fn new(width: usize, height: usize) -> Framebuffer {
        Framebuffer {
            width,
            height,
            pixels: vec![0; width * height],
        }
    }
    pub fn game() -> Framebuffer {
        Framebuffer::new(GAME_WIDTH, GAME_HEIGHT)
    }
    pub fn clear(&mut self, argb: u32) {
        self.pixels.fill(argb);
    }
    /// Export as RGBA bytes for the PNG encoder.
    pub fn to_rgba(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.width * self.height * 4);
        for &p in &self.pixels {
            out.push(((p >> 16) & 0xFF) as u8);
            out.push(((p >> 8) & 0xFF) as u8);
            out.push((p & 0xFF) as u8);
            out.push(((p >> 24) & 0xFF) as u8);
        }
        out
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Graphics {
    pub clip_x: i32,
    pub clip_y: i32,
    pub clip_w: i32,
    pub clip_h: i32,
    pub translate_x: i32,
    pub translate_y: i32,
    pub color: u32, // ARGB
}

impl Default for Graphics {
    fn default() -> Self {
        Graphics {
            clip_x: 0,
            clip_y: 0,
            clip_w: GAME_WIDTH as i32,
            clip_h: GAME_HEIGHT as i32,
            translate_x: 0,
            translate_y: 0,
            color: 0xFF00_0000,
        }
    }
}

impl Graphics {
    pub fn set_color_rgb(&mut self, r: u8, g: u8, b: u8) {
        self.color = 0xFF00_0000 | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
    }
    pub fn set_clip(&mut self, x: i32, y: i32, w: i32, h: i32) {
        self.clip_x = x;
        self.clip_y = y;
        self.clip_w = w;
        self.clip_h = h;
    }
    pub fn set_clip_full(&mut self, width: i32, height: i32) {
        self.set_clip(0, 0, width, height);
    }
    pub fn translate(&mut self, dx: i32, dy: i32) {
        self.translate_x += dx;
        self.translate_y += dy;
    }

    fn put(&self, fb: &mut Framebuffer, x: i32, y: i32, argb: u32) {
        if (argb >> 24) == 0 {
            return;
        }
        if x < self.clip_x
            || y < self.clip_y
            || x >= self.clip_x + self.clip_w
            || y >= self.clip_y + self.clip_h
        {
            return;
        }
        if x < 0 || y < 0 || x as usize >= fb.width || y as usize >= fb.height {
            return;
        }
        fb.pixels[y as usize * fb.width + x as usize] = argb;
    }

    pub fn fill_rect(&self, fb: &mut Framebuffer, x: i32, y: i32, w: i32, h: i32) {
        if w <= 0 || h <= 0 {
            return;
        }
        let x0 = x + self.translate_x;
        let y0 = y + self.translate_y;
        for yy in y0..y0 + h {
            for xx in x0..x0 + w {
                self.put(fb, xx, yy, self.color);
            }
        }
    }

    /// MIDP drawRGB: `rgb` holds ARGB ints; `process_alpha` false means
    /// alpha is ignored (drawn opaque), true means transparent pixels are
    /// skipped (no blending — the game does not rely on blending here).
    pub fn draw_rgb(
        &self,
        fb: &mut Framebuffer,
        rgb: &[u32],
        offset: usize,
        scanlength: usize,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        process_alpha: bool,
    ) {
        if w <= 0 || h <= 0 {
            return;
        }
        let x0 = x + self.translate_x;
        let y0 = y + self.translate_y;
        for row in 0..h {
            for col in 0..w {
                let idx = offset + row as usize * scanlength + col as usize;
                if let Some(&p) = rgb.get(idx) {
                    let p = if process_alpha {
                        p
                    } else {
                        p | 0xFF00_0000
                    };
                    self.put(fb, x0 + col, y0 + row, p);
                }
            }
        }
    }

    /// MIDP drawRegion with anchor. Anchor bits: HCENTER=1, VCENTER=2,
    /// LEFT=4, RIGHT=8, TOP=16, BOTTOM=32. The game passes 20 (TOP|LEFT).
    pub fn draw_region(
        &self,
        fb: &mut Framebuffer,
        img: &Image,
        src_x: i32,
        src_y: i32,
        src_w: i32,
        src_h: i32,
        transform: i32,
        dest_x: i32,
        dest_y: i32,
        anchor: i32,
    ) {
        if src_w <= 0 || src_h <= 0 {
            return;
        }
        // Destination size after rotation: 90/270-degree transforms swap w/h.
        let (dw, dh) = match transform {
            4 | 5 | 6 | 7 => (src_h, src_w),
            _ => (src_w, src_h),
        };
        let mut dx = dest_x + self.translate_x;
        let mut dy = dest_y + self.translate_y;
        if anchor & 8 != 0 {
            dx -= dw;
        } else if anchor & 1 != 0 {
            dx -= dw / 2;
        }
        if anchor & 32 != 0 {
            dy -= dh;
        } else if anchor & 2 != 0 {
            dy -= dh / 2;
        }
        for row in 0..src_h {
            for col in 0..src_w {
                let sx = src_x + col;
                let sy = src_y + row;
                if sx < 0 || sy < 0 || sx as usize >= img.width || sy as usize >= img.height {
                    continue;
                }
                let p = img.pixel(sx as usize, sy as usize);
                // Map (col, row) in source space to destination offset.
                let (ox, oy) = match transform {
                    0 => (col, row),
                    1 => (col, src_h - 1 - row), // mirror_rot180: flip Y
                    2 => (src_w - 1 - col, row), // mirror: flip X
                    3 => (src_w - 1 - col, src_h - 1 - row), // rot180
                    4 => (row, col), // mirror_rot270: mirror, then rot270
                    5 => (src_h - 1 - row, col), // rot90
                    6 => (row, src_w - 1 - col), // rot270
                    7 => (src_h - 1 - row, src_w - 1 - col), // mirror_rot90
                    _ => (col, row),
                };
                // Rotation transforms 4-7 follow the MIDP transform table
                // (mirror/rotate compositions); the game's sprite path
                // itself only uses 0-3, which are the flip combinations.
                self.put(fb, dx + ox, dy + oy, p);
            }
        }
    }
}

/// Sprite frame drawing, exactly `c.a(Graphics, x, y, transform)` in
/// `decompiled/c.java`: anchor flips, source clipping to the image, then
/// drawRegion with the same transform. Transform bits: 2 = flip X, 1 = flip Y.
pub fn draw_sprite_frame(
    g: &Graphics,
    fb: &mut Framebuffer,
    img: &Image,
    frame: &Frame,
    x: i32,
    y: i32,
    transform: i32,
) {
    let mut dx = x + if transform & 2 == 0 {
        frame.anchor_x
    } else {
        -(frame.anchor_x + frame.w)
    };
    let mut dy = y + if transform & 1 == 0 {
        frame.anchor_y
    } else {
        -(frame.anchor_y + frame.h)
    };
    let mut sw = frame.w;
    let mut sh = frame.h;
    let mut sx = frame.src_x;
    let mut sy = frame.src_y;
    if sx < 0 {
        sw += sx;
        dx += sx;
        sx = 0;
    }
    if sy < 0 {
        sh += sy;
        dy += sy;
        sy = 0;
    }
    if sx + sw > img.width as i32 {
        sw -= sx + sw - img.width as i32;
    }
    if sy + sh > img.height as i32 {
        sh -= sy + sh - img.height as i32;
    }
    if sw > 0 && sh > 0 {
        g.draw_region(fb, img, sx, sy, sw, sh, transform, dx, dy, 20);
    }
}

// ---------- RecordStore ----------

/// File-backed RecordStore, mirroring `a.a(int)` / `a.a(int, byte[])`:
/// store name = the integer as a string (e.g. "-9" for settings);
/// data is split into records of at most 589,824 bytes; reading
/// concatenates records 1..=num_records in order. A write deletes any
/// existing store of that name first; writing `None` just deletes it.
pub const RECORD_SPLIT: usize = 589_824;

pub struct RecordStore {
    root: PathBuf,
}

impl RecordStore {
    pub fn new(root: impl AsRef<Path>) -> RecordStore {
        RecordStore {
            root: root.as_ref().to_path_buf(),
        }
    }
    fn store_dir(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }
    pub fn read_all(&self, name: &str) -> Option<Vec<u8>> {
        let dir = self.store_dir(name);
        if !dir.is_dir() {
            return None;
        }
        let mut out = Vec::new();
        let mut id = 1usize;
        loop {
            let path = dir.join(id.to_string());
            match fs::read(&path) {
                Ok(bytes) => {
                    out.extend_from_slice(&bytes);
                    id += 1;
                }
                Err(_) => break,
            }
        }
        if id == 1 {
            return None;
        }
        Some(out)
    }
    pub fn write_all(&self, name: &str, data: Option<&[u8]>) -> bool {
        let dir = self.store_dir(name);
        if dir.exists() {
            if fs::remove_dir_all(&dir).is_err() {
                return false;
            }
        }
        let Some(data) = data else {
            return true;
        };
        if fs::create_dir_all(&dir).is_err() {
            return false;
        }
        let chunks = data.chunks(RECORD_SPLIT);
        let count = chunks.len();
        for (i, chunk) in chunks.enumerate() {
            if fs::write(dir.join((i + 1).to_string()), chunk).is_err() {
                return false;
            }
        }
        let _ = count;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fill_rect_respects_clip() {
        let mut fb = Framebuffer::new(10, 10);
        let mut g = Graphics::default();
        g.set_clip(2, 2, 4, 4);
        g.set_color_rgb(255, 0, 0);
        g.fill_rect(&mut fb, 0, 0, 10, 10);
        assert_eq!(fb.pixels[2 * 10 + 2], 0xFFFF_0000);
        assert_eq!(fb.pixels[0], 0);
        assert_eq!(fb.pixels[6 * 10 + 6], 0);
    }
    #[test]
    fn record_store_roundtrip_and_split() {
        let dir = std::env::temp_dir().join("gow-rms-test");
        let _ = fs::remove_dir_all(&dir);
        let rs = RecordStore::new(&dir);
        assert_eq!(rs.read_all("-9"), None);
        assert!(rs.write_all("-9", Some(b"abc")));
        assert_eq!(rs.read_all("-9").unwrap(), b"abc");
        let big = vec![7u8; RECORD_SPLIT + 10];
        assert!(rs.write_all("1", Some(&big)));
        assert_eq!(rs.read_all("1").unwrap().len(), RECORD_SPLIT + 10);
        assert!(rs.write_all("1", None));
        assert_eq!(rs.read_all("1"), None);
        let _ = fs::remove_dir_all(&dir);
    }
}
