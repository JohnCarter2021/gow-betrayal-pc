//! RP resource-pack parser.
//!
//! Direct translation of `a.a(DataInputStream)` in the decompiled J2ME code
//! (`decompiled/a.java`), verified against all 33 RP files in the jar:
//! for every file, sum(chunk sizes) == total field == payload length.
//!
//! Pack layout, all integers big-endian:
//!   byte 0: first byte; size-field width = (1 + (first & 1)) << 1
//!           (0x40 '@' in this jar => width 2)
//!   bytes 1..3: chunk count (u16)
//!   chunk count * width bytes: chunk sizes
//!   chunk count bytes: per-chunk flag bytes
//!   4 bytes: total payload size (u32)
//!   payload: chunks concatenated in order
//!
//! Resource ids used by the game: id = (file << 10) | chunk.
//! Example: 10240 = (10 << 10) | 0 => RP10 chunk 0 (English strings).

use std::fmt;

#[derive(Debug)]
pub struct RpPack {
    pub sizes: Vec<usize>,
    pub flags: Vec<u8>,
    pub payload: Vec<u8>,
}

#[derive(Debug)]
pub struct RpError(pub String);

impl fmt::Display for RpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RP error: {}", self.0)
    }
}
impl std::error::Error for RpError {}

pub fn parse_pack(data: &[u8]) -> Result<RpPack, RpError> {
    if data.len() < 7 {
        return Err(RpError("pack too short".into()));
    }
    // Java: int n2 = 1 + (n & 1) << 1  parses as (1 + (n & 1)) << 1.
    let width = ((1 + (data[0] & 1)) as usize) << 1;
    let count = u16::from_be_bytes([data[1], data[2]]) as usize;
    let mut off = 3usize;
    let need = off + count * width + count + 4;
    if data.len() < need {
        return Err(RpError(format!(
            "pack header truncated: need {need}, have {}",
            data.len()
        )));
    }
    let mut sizes = Vec::with_capacity(count);
    for _ in 0..count {
        let mut v: usize = 0;
        for _ in 0..width {
            v = (v << 8) | data[off] as usize;
            off += 1;
        }
        sizes.push(v);
    }
    let flags = data[off..off + count].to_vec();
    off += count;
    let total = u32::from_be_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]) as usize;
    off += 4;
    let payload = data[off..].to_vec();
    let sum: usize = sizes.iter().sum();
    if sum != total || payload.len() != total {
        return Err(RpError(format!(
            "size mismatch: sum={sum} total={total} payload={}",
            payload.len()
        )));
    }
    Ok(RpPack { sizes, flags, payload })
}

impl RpPack {
    pub fn chunk(&self, index: usize) -> Option<&[u8]> {
        if index >= self.sizes.len() {
            return None;
        }
        let start: usize = self.sizes[..index].iter().sum();
        let end = start + self.sizes[index];
        Some(&self.payload[start..end])
    }
}

/// String table from `a.f(n)`: first u16 = string count (the decompiled
/// code allocates count+1 offsets and reads count strings), then for each
/// string: u16 length + UTF-8 bytes.
pub fn parse_string_table(chunk: &[u8]) -> Result<Vec<String>, RpError> {
    if chunk.len() < 2 {
        return Err(RpError("string table too short".into()));
    }
    let count = u16::from_be_bytes([chunk[0], chunk[1]]) as usize;
    let mut off = 2usize;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        if off + 2 > chunk.len() {
            break;
        }
        let len = u16::from_be_bytes([chunk[off], chunk[off + 1]]) as usize;
        off += 2;
        if off + len > chunk.len() {
            break;
        }
        out.push(String::from_utf8_lossy(&chunk[off..off + len]).into_owned());
        off += len;
    }
    Ok(out)
}

/// Split a game resource id into (RP file number, chunk index).
pub fn split_resource_id(id: u32) -> (u32, usize) {
    (id >> 10, (id & 0x3FF) as usize)
}
