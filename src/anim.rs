//! Animation data, slice 3b-3: `a.g(int)` in `decompiled/a.java`,
//! plus the frame extraction `a.a(short[], int, int)` used by `e.s()`.
//!
//! A data set (e.g. 3073 = RP3 chunk 1, flag 247) is a stream of
//! variable-length ints:
//!   k(): read a byte; 255 escapes to a u16 char.
//!   l(): read a signed byte; -128 escapes to an i16 short.
//! All packed fields use Java i32 semantics (wrapping shifts;
//! `>>>` is a logical shift, implemented via `as u32`).
//!
//! Layout of `g()`, in order (names are the Java tables):
//!   byte (skipped), n4 = readChar (action count)
//!   B[n4]: a() — cumulative offset <<16 | count per action
//!   B[i] |= (0x3F & l()) << 10
//!   C/D[total]: k() per frame — C = low byte, D = high byte
//!   pack(D): per entry k()<<22, then l()<<22>>>10, then l()<<22>>>20
//!     (note: pack applies to D, not B — an easy misread; the
//!     Python prototype first packed B and landed on zeros)
//!   c2 = readChar (frame-table count)
//!   F[c2]: a() — cumulative offset <<16 | count
//!   G[totalF]: 2-bit values, 4 per k() byte
//!   pack(G): same three fields as above
//!   F[i] |= k() << 8; E[i] = cumulative sum (reset to 0 first —
//!     reusing the previous total here is the second misread the
//!     prototype caught; E total may be 0, as for 3073)
//!   H/I[totalE]: l()<<21, l()<<21>>>11, k(); then k()<<6, k()
//!   J[readChar]: k() each
//!
//! Verification for 3073: B=235 actions, D=238, F=221, G=1313,
//! H/I=0, J=[100], and the stream ends exactly at the chunk end
//! (6270 bytes). Any misread desyncs and breaks that total.
//!
//! Frame extraction, `a(short[], slot, 0)` with slot 136 set by
//! `a(136, 3073, action)`:
//!   d_idx = D[B[action] >>> 16] >>> 22
//!   f = F[d_idx]; offset = f >>> 16, count = f & 0xFF
//!   per G entry: sprite = g >>> 22,
//!     dx = ((transform & 2) == 0 ? 1 : -1) * (g << 10 >> 22),
//!     dy = ((transform & 1) == 0 ? 1 : -1) * (g << 20 >> 22),
//!     transform = g & 3
//! The dx/dy sign flips are part of `a(short[])`, not of `c.a` —
//! `draw_sprite_frame` then applies the frame's own anchor flip.
//!
//! Static sprites, `e.s()`: for layer 0..=7, for each type-23
//! item in position order whose eL (extra m3) equals the layer,
//! draw every frame of its action (extra m8) at
//! (eJ + dx, eK + dy) via the renderer. Pairing by position index
//! is exact: extra pair p (read order) belongs to position p
//! (see the reversed loop in `d()` case 23, worked out in the
//! prototype).

use crate::midp::{draw_sprite_frame, Framebuffer, Graphics, Image};
use crate::resources::SpriteSet;
use crate::rp::RpError;

struct Stream<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Stream<'a> {
    fn read_byte(&mut self) -> i32 {
        let v = *self.data.get(self.pos).unwrap_or(&0);
        self.pos += 1;
        v as i32
    }
    fn read_char(&mut self) -> i32 {
        let hi = self.read_byte();
        let lo = self.read_byte();
        (hi << 8) | lo
    }
    fn read_sbyte(&mut self) -> i32 {
        let v = self.read_byte();
        if v >= 128 {
            v - 256
        } else {
            v
        }
    }
    fn read_short(&mut self) -> i32 {
        let hi = self.read_byte();
        let lo = self.read_byte();
        let v = (hi << 8) | lo;
        if v >= 32768 {
            v - 65536
        } else {
            v
        }
    }
    /// `a.k()`
    fn k(&mut self) -> i32 {
        let n = self.read_byte();
        if n == 255 {
            self.read_char()
        } else {
            n
        }
    }
    /// `a.l()`
    fn l(&mut self) -> i32 {
        let b = self.read_sbyte();
        if b == -128 {
            self.read_short()
        } else {
            b
        }
    }
}

fn ushr(x: i32, n: u32) -> i32 {
    ((x as u32) >> n) as i32
}

fn pack(arr: &mut [i32], s: &mut Stream) {
    for i in 0..arr.len() {
        arr[i] |= s.k().wrapping_shl(22);
    }
    for i in 0..arr.len() {
        arr[i] |= ushr(s.l().wrapping_shl(22), 10);
    }
    for i in 0..arr.len() {
        arr[i] |= ushr(s.l().wrapping_shl(22), 20);
    }
}

#[derive(Debug, Clone)]
pub struct AnimSet {
    pub b: Vec<i32>,
    /// Low byte of each C/D entry (duration index + flags), as
    /// `a.b(int, int)` needs it for frame durations.
    pub c: Vec<i32>,
    pub d: Vec<i32>,
    pub f: Vec<i32>,
    pub g: Vec<i32>,
    pub h: Vec<i32>,
    pub j: Vec<i32>,
    pub bytes_consumed: usize,
}

pub fn load_anim_set(data: &[u8]) -> Result<AnimSet, RpError> {
    let mut s = Stream { data, pos: 0 };
    let _ = s.read_byte();
    let n4 = s.read_char() as usize;
    let mut b = vec![0i32; n4];
    let mut total = 0i32;
    for i in 0..n4 {
        let c = s.k();
        b[i] = (total << 16) | c;
        total += c;
    }
    for i in 0..n4 {
        b[i] |= (0x3F & s.l()) << 10;
    }
    let total = total as usize;
    let mut c = vec![0i32; total];
    let mut d = vec![0i32; total];
    for i in 0..total {
        let v = s.k();
        c[i] = v & 0xFF;
        d[i] = v >> 8;
    }
    pack(&mut d, &mut s);
    let c2 = s.read_char() as usize;
    let mut f = vec![0i32; c2];
    let mut total_f = 0i32;
    for i in 0..c2 {
        let c = s.k();
        f[i] = (total_f << 16) | c;
        total_f += c;
    }
    let total_f = total_f as usize;
    let mut g = vec![0i32; total_f];
    let mut n6 = 0i32;
    let mut cur = 0i32;
    for i in 0..total_f {
        if n6 == 0 {
            cur = s.k();
            n6 = 8;
        }
        n6 -= 2;
        g[i] = 3 & (cur >> n6);
    }
    pack(&mut g, &mut s);
    let mut total_e = 0i32;
    for i in 0..c2 {
        let v = s.k();
        f[i] |= v << 8;
        total_e += v;
    }
    let total_e = total_e as usize;
    let mut h = vec![0i32; total_e];
    for i in 0..total_e {
        h[i] = s.l().wrapping_shl(21);
    }
    for i in 0..total_e {
        h[i] |= ushr(s.l().wrapping_shl(21), 11);
    }
    for i in 0..total_e {
        h[i] |= s.k();
    }
    for _ in 0..total_e {
        let _ = s.k() << 6;
    }
    for _ in 0..total_e {
        let _ = s.k();
    }
    let j_count = s.read_char() as usize;
    let mut j = vec![0i32; j_count];
    for i in 0..j_count {
        j[i] = s.k();
    }
    Ok(AnimSet {
        b,
        c,
        d,
        f,
        g,
        h,
        j,
        bytes_consumed: s.pos,
    })
}

#[derive(Debug, Clone, Copy)]
pub struct AnimFrame {
    pub sprite_idx: usize,
    pub dx: i32,
    pub dy: i32,
    pub transform: i32,
}

pub fn frames_for_action(set: &AnimSet, action: usize) -> Result<Vec<AnimFrame>, RpError> {
    frames_for_action_transform(set, action, 0)
}

/// As `frames_for_action`, but with an extra transform XORed in,
/// as `a.b(Graphics, c[], slot, x, y, transform)` does for the
/// player (`s(Graphics)`: transform = hu ? 0 : 2). The dx/dy sign
/// flips use the FINAL transform, so that `draw_sprite_frame`
/// (which applies the frame anchor flip for that transform)
/// reproduces `c.a` exactly in both facing directions.
pub fn frames_for_action_transform(
    set: &AnimSet,
    action: usize,
    extra_transform: i32,
) -> Result<Vec<AnimFrame>, RpError> {
    frames_for_timeline_frame(set, action, 0, extra_transform)
}

/// Visual composition for one timeline frame of an action:
/// `o(slot)` = D[B[action]>>>16 + timeline]>>>22, then F, then G.
/// For a freshly set slot the timeline frame is 0; after
/// `AnimSlot::advance` it is the slot's visual frame.
pub fn frames_for_timeline_frame(
    set: &AnimSet,
    action: usize,
    timeline: usize,
    extra_transform: i32,
) -> Result<Vec<AnimFrame>, RpError> {
    let b_entry = *set
        .b
        .get(action)
        .ok_or_else(|| RpError(format!("anim action {action} out of range")))?;
    let d_idx = ushr(
        set.d[((b_entry as u32 >> 16) as usize) + timeline],
        22,
    ) as usize;
    let f_entry = *set
        .f
        .get(d_idx)
        .ok_or_else(|| RpError(format!("anim frame-table {d_idx} out of range")))?;
    let offset = (f_entry as u32 >> 16) as usize;
    let count = (f_entry & 0xFF) as usize;
    let mut out = Vec::with_capacity(count);
    for entry in set.g.iter().skip(offset).take(count) {
        let v = *entry;
        let transform = (v & 3) ^ extra_transform;
        let raw_dx = v.wrapping_shl(10).wrapping_shr(22);
        let raw_dy = v.wrapping_shl(20).wrapping_shr(22);
        out.push(AnimFrame {
            sprite_idx: (v as u32 >> 22) as usize,
            dx: if transform & 2 == 0 { raw_dx } else { -raw_dx },
            dy: if transform & 1 == 0 { raw_dy } else { -raw_dy },
            transform,
        });
    }
    Ok(out)
}

/// Draw a type-23 group on top of a tile view, in `e.s()` layer
/// order (layer 0..=7, position order within a layer).
pub fn draw_static_sprites(
    g: &Graphics,
    fb: &mut Framebuffer,
    sprites: &SpriteSet,
    anim: &AnimSet,
    positions: &[(i32, i32)],
    extra: &[u32],
    cam_x: i32,
    cam_y: i32,
) {
    for layer in 0..=7u32 {
        for (p, &(x, y)) in positions.iter().enumerate() {
            let action = match extra.get(2 * p) {
                Some(&a) => a,
                None => continue,
            };
            let e_l = match extra.get(2 * p + 1) {
                Some(&l) => l,
                None => continue,
            };
            if e_l != layer {
                continue;
            }
            let frames = match frames_for_action(anim, action as usize) {
                Ok(f) => f,
                Err(_) => continue,
            };
            for fr in frames {
                if let Some(frame) = sprites.frame(fr.sprite_idx) {
                    draw_sprite_frame(
                        g,
                        fb,
                        &sprites.image,
                        frame,
                        x + fr.dx - cam_x,
                        y + fr.dy - cam_y,
                        fr.transform,
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn load_3073_exact() {
        let data = std::fs::read("../extracted/RP3").unwrap();
        let pack = crate::rp::parse_pack(&data).unwrap();
        let chunk = pack.chunk(1).unwrap();
        let set = load_anim_set(chunk).unwrap();
        assert_eq!(set.b.len(), 235);
        assert_eq!(set.d.len(), 238);
        assert_eq!(set.f.len(), 221);
        assert_eq!(set.g.len(), 1313);
        assert_eq!(set.h.len(), 0);
        assert_eq!(set.j, vec![100]);
        assert_eq!(set.bytes_consumed, chunk.len());
        // Action 103 from the prototype: 16 frames, first sprite 94.
        let frames = frames_for_action(&set, 103).unwrap();
        assert_eq!(frames.len(), 16);
        assert_eq!(frames[0].sprite_idx, 94);
        assert!(frames.iter().all(|f| f.sprite_idx < 224));
    }
    #[test]
    fn load_player_anim_28672_action_0() {
        let data = std::fs::read("../extracted/RP28").unwrap();
        let pack = crate::rp::parse_pack(&data).unwrap();
        let chunk = pack.chunk(0).unwrap();
        let set = load_anim_set(chunk).unwrap();
        assert_eq!(set.bytes_consumed, chunk.len());
        let frames = frames_for_action(&set, 0).unwrap();
        assert!(!frames.is_empty());
        // Sprite set 28673 has its own frame count checked in --scene;
        // here just check indices are in a plausible range (< 512).
        assert!(frames.iter().all(|f| f.sprite_idx < 512));
    }
}

/// One animation slot, as in `a.java` (`L[n*8 + 0..8]`):
/// frame = L[+2], repeat = L[+3], acc = L[+4],
/// prev_dx/dy = L[+5]/L[+6], visual = L[+7].
pub struct AnimSlot {
    pub action: usize,
    pub frame: usize,
    pub repeat: i32,
    pub acc: i32,
    pub prev_dx: i32,
    pub prev_dy: i32,
    pub visual: usize,
}

impl AnimSlot {
    pub fn new(set: &AnimSet, action: usize) -> Self {
        let b_entry = set.b[action];
        AnimSlot {
            action,
            frame: 0,
            repeat: (b_entry >> 10) & 0x3F,
            acc: 0,
            prev_dx: 0,
            prev_dy: 0,
            visual: 0,
        }
    }

    fn d_dx(d: i32) -> i32 {
        d.wrapping_shl(10).wrapping_shr(22)
    }
    fn d_dy(d: i32) -> i32 {
        d.wrapping_shl(20).wrapping_shr(22)
    }

    /// Port of `a.b(int slot, int delta_ms)`: returns the root-motion
    /// delta (a, b) for this tick and updates frame/visual/repeat.
    pub fn advance(&mut self, set: &AnimSet, delta_ms: i32) -> (i32, i32) {
        if self.repeat == 0 {
            return (0, 0);
        }
        let b_entry = set.b[self.action];
        let off = (b_entry as u32 >> 16) as usize;
        let count = (b_entry & 0x3FF) as usize;
        if count == 0 {
            return (0, 0);
        }
        let mut n6 = self.frame;
        let mut n5 = self.repeat;
        let mut n12 = n6;
        let mut n13 = if n6 == self.visual {
            0
        } else {
            (set.c[off + n12] >> 6) & 3
        };
        let mut n2 = self.acc + delta_ms;
        let mut n4 = set.j[(set.c[off + n6] & 0x3F) as usize];
        while n2 >= n4 {
            n2 -= n4;
            if n6 + 1 == count {
                if n5 > 0 {
                    n5 -= 1;
                    if n5 == 0 {
                        n6 -= 1;
                        n2 = n4;
                        break;
                    }
                }
                n6 = 0;
                let last = set.d[off + count - 1];
                self.prev_dx -= Self::d_dx(last);
                self.prev_dy -= Self::d_dy(last);
            } else {
                n6 += 1;
            }
            let n3 = (set.c[off + n6] >> 6) & 3;
            if n3 < n13 {
                // visual stays
            } else {
                n13 = n3;
                n12 = n6;
            }
            n4 = set.j[(set.c[off + n6] & 0x3F) as usize];
        }
        let d_entry = set.d[off + n6];
        let mut n3 = Self::d_dx(d_entry);
        let mut n14 = Self::d_dy(d_entry);
        if (d_entry & 3) > 0 && n5 != 0 && (n6 + 1 != count || n5 != 1) {
            let (next_dx, next_dy) = if n6 + 1 == count {
                let last = set.d[off + count - 1];
                let first = set.d[off];
                (
                    Self::d_dx(last) + Self::d_dx(first),
                    Self::d_dy(last) + Self::d_dy(first),
                )
            } else {
                let next = set.d[off + n6 + 1];
                (Self::d_dx(next), Self::d_dy(next))
            };
            let frac = (n2 << 12) / n4;
            n3 += (next_dx - n3) * frac >> 12;
            n14 += (next_dy - n14) * frac >> 12;
        }
        let a = n3 - self.prev_dx;
        let b = n14 - self.prev_dy;
        self.frame = n6;
        self.repeat = n5;
        self.acc = n2;
        self.prev_dx = n3;
        self.prev_dy = n14;
        self.visual = n12;
        (a, b)
    }
}
