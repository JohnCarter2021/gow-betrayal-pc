//! Main menu table, from `e.J()` / `e.H()` / `e.K()` / `e.w()` in
//! `decompiled/e.java`.
//!
//! The table is chunk 1028 (RP1 chunk 4, flag 255), 32 entries, each a
//! big-endian i32:
//!   bits 31..26: type (>> 26, Java arithmetic shift — the entries here
//!     are all positive, so unsigned shift gives the same value)
//!   bits 15..8:  next menu index (>> 8 & 0xFF)
//!   bits 7..0:   string index
//!   bit 24 (0x1000000): visible in the draw loop
//!   bit 25 (0x2000000): next-item link for k()/l() navigation
//!   bit 22 (0x400000):  terminator of a menu range
//!   bit 23 (0x800000):  first item of a menu range (prev-stop for l())
//!
//! Decoded root menu (indices 0..7, terminator 8):
//!   0 Play Game (type 0, next 9), 1 Arena (6), 2 Help (3),
//!   3 Options (0, next 19), 4 About (4), 5 gameroom (0, next 25),
//!   6 More Games (0, next 30), 7 Exit (0, next 23)
//! Options submenu (19..22, terminator 22 is itself a header):
//!   19 Sound (1, audio toggle), 20 Clear Save (0, next 30),
//!   21 Language (12, cycles language), 22 Options (7, header)
//! Level submenu (9..18, type 5): For Sparta, Dead Rider, Minotaur,
//!   Garden, The Beast, Ruins, Catacombs, Labyrinth, The Tower, Messenger.
//!   These start invisible (bits 24/25 clear); `H()` sets bits 24+25
//!   (0x3000000) for levels up to the save progress `bD`, so only
//!   "For Sparta" is visible on a fresh save.
//! Confirm submenu (30..31): 30 Are you sure? (11, Clear Save),
//!   31 Clear Save (7, header). Gameroom submenu (25..29): RECOMMEND,
//!   RATE GAME, LEADERBOARD, visit game lobby, header.
//!
//! Navigation is LEFT/RIGHT (s = 2 / 5), not up/down: `k()` finds the
//! next item with bit 25, `l()` the previous one, both implemented
//! exactly as in `e.java` (with underflow/overflow guards the original
//! does not need because its tables always terminate).
//!
//! The menu is a carousel: `w()` draws only the selected item, centred,
//! with left/right arrows when there is a previous/next item. That is
//! why `draw` in game.rs draws one item, not a list.

pub struct Menu {
    pub items: Vec<u32>,
    pub ix: usize,
    /// Previous menu indices, for Back (Exit softkey). The original
    /// uses `iy`/`iA` bottom-bar state for this; a stack gives the same
    /// visible behaviour (root -> submenu -> root) and is documented
    /// as a slice-2 simplification.
    pub stack: Vec<usize>,
}

impl Menu {
    pub fn from_chunk(chunk: &[u8]) -> Menu {
        let mut items = Vec::new();
        for i in (0..chunk.len()).step_by(4) {
            if i + 4 > chunk.len() {
                break;
            }
            items.push(u32::from_be_bytes([chunk[i], chunk[i + 1], chunk[i + 2], chunk[i + 3]]));
        }
        Menu {
            items,
            ix: 0,
            stack: Vec::new(),
        }
    }

    pub fn entry(&self, index: usize) -> Option<(u32, usize, usize)> {
        // (type, next, string_index)
        self.items.get(index).map(|&v| {
            ((v >> 26) & 0x3F, ((v >> 8) & 0xFF) as usize, (v & 0xFF) as usize)
        })
    }

    /// `H()`: unlock levels up to `progress` (bD).
    pub fn apply_progress(&mut self, progress: usize) {
        for n in 0..10 {
            let idx = 9 + n;
            if idx >= self.items.len() {
                break;
            }
            if n <= progress {
                self.items[idx] |= 0x3000000;
            } else {
                self.items[idx] &= 0xFCFF_FFFF;
            }
        }
    }

    /// `k(n, flag)` from e.java.
    pub fn k(&self, n: usize, flag: u32) -> usize {
        let orig = n;
        let mut cur = n;
        loop {
            if cur >= self.items.len() || (self.items[cur] & 0x400000) != 0 {
                return orig;
            }
            cur += 1;
            if cur >= self.items.len() {
                return orig;
            }
            if (self.items[cur] & flag) != 0 {
                return cur;
            }
            if (self.items[cur] & 0x400000) != 0 {
                return orig;
            }
        }
    }

    /// `l(n, flag)` from e.java, with underflow guards.
    pub fn l(&self, n: usize, flag: u32) -> usize {
        let mut candidate = n;
        let mut cur = n as isize;
        loop {
            if cur < 0 || cur as usize >= self.items.len() {
                return candidate;
            }
            let cur_u = cur as usize;
            if (self.items[cur_u] & 0x800000) == 0 {
                cur -= 1;
                if cur < 0 {
                    return candidate;
                }
                if (self.items[cur as usize] & flag) != 0 {
                    candidate = cur as usize;
                }
            }
            let cur_u = cur as usize;
            if (self.items[cur_u] & 0x800000) == 0 || (self.items[cur_u] & flag) != 0 {
                if (self.items[cur_u] & flag) == 0 {
                    continue;
                } else {
                    return candidate;
                }
            } else {
                return candidate;
            }
        }
    }

    /// Start index of the current menu range, from `w()`'s iG logic.
    pub fn menu_start(&self) -> usize {
        let mut n3 = self.ix as isize;
        let mut n4 = 0usize;
        while n3 == 1 || (n3 > 1 && (self.items[n3 as usize] & 0x800000) == 0) {
            n3 -= 1;
            if n3 < 0 {
                break;
            }
            if (self.items[n3 as usize] & 0x1000000) == 0 {
                continue;
            }
            n4 += 1;
        }
        let mut cur = self.ix;
        for _ in 0..n4 {
            cur = self.l(cur, 0x1000000);
        }
        cur
    }

    /// End index (iH) of the current menu range, from `w()`.
    pub fn menu_end(&self) -> usize {
        let start = self.menu_start();
        // n4 is recomputed the same way as in menu_start.
        let mut n3 = self.ix as isize;
        let mut n4 = 0usize;
        while n3 == 1 || (n3 > 1 && (self.items[n3 as usize] & 0x800000) == 0) {
            n3 -= 1;
            if n3 < 0 {
                break;
            }
            if (self.items[n3 as usize] & 0x1000000) == 0 {
                continue;
            }
            n4 += 1;
        }
        let mut cur = start;
        for _ in 0..n4 {
            cur = self.k(cur, 0x2000000);
        }
        cur
    }

    /// Last item with next-flag in the range starting at `start`
    /// (n5 in w()), used for the right-arrow condition.
    pub fn range_last_next(&self, start: usize) -> usize {
        let mut n5 = start;
        let mut n8 = start;
        loop {
            if n8 >= self.items.len() {
                break;
            }
            if (self.items[n8] & 0x2000000) != 0 {
                n5 = n8;
            }
            if (self.items[n8] & 0x400000) != 0 {
                break;
            }
            n8 += 1;
        }
        n5
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn test_menu() -> Menu {
        // Build from the real chunk via Resources in the game test; here
        // use a synthetic root menu matching the decoded table's flags.
        let mut m = Menu {
            items: vec![
                0x03800926, 0x1b000042, 0x0f000028, 0x03001329, 0x1300002a, 0x03001943,
                0x03001e05, 0x03001701, 0x1d400034,
            ],
            ix: 0,
            stack: Vec::new(),
        };
        m.apply_progress(0);
        m
    }
    #[test]
    fn navigation_root() {
        let m = test_menu();
        assert_eq!(m.k(0, 0x2000000), 1);
        assert_eq!(m.k(7, 0x2000000), 7); // terminator at 8 stops it
        assert_eq!(m.l(3, 0x1000000), 2);
        assert_eq!(m.l(0, 0x1000000), 0);
        assert_eq!(m.menu_start(), 0);
    }
    #[test]
    fn level_visibility() {
        let mut m = test_menu();
        // Extend with level items as in the real table (invisible).
        m.items.extend_from_slice(&[0x14800036, 0x14000037, 0x1440003f]);
        m.apply_progress(0);
        assert_eq!(m.items[9] & 0x3000000, 0x3000000);
        assert_eq!(m.items[10] & 0x3000000, 0);
        m.apply_progress(2);
        assert_eq!(m.items[11] & 0x3000000, 0x3000000);
    }
}
