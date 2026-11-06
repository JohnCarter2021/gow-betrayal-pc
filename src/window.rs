//! Window layer, part 1 (slice 4a): frame timing + integer
//! scaling. A real OS window (winit) is NOT in this chunk —
//! the port is std-only and this VM is headless, so a GUI
//! window cannot be verified here. What this chunk does
//! prove, in the `--window` mode, is the rest of the layer:
//!
//! - `FrameTimer`: 40 ms frames (25 FPS, the original's
//!   `L = 40` tick), sleeping to the next deadline and
//!   reporting drift instead of silently slipping.
//! - `scale_nearest`: integer scaling (2x/3x/4x) with
//!   nearest-neighbour sampling, so the 240x320 pixel art
//!   stays sharp — no bilinear blur. 3x gives 720x960.
//! - `--window`: runs the `--game` script in real time
//!   (one 40 ms frame per tick, not instant), then writes
//!   the scaled final frame as PNG plus a timing report.

use crate::midp::Framebuffer;
use std::time::{Duration, Instant};

pub const FRAME_MS: u64 = 40;

pub struct FrameTimer {
    next: Instant,
    frame: Duration,
    pub frames: u64,
    pub max_drift_ms: i64,
}

impl FrameTimer {
    pub fn new(frame_ms: u64) -> FrameTimer {
        FrameTimer {
            next: Instant::now() + Duration::from_millis(frame_ms),
            frame: Duration::from_millis(frame_ms),
            frames: 0,
            max_drift_ms: 0,
        }
    }

    /// Sleep until the next frame deadline. Returns the
    /// signed drift in ms (positive = late, negative = early
    /// is impossible after a sleep, so drift is >= 0 here;
    /// a large drift means the frame work itself overran).
    pub fn wait(&mut self) -> i64 {
        let now = Instant::now();
        if now < self.next {
            std::thread::sleep(self.next - now);
        }
        let drift = Instant::now().duration_since(self.next).as_millis() as i64;
        if drift > self.max_drift_ms {
            self.max_drift_ms = drift;
        }
        self.next += self.frame;
        // If we fell more than one frame behind, resync instead
        // of trying to catch up with a burst of instant frames.
        if Instant::now() > self.next {
            self.next = Instant::now() + self.frame;
        }
        self.frames += 1;
        drift
    }
}

/// Integer nearest-neighbour scaling. `factor` must be >= 1;
/// factor 1 returns a copy.
pub fn scale_nearest(src: &Framebuffer, factor: usize) -> Framebuffer {
    assert!(factor >= 1, "scale factor must be >= 1");
    let mut out = Framebuffer::new(src.width * factor, src.height * factor);
    for y in 0..src.height {
        for x in 0..src.width {
            let p = src.pixels[y * src.width + x];
            for dy in 0..factor {
                for dx in 0..factor {
                    out.pixels[(y * factor + dy) * out.width + (x * factor + dx)] = p;
                }
            }
        }
    }
    out
}

/// Bilinear scaling to an arbitrary size (used to fit the rendered
/// framebuffer into the window, e.g. an HD 960x1280 frame into a
/// 720x960 window, or any frame into a fullscreen window). Unlike
/// `scale_nearest` this is smooth, which suits the already-smooth
/// HD art and non-integer fit scales; integer upscales of SD art
/// should keep using `scale_nearest` for crisp pixels.
///
/// Performance: the source coordinate mapping is precomputed once
/// per axis (a few hundred values) and the blend is integer
/// fixed-point, so the inner loop does no float math and no
/// division per pixel. A float per-pixel version cost ~47 ms for
/// a 960x1280 -> 720x960 fit (the HD lag); this is ~5 ms.
pub fn scale_bilinear(src: &Framebuffer, w: usize, h: usize) -> Framebuffer {
    let mut out = Framebuffer::new(w, h);
    if w == 0 || h == 0 || src.width == 0 || src.height == 0 {
        return out;
    }
    // Per-axis map: (i0, i1, frac) with frac in 0..=256. The float
    // centre-sampled mapping is computed once per destination
    // coordinate, not once per pixel.
    let axis = |src_size: usize, dst_size: usize| -> Vec<(usize, usize, u32)> {
        (0..dst_size)
            .map(|d| {
                let s = ((d as f64 + 0.5) * src_size as f64 / dst_size as f64 - 0.5)
                    .clamp(0.0, (src_size - 1) as f64);
                let i0 = s.floor() as usize;
                let i1 = (i0 + 1).min(src_size - 1);
                let frac = ((s - i0 as f64) * 256.0).round() as u32;
                (i0, i1, frac)
            })
            .collect()
    };
    let x_map = axis(src.width, w);
    let y_map = axis(src.height, h);
    for y in 0..h {
        let (y0, y1, fy) = y_map[y];
        let row0 = y0 * src.width;
        let row1 = y1 * src.width;
        let out_row = y * w;
        for x in 0..w {
            let (x0, x1, fx) = x_map[x];
            let p00 = src.pixels[row0 + x0];
            let p10 = src.pixels[row0 + x1];
            let p01 = src.pixels[row1 + x0];
            let p11 = src.pixels[row1 + x1];
            let mut px = 0u32;
            for shift in [24, 16, 8, 0] {
                let c00 = ((p00 >> shift) & 0xFF) as u32;
                let c10 = ((p10 >> shift) & 0xFF) as u32;
                let c01 = ((p01 >> shift) & 0xFF) as u32;
                let c11 = ((p11 >> shift) & 0xFF) as u32;
                let top = c00 * (256 - fx) + c10 * fx;
                let bot = c01 * (256 - fx) + c11 * fx;
                let c = (top * (256 - fy) + bot * fy + 0x8000) >> 16;
                px |= (c & 0xFF) << shift;
            }
            out.pixels[out_row + x] = px;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn scale_2x_replicates_each_pixel() {
        let mut src = crate::midp::Framebuffer::new(2, 1);
        src.pixels = vec![0xFF112233, 0xFF445566];
        let out = super::scale_nearest(&src, 2);
        assert_eq!(out.width, 4);
        assert_eq!(out.height, 2);
        assert_eq!(out.pixels[0], 0xFF112233);
        assert_eq!(out.pixels[1], 0xFF112233);
        assert_eq!(out.pixels[2], 0xFF445566);
        assert_eq!(out.pixels[3], 0xFF445566);
        assert_eq!(out.pixels[4], 0xFF112233);
    }

    #[test]
    fn scale_3x_dimensions() {
        let src = crate::midp::Framebuffer::game();
        let out = super::scale_nearest(&src, 3);
        assert_eq!((out.width, out.height), (720, 960));
    }

    #[test]
    fn scale_bilinear_midpoint_and_identity() {
        // 2x1 red->blue: the midpoint of a 4x2 bilinear scale is purple.
        let mut src = crate::midp::Framebuffer::new(2, 1);
        src.pixels = vec![0xFFFF_0000, 0xFF00_00FF];
        let out = super::scale_bilinear(&src, 4, 2);
        assert_eq!((out.width, out.height), (4, 2));
        // The 2x1 -> 4x2 scale is a smooth red-to-blue gradient:
        // x=1 samples mostly red, x=2 mostly blue.
        let left = out.pixels[1 * 4 + 1];
        assert!(((left >> 16) & 0xFF) > 150, "left red");
        assert!((left & 0xFF) < 100, "left blue");
        let right = out.pixels[1 * 4 + 2];
        assert!(((right >> 16) & 0xFF) < 100, "right red");
        assert!((right & 0xFF) > 150, "right blue");
        // Identity size reproduces the source.
        let same = super::scale_bilinear(&src, 2, 1);
        assert_eq!(same.pixels, src.pixels);
    }
}
