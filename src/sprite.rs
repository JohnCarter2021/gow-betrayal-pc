//! Sprite definitions (RP flag 254) and palette keys (flag 251).
//!
//! Direct translation of `a.b(int, byte[])` and `a.c(int, byte[])` in
//! `decompiled/a.java`, verified against all 34 definitions in the jar
//! (see `../sprites/frames.json`, produced by the Python reference decoder).
//!
//! Definition chunk layout:
//!   u16 image chunk index (pack-relative — the pack stays loaded)
//!   u8  ignored
//!   u16 frame-data length (= chunk size - 5, checked)
//!   u16 bitmask
//!   u16 frame count
//!   frame data, field-major: all frames' values for field 0, then field 1, …
//!
//! Bitmask -> 10 field widths, in 3 groups (2, then 4, then 4):
//! a field is absent (0), 1 byte, or 2 bytes. Field order in `c.a(short[])`:
//!   0 anchor_x, 1 anchor_y, 2 src_x, 3 src_y, 4 width, 5 height,
//!   6 e, 7 f, 8 g, 9 extra_h.
//! 1-byte fields 0, 1, 6, 7 are signed; other 1-byte fields are unsigned;
//! 2-byte values are big-endian signed 16-bit.

use crate::crc::crc32;
use crate::png::png_plte;
use crate::rp::RpError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frame {
    pub anchor_x: i32,
    pub anchor_y: i32,
    pub src_x: i32,
    pub src_y: i32,
    pub w: i32,
    pub h: i32,
    pub e: i32,
    pub f: i32,
    pub g: i32,
    pub extra_h: i32,
}

#[derive(Debug, Clone)]
pub struct SpriteDef {
    pub image_chunk: usize,
    pub field_widths: [usize; 10],
    pub frames: Vec<Frame>,
}

fn be_u16(b: &[u8], off: usize) -> Result<u16, RpError> {
    b.get(off..off + 2)
        .map(|s| u16::from_be_bytes([s[0], s[1]]))
        .ok_or_else(|| RpError("sprite def truncated".into()))
}

pub fn parse_sprite_def(chunk: &[u8]) -> Result<SpriteDef, RpError> {
    if chunk.len() < 9 {
        return Err(RpError("sprite def too short".into()));
    }
    let image_chunk = be_u16(chunk, 0)? as usize;
    // readShort at bytes 3-4: chunk size = readShort + 5, and the actual
    // frame data (after the 9-byte header) is readShort - 4 bytes.
    let read_short = be_u16(chunk, 3)? as usize;
    if read_short + 5 != chunk.len() {
        return Err(RpError(format!(
            "sprite def length mismatch: readShort+5={} chunk={}",
            read_short + 5,
            chunk.len()
        )));
    }
    let frame_data_len = read_short - 4;
    let bitmask = be_u16(chunk, 5)?;
    let frame_count = be_u16(chunk, 7)? as usize;
    let bl: [bool; 16] = std::array::from_fn(|i| (bitmask >> i) & 1 == 0);
    let mut widths = [0usize; 10];
    let mut idx = 5usize;
    let mut wi = 0usize;
    for group in 0..3 {
        let per = if group == 0 { 2 } else { 4 };
        for _ in 0..per {
            widths[wi] = if bl[group] {
                let w = if bl[idx] { 2 } else { 1 };
                idx += 1;
                w
            } else {
                0
            };
            wi += 1;
        }
    }
    let expected_len: usize = widths.iter().map(|w| w * frame_count).sum();
    if expected_len != frame_data_len {
        return Err(RpError(format!(
            "sprite def frame-data mismatch: widths sum {expected_len} != {frame_data_len}"
        )));
    }
    let framedata = &chunk[9..];
    let mut offsets = [0usize; 10];
    let mut off = 0usize;
    for (i, w) in widths.iter().enumerate() {
        offsets[i] = off;
        off += w * frame_count;
    }
    let mut frames = Vec::with_capacity(frame_count);
    for f in 0..frame_count {
        let mut vals = [0i32; 10];
        for (j, &w) in widths.iter().enumerate() {
            vals[j] = match w {
                0 => 0,
                1 => {
                    let v = framedata[offsets[j] + f];
                    if j == 0 || j == 1 || j == 6 || j == 7 {
                        v as i8 as i32
                    } else {
                        v as i32
                    }
                }
                2 => {
                    let raw = ((framedata[offsets[j] + f * 2] as u16) << 8)
                        | (framedata[offsets[j] + f * 2 + 1] as u16);
                    raw as i16 as i32
                }
                _ => unreachable!(),
            };
        }
        frames.push(Frame {
            anchor_x: vals[0],
            anchor_y: vals[1],
            src_x: vals[2],
            src_y: vals[3],
            w: vals[4],
            h: vals[5],
            e: vals[6],
            f: vals[7],
            g: vals[8],
            extra_h: vals[9],
        });
    }
    Ok(SpriteDef {
        image_chunk,
        field_widths: widths,
        frames,
    })
}

/// Apply a palette key (flag 251) to a PNG: key = new PLTE + CRC32("PLTE" +
/// new PLTE). Returns a new PNG with the PLTE data and its CRC replaced.
/// This mirrors `a.c()`'s in-place swap (swap in, createImage, swap back);
/// the Rust version just returns the swapped bytes.
pub fn apply_palette_key(png: &[u8], key: &[u8]) -> Result<Vec<u8>, RpError> {
    let (plte_off, plte_len) = png_plte(png).ok_or_else(|| RpError("PNG has no PLTE".into()))?;
    if key.len() != plte_len + 4 {
        return Err(RpError(format!(
            "palette key length {} != PLTE {} + 4",
            key.len(),
            plte_len
        )));
    }
    let new_plte = &key[..plte_len];
    let mut crc_input = b"PLTE".to_vec();
    crc_input.extend_from_slice(new_plte);
    let expected = crc32(&crc_input).to_be_bytes();
    if key[plte_len..] != expected {
        return Err(RpError("palette key CRC mismatch".into()));
    }
    let mut out = png.to_vec();
    out[plte_off..plte_off + key.len()].copy_from_slice(key);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rp1_def5_first_frames() {
        // Checked against sprites/frames.json: RP1 def chunk 5 -> PNG 17,
        // 4 frames of 5x5 at src_y 0, 5, 10, 15.
        let data = std::fs::read("../extracted/RP1").unwrap();
        let pack = crate::rp::parse_pack(&data).unwrap();
        assert_eq!(pack.flags[5], 254);
        let def = parse_sprite_def(pack.chunk(5).unwrap()).unwrap();
        assert_eq!(def.image_chunk, 17);
        assert_eq!(def.frames.len(), 4);
        assert_eq!(def.frames[0].w, 5);
        assert_eq!(def.frames[0].h, 5);
        assert_eq!(def.frames[1].src_y, 5);
        assert_eq!(def.frames[3].src_y, 15);
    }
}
