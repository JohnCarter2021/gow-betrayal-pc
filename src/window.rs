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
}
