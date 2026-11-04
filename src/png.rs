//! Minimal std-only PNG decoder/encoder, scoped to what this game needs.
//!
//! Decoder: 8-bit, non-interlaced, colour type 3 (palette) only — that is
//! every one of the 34 sprite sheets in the jar. PLTE + tRNS are honoured
//! (tRNS alpha 0 = transparent; indices past the tRNS length are opaque).
//! DEFLATE (stored / fixed-Huffman / dynamic-Huffman) and PNG filters
//! (None / Sub / Up / Average / Paeth) are implemented here, no crates.
//!
//! Encoder: 8-bit RGBA (colour type 6), filter None, stored DEFLATE blocks.
//! Used only for test-output images from the port, not for game assets.

use crate::crc::crc32;

#[derive(Debug)]
pub struct PngError(pub String);
impl std::fmt::Display for PngError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PNG error: {}", self.0)
    }
}
impl std::error::Error for PngError {}

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

// ---------- DEFLATE ----------

struct BitReader<'a> {
    data: &'a [u8],
    pos: usize, // bit position
}
impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        BitReader { data, pos: 0 }
    }
    fn bits(&mut self, n: usize) -> Result<u32, PngError> {
        let mut v = 0u32;
        for i in 0..n {
            let byte = self.pos / 8;
            if byte >= self.data.len() {
                return Err(PngError("deflate: unexpected end".into()));
            }
            let bit = (self.data[byte] >> (self.pos % 8)) & 1;
            v |= (bit as u32) << i;
            self.pos += 1;
        }
        Ok(v)
    }
    fn align_byte(&mut self) {
        self.pos = (self.pos + 7) / 8 * 8;
    }
}

struct Huffman {
    // (code, length, symbol), sorted by (length, code) for canonical decode.
    codes: Vec<(u32, u8, u16)>,
    max_len: u8,
}
impl Huffman {
    fn from_lengths(lengths: &[u8]) -> Huffman {
        let mut bl_count = [0u32; 16];
        for &l in lengths {
            if l > 0 {
                bl_count[l as usize] += 1;
            }
        }
        let mut next_code = [0u32; 16];
        let mut code = 0u32;
        for bits in 1..16 {
            code = (code + bl_count[bits - 1]) << 1;
            next_code[bits] = code;
        }
        let mut codes = Vec::new();
        let mut max_len = 0u8;
        for (sym, &l) in lengths.iter().enumerate() {
            if l > 0 {
                let c = next_code[l as usize];
                next_code[l as usize] += 1;
                codes.push((c, l, sym as u16));
                max_len = max_len.max(l);
            }
        }
        codes.sort_by_key(|&(c, l, _)| (l, c));
        Huffman { codes, max_len }
    }
    fn decode(&self, r: &mut BitReader) -> Result<u16, PngError> {
        let mut code = 0u32;
        for len in 1..=self.max_len {
            let bit = r.bits(1)?;
            code = (code << 1) | bit;
            // Linear scan is fine for these small PNGs (max ~17 KB IDAT).
            for &(c, l, sym) in &self.codes {
                if l == len && c == code {
                    return Ok(sym);
                }
            }
        }
        Err(PngError("deflate: invalid huffman code".into()))
    }
}

const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

fn inflate_block(
    r: &mut BitReader,
    lit: &Huffman,
    dist: &Huffman,
    out: &mut Vec<u8>,
) -> Result<bool, PngError> {
    // Returns true on end-of-block.
    loop {
        let sym = lit.decode(r)?;
        if sym < 256 {
            out.push(sym as u8);
        } else if sym == 256 {
            return Ok(true);
        } else {
            let li = (sym - 257) as usize;
            if li >= 29 {
                return Err(PngError("deflate: bad length symbol".into()));
            }
            let len = LENGTH_BASE[li] as usize + r.bits(LENGTH_EXTRA[li] as usize)? as usize;
            let dsym = dist.decode(r)? as usize;
            if dsym >= 30 {
                return Err(PngError("deflate: bad distance symbol".into()));
            }
            let d = DIST_BASE[dsym] as usize + r.bits(DIST_EXTRA[dsym] as usize)? as usize;
            if d > out.len() {
                return Err(PngError("deflate: distance past start".into()));
            }
            let start = out.len() - d;
            for i in 0..len {
                let b = out[start + i];
                out.push(b);
            }
        }
    }
}

pub fn inflate(data: &[u8]) -> Result<Vec<u8>, PngError> {
    // zlib wrapper: 2-byte header, then DEFLATE stream (adler32 trailer ignored
    // here; PNG chunk CRCs already protect the IDAT bytes themselves).
    if data.len() < 2 {
        return Err(PngError("zlib stream too short".into()));
    }
    let mut r = BitReader::new(&data[2..]);
    let mut out = Vec::new();
    loop {
        let bfinal = r.bits(1)?;
        let btype = r.bits(2)?;
        match btype {
            0 => {
                r.align_byte();
                let byte_pos = r.pos / 8;
                if byte_pos + 4 > r.data.len() {
                    return Err(PngError("deflate: stored block truncated".into()));
                }
                let len = u16::from_le_bytes([r.data[byte_pos], r.data[byte_pos + 1]]) as usize;
                if byte_pos + 4 + len > r.data.len() {
                    return Err(PngError("deflate: stored block overruns".into()));
                }
                out.extend_from_slice(&r.data[byte_pos + 4..byte_pos + 4 + len]);
                r.pos = (byte_pos + 4 + len) * 8;
            }
            1 => {
                let mut lit_lengths = [0u8; 288];
                for i in 0..144 {
                    lit_lengths[i] = 8;
                }
                for i in 144..256 {
                    lit_lengths[i] = 9;
                }
                for i in 256..280 {
                    lit_lengths[i] = 7;
                }
                for i in 280..288 {
                    lit_lengths[i] = 8;
                }
                let lit = Huffman::from_lengths(&lit_lengths);
                let dist = Huffman::from_lengths(&[5u8; 30]);
                inflate_block(&mut r, &lit, &dist, &mut out)?;
            }
            2 => {
                let hlit = r.bits(5)? as usize + 257;
                let hdist = r.bits(5)? as usize + 1;
                let hclen = r.bits(4)? as usize + 4;
                const ORDER: [usize; 19] = [
                    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
                ];
                let mut cl_lengths = [0u8; 19];
                for i in 0..hclen {
                    cl_lengths[ORDER[i]] = r.bits(3)? as u8;
                }
                let cl = Huffman::from_lengths(&cl_lengths);
                let total = hlit + hdist;
                let mut lengths = vec![0u8; total];
                let mut i = 0;
                while i < total {
                    let sym = cl.decode(&mut r)?;
                    match sym {
                        0..=15 => {
                            lengths[i] = sym as u8;
                            i += 1;
                        }
                        16 => {
                            if i == 0 {
                                return Err(PngError("deflate: repeat with no previous".into()));
                            }
                            let prev = lengths[i - 1];
                            let rep = 3 + r.bits(2)? as usize;
                            for _ in 0..rep {
                                if i >= total {
                                    break;
                                }
                                lengths[i] = prev;
                                i += 1;
                            }
                        }
                        17 => {
                            let rep = 3 + r.bits(3)? as usize;
                            i += rep;
                        }
                        18 => {
                            let rep = 11 + r.bits(7)? as usize;
                            i += rep;
                        }
                        _ => return Err(PngError("deflate: bad code-length symbol".into())),
                    }
                }
                let lit = Huffman::from_lengths(&lengths[..hlit]);
                let dist = Huffman::from_lengths(&lengths[hlit..]);
                inflate_block(&mut r, &lit, &dist, &mut out)?;
            }
            _ => return Err(PngError("deflate: reserved block type".into())),
        }
        if bfinal == 1 {
            break;
        }
    }
    Ok(out)
}

// ---------- PNG decode ----------

fn paeth(a: i32, b: i32, c: i32) -> i32 {
    let p = a + b - c;
    let pa = (p - a).abs();
    let pb = (p - b).abs();
    let pc = (p - c).abs();
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// Decoded image: pixels are ARGB (0xAARRGGBB), row-major.
pub struct DecodedPng {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u32>,
}

pub fn decode_png(data: &[u8]) -> Result<DecodedPng, PngError> {
    if data.len() < 8 || &data[0..8] != b"\x89PNG\r\n\x1a\n" {
        return Err(PngError("not a PNG".into()));
    }
    let mut off = 8usize;
    let mut width = 0usize;
    let mut height = 0usize;
    let mut bit_depth = 0u8;
    let mut color_type = 0u8;
    let mut interlace = 0u8;
    let mut palette: Vec<[u8; 3]> = Vec::new();
    let mut trns: Vec<u8> = Vec::new();
    let mut idat: Vec<u8> = Vec::new();
    while off + 8 <= data.len() {
        let len = be32(&data[off..]) as usize;
        let typ = &data[off + 4..off + 8];
        let chunk_start = off + 8;
        if chunk_start + len + 4 > data.len() {
            return Err(PngError("PNG chunk overruns file".into()));
        }
        let chunk = &data[chunk_start..chunk_start + len];
        match typ {
            b"IHDR" => {
                if len != 13 {
                    return Err(PngError("bad IHDR".into()));
                }
                width = be32(&chunk[0..]) as usize;
                height = be32(&chunk[4..]) as usize;
                bit_depth = chunk[8];
                color_type = chunk[9];
                // chunk[10] compression, chunk[11] filter: must be 0 for PNG.
                interlace = chunk[12];
            }
            b"PLTE" => {
                if len % 3 != 0 {
                    return Err(PngError("bad PLTE length".into()));
                }
                palette = chunk
                    .chunks_exact(3)
                    .map(|c| [c[0], c[1], c[2]])
                    .collect();
            }
            b"tRNS" => trns = chunk.to_vec(),
            b"IDAT" => idat.extend_from_slice(chunk),
            b"IEND" => break,
            _ => {}
        }
        off = chunk_start + len + 4;
    }
    if width == 0 || height == 0 {
        return Err(PngError("PNG has no IHDR".into()));
    }
    if bit_depth != 8 || color_type != 3 || interlace != 0 {
        return Err(PngError(format!(
            "unsupported PNG: bit_depth={bit_depth} color_type={color_type} interlace={interlace} (game sheets are 8-bit palette, non-interlaced)"
        )));
    }
    if palette.is_empty() {
        return Err(PngError("palette PNG without PLTE".into()));
    }
    let raw = inflate(&idat)?;
    // One filter byte per scanline, 1 byte per pixel (palette index).
    let stride = width + 1;
    if raw.len() < height * stride {
        return Err(PngError(format!(
            "inflated data too short: {} < {}",
            raw.len(),
            height * stride
        )));
    }
    let mut indices = vec![0u8; width * height];
    let mut prev: Vec<u8> = vec![0; width];
    for y in 0..height {
        let filter = raw[y * stride];
        let row = &raw[y * stride + 1..(y + 1) * stride];
        let mut cur = vec![0u8; width];
        for x in 0..width {
            let a = if x > 0 { cur[x - 1] as i32 } else { 0 };
            let b = prev[x] as i32;
            let c = if x > 0 { prev[x - 1] as i32 } else { 0 };
            let v = row[x] as i32;
            cur[x] = match filter {
                0 => v as u8,
                1 => (v + a) as u8,
                2 => (v + b) as u8,
                3 => (v + (a + b) / 2) as u8,
                4 => (v + paeth(a, b, c)) as u8,
                _ => return Err(PngError(format!("unknown PNG filter {filter}"))),
            };
        }
        indices[y * width..(y + 1) * width].copy_from_slice(&cur);
        prev = cur;
    }
    let mut pixels = Vec::with_capacity(width * height);
    for &idx in &indices {
        let rgb = palette.get(idx as usize).copied().unwrap_or([0, 0, 0]);
        let alpha = trns.get(idx as usize).copied().unwrap_or(255);
        pixels.push(
            ((alpha as u32) << 24)
                | ((rgb[0] as u32) << 16)
                | ((rgb[1] as u32) << 8)
                | (rgb[2] as u32),
        );
    }
    Ok(DecodedPng {
        width,
        height,
        pixels,
    })
}

/// Find the PLTE data offset and length in a PNG byte string.
pub fn png_plte(data: &[u8]) -> Option<(usize, usize)> {
    let mut off = 8usize;
    while off + 8 <= data.len() {
        let len = be32(&data[off..]) as usize;
        let typ = &data[off + 4..off + 8];
        if typ == b"PLTE" {
            return Some((off + 8, len));
        }
        off += 12 + len;
    }
    None
}

// ---------- PNG encode (RGBA, stored DEFLATE) ----------

pub fn encode_png_rgba(width: usize, height: usize, rgba: &[u8]) -> Vec<u8> {
    // rgba: 4 bytes per pixel, row-major, no filter (filter byte 0 per row).
    let mut raw = Vec::with_capacity(height * (1 + width * 4));
    for y in 0..height {
        raw.push(0u8);
        raw.extend_from_slice(&rgba[y * width * 4..(y + 1) * width * 4]);
    }
    // zlib header 0x78 0x01 (no/low compression) + stored blocks + adler32.
    let mut z = vec![0x78u8, 0x01];
    let mut pos = 0usize;
    loop {
        let chunk = raw.len().min(pos + 65535) - pos;
        let last = pos + chunk >= raw.len();
        z.push(if last { 1 } else { 0 });
        z.extend_from_slice(&(chunk as u16).to_le_bytes());
        z.extend_from_slice(&(!(chunk as u16)).to_le_bytes());
        z.extend_from_slice(&raw[pos..pos + chunk]);
        pos += chunk;
        if last {
            break;
        }
    }
    // Adler-32.
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in &raw {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());

    let mut out = Vec::new();
    out.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    let mut write_chunk = |out: &mut Vec<u8>, typ: &[u8; 4], data: &[u8]| {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(typ);
        out.extend_from_slice(data);
        let mut crc_data = typ.to_vec();
        crc_data.extend_from_slice(data);
        out.extend_from_slice(&crc32(&crc_data).to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(width as u32).to_be_bytes());
    ihdr.extend_from_slice(&(height as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // bit depth, colour type 6, comp, filter, interlace
    write_chunk(&mut out, b"IHDR", &ihdr);
    write_chunk(&mut out, b"IDAT", &z);
    write_chunk(&mut out, b"IEND", &[]);
    out
}
