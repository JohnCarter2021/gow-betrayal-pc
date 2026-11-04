//! Bitmap font, from `a.a(c[])` / `a.a(Graphics, char[], ...)` /
//! `a.c()` / `a.b()` in `decompiled/a.java`.
//!
//! A font is a sprite set whose frames are glyphs. When the set's 4th
//! frame has `extra_h == 255` (the original's `cArray[3].h == 255` test),
//! `a(c[])` rewrites every glyph: the anchor becomes the old (e, f),
//! and `e` becomes a running sum of `(old_anchor_x << 8) | old_anchor_y`,
//! which is the character code — glyphs are then sorted by `e`, so
//! lookup is a binary search by char code. That transform is applied
//! here, in `Font::from_sprite_set`.
//!
//! Metrics: height = glyphs[0].extra_h, baseline = glyphs[1].extra_h,
//! missing-glyph advance = glyphs[2].extra_h, glyph advance = glyph.g.
//! Text draw anchors (MIDP): HCENTER=1, VCENTER=2, LEFT=4, RIGHT=8,
//! TOP=16, BOTTOM=32, BASELINE=64 — exactly as in `a.a(Graphics, ...)`.

use crate::midp::{draw_sprite_frame, Framebuffer, Graphics};
use crate::resources::SpriteSet;
use crate::sprite::Frame;

pub struct Font {
    pub image: crate::midp::Image,
    pub glyphs: Vec<Frame>,
}

impl Font {
    pub fn from_sprite_set(set: SpriteSet) -> Font {
        let mut glyphs = set.frames;
        if glyphs.len() > 3 && glyphs[3].extra_h == 255 {
            let mut code = 0i32;
            for g in glyphs.iter_mut() {
                code += (g.anchor_x << 8) + (g.anchor_y & 0xFF);
                g.anchor_x = g.e;
                g.anchor_y = g.f;
                g.e = code;
            }
        }
        Font {
            image: set.image,
            glyphs,
        }
    }

    pub fn height(&self) -> i32 {
        self.glyphs.first().map(|g| g.extra_h).unwrap_or(0)
    }
    pub fn baseline(&self) -> i32 {
        self.glyphs.get(1).map(|g| g.extra_h).unwrap_or(0)
    }
    fn missing_advance(&self) -> i32 {
        self.glyphs.get(2).map(|g| g.extra_h).unwrap_or(0)
    }

    pub fn glyph(&self, ch: char) -> Option<&Frame> {
        let code = ch as i32;
        // Glyphs are sorted by e (char code) after the transform.
        self.glyphs
            .binary_search_by_key(&code, |g| g.e)
            .ok()
            .map(|i| &self.glyphs[i])
    }

    pub fn width(&self, text: &str) -> i32 {
        text.chars()
            .map(|ch| self.glyph(ch).map(|g| g.g).unwrap_or_else(|| self.missing_advance()))
            .sum()
    }

    /// Draw text, following `a.a(Graphics, char[], n, n2, n3, n4, n5)`:
    /// anchor bits adjust x (HCENTER/RIGHT) and y (BOTTOM/BASELINE),
    /// then each glyph is drawn and x advances by its `g`.
    pub fn draw(&self, g: &Graphics, fb: &mut Framebuffer, text: &str, x: i32, y: i32, anchor: i32) {
        let mut dx = x;
        let mut dy = y;
        if anchor & 0x40 != 0 {
            dy -= self.baseline();
        } else if anchor & 0x20 != 0 {
            dy -= self.height();
        }
        if anchor & 9 != 0 {
            dx -= self.width(text) >> (anchor & 1);
        }
        for ch in text.chars() {
            if let Some(glyph) = self.glyph(ch) {
                draw_sprite_frame(g, fb, &self.image, glyph, dx, dy, 0);
                dx += glyph.g;
            } else {
                dx += self.missing_advance();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn font_transform_and_width() {
        // RP1 chunk 7 is the font def (1031); its image is chunk 19.
        let mut res = crate::resources::Resources::new("../extracted");
        let set = res.sprite_set(1031, None).unwrap();
        assert_eq!(set.frames.len(), 150);
        let font = Font::from_sprite_set(set);
        assert_eq!(font.height(), 12);
        // After the transform, glyph e values are char codes, sorted.
        assert!(font.glyphs.windows(2).all(|w| w[0].e <= w[1].e));
        // 'A' (65) should exist and 'W' width is used by the game (iJ = a("W")*4).
        assert!(font.glyph('A').is_some());
        assert!(font.width("W") > 0);
        assert_eq!(font.width(""), 0);
    }
}
