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
pub fn scale_bilinear(src: &Framebuffer, w: usize, h: usize) -> Framebuffer {
    let mut out = Framebuffer::new(w, h);
    if w == 0 || h == 0 || src.width == 0 || src.height == 0 {
        return out;
    }
    for y in 0..h {
        // Centre of the destination pixel in source coordinates.
        let sy = ((y as f64 + 0.5) * src.height as f64 / h as f64 - 0.5)
            .clamp(0.0, (src.height - 1) as f64);
        let y0 = sy.floor() as usize;
        let y1 = (y0 + 1).min(src.height - 1);
        let fy = sy - y0 as f64;
        for x in 0..w {
            let sx = ((x as f64 + 0.5) * src.width as f64 / w as f64 - 0.5)
                .clamp(0.0, (src.width - 1) as f64);
            let x0 = sx.floor() as usize;
            let x1 = (x0 + 1).min(src.width - 1);
            let fx = sx - x0 as f64;
            let p00 = src.pixels[y0 * src.width + x0];
            let p10 = src.pixels[y0 * src.width + x1];
            let p01 = src.pixels[y1 * src.width + x0];
            let p11 = src.pixels[y1 * src.width + x1];
            let mut px = 0u32;
            for shift in [24, 16, 8, 0] {
                let c00 = ((p00 >> shift) & 0xFF) as f64;
                let c10 = ((p10 >> shift) & 0xFF) as f64;
                let c01 = ((p01 >> shift) & 0xFF) as f64;
                let c11 = ((p11 >> shift) & 0xFF) as f64;
                let c = (c00 * (1.0 - fx) + c10 * fx) * (1.0 - fy)
                    + (c01 * (1.0 - fx) + c11 * fx) * fy;
                px |= ((c.round() as u32) & 0xFF) << shift;
            }
            out.pixels[y * w + x] = px;
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
