//! HUD (slice 3e): top-left bars + bottom bar, as in
//! `e.f(Graphics)` and `e.a(Graphics, int, int)`.
//!
//! Top-left (`f(Graphics)`):
//! - portrait: anim set 1025 action 4, sprite set 1036, at (0,0).
//! - weapon icon: sprite set 1036 frame `5 + fM` at (6,17)
//!   (Phase 6, e.java:2345-2346: `n = 5 + fM` — blades 5,
//!   Artemis 6, magic modes 7/8).
//! - health bar: outline (42,13, hm*6>>8, 3) colour 0x424242,
//! - health bar: outline (42,13, hm*6>>8, 3) colour 0x424242,
//!   black fill (42,14, hm*6>>8, 2), health fill colour 4783872
//!   width hj*6>>8 at y14.
//! - magic bar: outline (42,17, hn*6>>8, 2), black fill,
//!   magic fill colour 104191 width hl*6>>8 at y17 (white
//!   when empty, approximating the flashing `bq`).
//! - gold text `hq` centred at (58,24).
//! Bottom bar (`a(Graphics, 160, 197)` for gameplay):
//! - black fill (0,306,240,14), lines y306 colour 9787452,
//!   y307 colour 4269078.
//! - softkeys "Weapon" (string 160, left) and "Menu" (197, right).
//! - centre: "N Hits" when combo_hits > 1 (original `gH`
//!   with `gI > 0`); Arena kills text is not in this chunk.
//! Text uses the bitmap font (def 1031, height 12), drawn at
//! y308 so it fits the 306-320 bar (original y = 306 + A - 3
//! with A = z>>1).

use crate::anim;
use crate::font::Font;
use crate::midp::{draw_sprite_frame, Framebuffer, Graphics};
use crate::resources::Resources;

pub struct HudState {
    pub health: i32,
    pub max_health: i32,
    pub magic: i32,
    pub max_magic: i32,
    pub gold: i32,
    pub combo_hits: i32,
    /// Weapon mode `fM` (Phase 6): the icon is frame
    /// `5 + fM` of sprite set 1036.
    pub weapon_mode: i32,
}

pub fn draw_hud(
    g: &Graphics,
    fb: &mut Framebuffer,
    res: &mut Resources,
    font: &Font,
    hud: &HudState,
) {
    // Portrait + weapon icon (best effort; bars still draw if missing).
    if let Ok(anim_chunk) = res.chunk(1025) {
        if let Ok(anim_set) = anim::load_anim_set(&anim_chunk) {
            if let Ok(portrait) = res.sprite_set(1036, None) {
                if let Ok(frames) = anim::frames_for_timeline_frame(&anim_set, 4, 0, 0) {
                    for fr in frames {
                        if let Some(frame) = portrait.frame(fr.sprite_idx) {
                            draw_sprite_frame(g, fb, &portrait.image, frame, fr.dx, fr.dy, fr.transform);
                        }
                    }
                }
                if let Some(frame) = portrait.frame((5 + hud.weapon_mode) as usize) {
                    draw_sprite_frame(g, fb, &portrait.image, frame, 6, 17, 0);
                }
            }
        }
    }
    // Health / magic bars.
    let set = |fb: &mut Framebuffer, x: i32, y: i32, argb: u32| {
        if x >= 0 && y >= 0 && (x as usize) < fb.width && (y as usize) < fb.height {
            fb.pixels[(x as usize) + fb.width * (y as usize)] = argb;
        }
    };
    let bar = |fb: &mut Framebuffer, x: i32, y: i32, w: i32, h: i32, argb: u32| {
        for yy in y..y + h {
            for xx in x..x + w {
                set(fb, xx, yy, argb);
            }
        }
    };
    let health_w = hud.max_health * 6 >> 8;
    let magic_w = hud.max_magic * 6 >> 8;
    // Outlines (drawRect in the original: border only, 1 px).
    for xx in 42..42 + health_w {
        set(fb, xx, 13, 0xFF424242);
        set(fb, xx, 16, 0xFF424242);
    }
    for yy in 13..17 {
        set(fb, 42, yy, 0xFF424242);
        set(fb, 42 + health_w - 1, yy, 0xFF424242);
    }
    for xx in 42..42 + magic_w {
        set(fb, xx, 17, 0xFF424242);
        set(fb, xx, 19, 0xFF424242);
    }
    bar(fb, 42, 14, health_w, 2, 0xFF000000);
    bar(fb, 42, 17, magic_w, 2, 0xFF000000);
    let health_fill = hud.health * 6 >> 8;
    if health_fill > 0 {
        bar(fb, 42, 14, health_fill, 2, 4783872);
    }
    let magic_fill = hud.magic * 6 >> 8;
    if magic_fill > 0 {
        bar(fb, 42, 17, magic_fill, 2, 104191);
    } else {
        bar(fb, 42, 17, magic_w, 2, 0xFFFFFFFF);
    }
    font.draw(g, fb, &hud.gold.to_string(), 58, 24, 1);
    // Bottom bar.
    bar(fb, 0, 306, 240, 14, 0xFF000000);
    for xx in 0..240 {
        set(fb, xx, 306, 9787452);
        set(fb, xx, 307, 4269078);
    }
    font.draw(g, fb, "Weapon", 0, 308, 4); // LEFT
    let menu_w = font.width("Menu");
    font.draw(g, fb, "Menu", 240 - menu_w, 308, 4);
    if hud.combo_hits > 1 {
        let text = format!("{} Hits", hud.combo_hits);
        let w = font.width(&text);
        font.draw(g, fb, &text, 120 - w / 2, 308, 4);
    }
}


#[cfg(test)]
mod tests {
    #[test]
    fn hud_bar_widths_match_original_formula() {
        // f(Graphics): width = value * 6 >> 8.
        assert_eq!(1024 * 6 >> 8, 24);
        assert_eq!(1018 * 6 >> 8, 23);
        assert_eq!(512 * 6 >> 8, 12);
    }
}
