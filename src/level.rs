//! Level tile map, slice 3a: `e.a(byte[], false)` case 0, up to the
//! tile map, from `decompiled/e.java`.
//!
//! The level chunk (e.g. 9217 = RP9 chunk 1, "For Sparta") is read
//! with the bit reader `e.m(int)` — low bits of each byte first
//! (as `e.m` computes it: `je >> 8-jf`), `jf` bits remaining
//! in the current byte — not with byte-aligned fields:
//!   m(1), m(10),
//!   c(false): Q = m(10), hv = m(1) != 1, dA = m(1) == 1,
//!     dB = m(1) == 1, m(1), hs = m(16),
//!   m(1),
//!   bw = m(16) (palette size), bx = m(4) (bits per palette entry),
//!   palette[bw] = m(bx) each, by = l(bw) (bits per map cell,
//!     `l(n)` = smallest k with (n-1) >> k == 0),
//!   iX = m(16) (tiles wide), iY = m(16) (tiles high),
//!   map[iX*iY] = m(by) each (palette indices),
//!   m(8),
//!   collision[bw] = m(1) each (only bit 0 is read, `b(int, false)`).
//!
//! The renderer then sets tile (x, y) to `1 + palette[map]`
//! (`b.a(x, y, 1 + nArray[iV])`), and `b.b(Graphics, ...)` draws
//! tile `l - 1` from the tileset, so the tileset tile actually drawn
//! is exactly `palette[map]` — tileset 2058 (RP2 chunk 10, 128x256,
//! 16x16 tiles, 8 per row, 128 tiles, indices 0..127, matching the
//! palette range 0..127 found in every level checked).
//!
//! Verified against the first three levels (Python prototype, then
//! Rust tests): For Sparta 9217 = 232x39 tiles (3712x624 px),
//! 19456 = 204x46, 20480 = 112x59. Bits consumed by the tile map
//! alone are ~half the chunk; the rest is entity/object data
//! (`bw = m(10)` and following), which is slice 3b, not parsed here.
//!
//! Drawing: `draw_view` copies 16x16 tiles from the tileset image
//! (an `Image` from `Resources::image(2058, key)`) into a framebuffer
//! at the given camera offset, skipping fully transparent pixels
//! (the MIDP drawRegion semantics in `midp.rs` already do that via
//! alpha-0 pixels). `draw_full` renders the whole level to an
//! `Image` for inspection.

use crate::midp::{Framebuffer, Image};
use crate::rp::RpError;

pub struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    cur: u8,
    bits_left: u8,
}

impl<'a> BitReader<'a> {
    pub fn new(data: &'a [u8]) -> BitReader<'a> {
        BitReader {
            data,
            pos: 0,
            cur: 0,
            bits_left: 0,
        }
    }

    /// `e.m(n)`: read n bits, low bits of each byte first, as in e.m.
    pub fn read(&mut self, n: usize) -> u32 {
        let mut out: u32 = 0;
        let mut remaining = n;
        while remaining > 0 {
            if self.bits_left == 0 {
                self.cur = *self.data.get(self.pos).unwrap_or(&0);
                self.pos += 1;
                self.bits_left = 8;
            }
            let take = remaining.min(self.bits_left as usize);
            let shift = 8 - self.bits_left;
            let mask = (1u32 << take) - 1;
            out |= (((self.cur >> shift) as u32) & mask) << (n - remaining);
            self.bits_left -= take as u8;
            remaining -= take;
        }
        out
    }

    pub fn bytes_consumed(&self) -> usize {
        self.pos
    }

    pub fn bits_consumed(&self) -> usize {
        self.pos * 8 - self.bits_left as usize
    }
}

/// `e.l(n)`: smallest k with (n-1) >> k == 0.
pub fn bits_needed(n: usize) -> usize {
    let mut k = 0usize;
    // Mirrors the Java loop: k starts at -1, increments until
    // (n-1) >> k == 0; for n <= 1 that is 0.
    if n <= 1 {
        return 0;
    }
    while ((n - 1) >> k) > 0 {
        k += 1;
    }
    k
}

#[derive(Debug, Clone)]
pub struct Level {
    pub id: u32,
    pub width_tiles: usize,
    pub height_tiles: usize,
    pub palette: Vec<u16>,
    /// Tileset tile index per cell (== palette[map cell]).
    pub tiles: Vec<u16>,
    pub collision: Vec<bool>,
    pub q: i32,
    pub hs: i32,
    /// Facing direction from the level header `c(false)`:
    /// `hu = hv = m(1) != 1`. True = facing right (the usual case).
    pub facing_right: bool,
    /// Level header flags `dA` / `dB`: they select which enemy
    /// sprite/anim sets are used (dC 4097 vs 18433, dD 17409 vs 16385).
    pub da: bool,
    pub db: bool,
}

impl Level {
    pub fn pixel_width(&self) -> usize {
        self.width_tiles * 16
    }
    pub fn pixel_height(&self) -> usize {
        self.height_tiles * 16
    }
    pub fn tile_at(&self, x: usize, y: usize) -> u16 {
        self.tiles
            .get(y * self.width_tiles + x)
            .copied()
            .unwrap_or(0)
    }
}

pub fn parse_level(id: u32, data: &[u8]) -> Result<Level, RpError> {
    let mut br = BitReader::new(data);
    parse_level_from_reader(id, &mut br)
}

/// Tile-map section only (case 0 up to the collision bits), leaving
/// the reader positioned at the case-0 tail (`bw = m(10)`), so
/// `entities.rs` can continue with the entity groups from the same
/// stream — exactly as `e.a(byte[], false)` does across its u=0..10
/// calls, where only u=0 sets the reader and u=10 consumes the rest.
pub fn parse_level_from_reader(id: u32, br: &mut BitReader) -> Result<Level, RpError> {
    let _ = br.read(1);
    let _ = br.read(10);
    // c(false)
    let q = br.read(10) as i32;
    let facing_right = br.read(1) != 1;
    let da = br.read(1) == 1;
    let db = br.read(1) == 1;
    let _ = br.read(1);
    let hs = br.read(16) as i32;
    let _ = br.read(1);
    let bw = br.read(16) as usize;
    let bx = br.read(4) as usize;
    let mut palette = Vec::with_capacity(bw);
    for _ in 0..bw {
        palette.push(br.read(bx) as u16);
    }
    let by = bits_needed(bw);
    let width = br.read(16) as usize;
    let height = br.read(16) as usize;
    if width == 0 || height == 0 || width > 1024 || height > 1024 {
        return Err(RpError(format!(
            "level {id}: implausible dimensions {width}x{height}"
        )));
    }
    let mut tiles = Vec::with_capacity(width * height);
    for _ in 0..width * height {
        let idx = br.read(by) as usize;
        tiles.push(*palette.get(idx).unwrap_or(&0));
    }
    let _ = br.read(8);
    let mut collision = Vec::with_capacity(bw);
    for _ in 0..bw {
        collision.push(br.read(1) == 1);
    }
    Ok(Level {
        id,
        width_tiles: width,
        height_tiles: height,
        palette,
        tiles,
        collision,
        q,
        hs,
        facing_right,
        da,
        db,
    })
}

/// Draw a 240x320 viewport of the level at camera (cam_x, cam_y)
/// into a framebuffer, following `b.java`'s tile drawing: tiles are
/// 16x16, taken row-major from the tileset (8 per row).
pub fn draw_view(level: &Level, tileset: &Image, fb: &mut Framebuffer, cam_x: i32, cam_y: i32) {
    fb.clear(0xFF00_0000);
    // Source tile size in image pixels (16 logical px * image scale);
    // destination tile size in framebuffer pixels (16 * fb scale).
    let sw = 16i32 * tileset.scale.max(1) as i32;
    let dw = 16i32 * fb.scale as i32;
    let tile_w = 16i32;
    let cols = (tileset.width as i32 / sw).max(1);
    let x0 = (cam_x / tile_w).max(0) as usize;
    let y0 = (cam_y / tile_w).max(0) as usize;
    let x1 = ((cam_x + 240 + tile_w - 1) / tile_w).min(level.width_tiles as i32) as usize;
    let y1 = ((cam_y + 320 + tile_w - 1) / tile_w).min(level.height_tiles as i32) as usize;
    for ty in y0..y1 {
        for tx in x0..x1 {
            let tile = level.tile_at(tx, ty) as i32;
            let sx = (tile % cols) * sw;
            let sy = (tile / cols) * sw;
            let dx = (tx as i32 * tile_w - cam_x) * fb.scale as i32;
            let dy = (ty as i32 * tile_w - cam_y) * fb.scale as i32;
            // Source coordinates are exact and division-free: the
            // source row is fixed per destination row, and the
            // source column advances with a Bresenham error term
            // (src = dst*sw/dw). The old code divided twice per
            // pixel; at HD that is ~2.4M divisions per frame.
            let mut src_y = sy;
            let mut err_y = 0i32;
            for py in 0..dw {
                let mut src_x = sx;
                let mut err_x = 0i32;
                let fy = dy + py;
                let fb_row = if fy >= 0 && (fy as usize) < fb.height {
                    (fy as usize) * fb.width
                } else {
                    // Whole row is off-screen; still advance src_y.
                    err_y += sw;
                    while err_y >= dw {
                        src_y += 1;
                        err_y -= dw;
                    }
                    continue;
                };
                for px in 0..dw {
                    let sp = tileset.pixel(src_x as usize, src_y as usize);
                    if (sp >> 24) & 0xFF != 0 {
                        let fx = dx + px;
                        if fx >= 0 && (fx as usize) < fb.width {
                            fb.pixels[fb_row + (fx as usize)] = sp;
                        }
                    }
                    err_x += sw;
                    while err_x >= dw {
                        src_x += 1;
                        err_x -= dw;
                    }
                }
                err_y += sw;
                while err_y >= dw {
                    src_y += 1;
                    err_y -= dw;
                }
            }
        }
    }
}

/// Render the whole level to an Image (for inspection / tests).
/// Output is always at logical (1x) resolution; an HD tileset is
/// nearest-downsampled to it.
pub fn draw_full(level: &Level, tileset: &Image) -> Image {
    let mut img = Image::from_argb(level.pixel_width(), level.pixel_height(), vec![0; level.pixel_width() * level.pixel_height()]).unwrap();
    let tile_w = 16i32;
    let sw = tile_w * tileset.scale.max(1) as i32;
    let cols = (tileset.width as i32 / sw).max(1);
    for ty in 0..level.height_tiles {
        for tx in 0..level.width_tiles {
            let tile = level.tile_at(tx, ty) as i32;
            let sx = (tile % cols) * sw;
            let sy = (tile / cols) * sw;
            for py in 0..tile_w {
                for px in 0..tile_w {
                    let sp = tileset.pixel((sx + px * sw / tile_w) as usize, (sy + py * sw / tile_w) as usize);
                    if (sp >> 24) & 0xFF == 0 {
                        continue;
                    }
                    img.pixels[(ty * tile_w as usize + py as usize) * img.width + tx * tile_w as usize + px as usize] = sp;
                }
            }
        }
    }
    img
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bit_reader_msb_first() {
        let mut br = BitReader::new(&[0b1010_0101]);
        // 0b1010_0101, read low-to-high: 1, 010, 1010.
        assert_eq!(br.read(1), 1);
        assert_eq!(br.read(3), 0b010);
        assert_eq!(br.read(4), 0b1010);
    }
    #[test]
    fn bits_needed_matches_java_l() {
        assert_eq!(bits_needed(1), 0);
        assert_eq!(bits_needed(107), 7); // (107-1)>>7 == 0, >>6 == 1
        assert_eq!(bits_needed(108), 7);
    }
    #[test]
    fn parse_for_sparta() {
        let data = std::fs::read("../extracted/RP9").unwrap();
        // Parse the pack to get chunk 1 (level 9217).
        let pack = crate::rp::parse_pack(&data).unwrap();
        let chunk = pack.chunk(1).unwrap();
        let level = parse_level(9217, chunk).unwrap();
        assert_eq!(level.width_tiles, 232);
        assert_eq!(level.height_tiles, 39);
        assert_eq!(level.palette.len(), 107);
        assert_eq!(level.tiles.len(), 232 * 39);
        assert!(level.tiles.iter().all(|&t| t < 128));
        assert_eq!(level.tile_at(0, 0), 25);
    }
}
