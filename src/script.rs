//! Script triggers and script playback (slice 13).
//!
//! Traced from `decompiled/e.java`:
//! - Type-8 entities are script triggers (`d()` case 8):
//!   per item, tile `(bs/16, bt/16)` gets tile flag 128
//!   (`b(x, y, 128)`), script start `cn = m(10)`,
//!   length `co = m(3)`. The per-frame gameplay update
//!   (`z()`) fires when the player stands on a flag-128
//!   tile: `n7 = ga/16`, `n8 = gb/16` (the `(hi >> 8) + 1
//!   >= 16` branch never fires, since `hi = gb % 16`).
//!   Firing clears flag 128 for every trigger sharing the
//!   same `cn` (the `c(cl, cm, 128)` loop), so a script
//!   fires once.
//! - `p()` shows string `cs`: a leading `!` plays
//!   cutscene actions instead of a dialog (state 105,
//!   `s(aY[ba])` / `i(aZ[bb], dL)` from the type-6 action
//!   tables); a leading `^` switches weapon/magic
//!   (`fM`, `hb`/`hc`/`hd`) and then shows the rest as
//!   dialog text; a lone `*` sets `be` (level advance).
//! - `C()` advances: `cs += 1`; if `cs >= cu` the script
//!   ends and gameplay resumes; else `p()` shows the next
//!   string, and a `*` there advances the level
//!   (`++bC`, `bD` update, `bA = bB[bC]`, state 109).
//! - A trigger whose first string starts with `@` sets
//!   the boss-bar latch (`aR`) instead of a dialog.
//!
//! Port approximations (documented, not silent):
//! - `!` cutscene steps auto-advance: the type-6 action
//!   tables and state-105 cutscene playback are not
//!   ported, so a `!` string plays no actions and costs
//!   no input. Dialog steps still need one Select each.
//! - `^` weapon/magic switches are not applied (no
//!   weapons in the port yet); the text after the
//!   2-char `^` + digit prefix is shown as dialog.
//! - `@` triggers are consumed with no visible effect
//!   (boss bars are not ported; see slice 14).
//! - Level advance skips the original's state-109
//!   interstitial (Select -> 106, Exit -> menu): the
//!   next level loads immediately, or the main menu
//!   when the last level completes.
//! - The script dialog box reuses the audio-prompt
//!   geometry (12, 30, 211, 270, lines from y = 55);
//!   the original script-dialog box arguments
//!   `(8, 0, 211, z*5+A, 30)` are untraced.

/// One script trigger: a tile that starts a script.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptTrigger {
    pub tile_x: i32,
    pub tile_y: i32,
    /// First string index (`cn`).
    pub start: usize,
    /// Script length in strings (`co`); strings are
    /// `start .. start + len`.
    pub len: usize,
}

/// What script playback produces for the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptEvent {
    /// A dialog step is now showing this text.
    Dialog(String),
    /// The script ended without a level advance.
    Ended,
    /// A lone `*` was reached: advance the level.
    LevelComplete,
    /// Nothing is showing (no active script).
    None,
}

struct ActiveScript {
    /// Current string index (`cs`).
    current: usize,
    /// End index, exclusive (`cu = cn + co`).
    end: usize,
}

pub struct ScriptEngine {
    triggers: Vec<ScriptTrigger>,
    strings: Vec<String>,
    active: Option<ActiveScript>,
    /// Current dialog text while a dialog step shows.
    dialog_text: Option<String>,
    /// Weapon mode set by the most recent shown
    /// `^` step (slice 17): digit d > 3 means
    /// mode d - 4 (4 blades, 5 Artemis, 6/7
    /// magic). Taken by the caller after the step
    /// shows; None when no switch is pending.
    pending_weapon: Option<i32>,
}

impl ScriptEngine {
    pub fn new(triggers: Vec<ScriptTrigger>, strings: Vec<String>) -> ScriptEngine {
        ScriptEngine {
            triggers,
            strings,
            active: None,
            dialog_text: None,
            pending_weapon: None,
        }
    }

    /// Take a weapon switch recorded by the most
    /// recently shown `^` step, if any.
    pub fn take_weapon_switch(&mut self) -> Option<i32> {
        self.pending_weapon.take()
    }

    pub fn set_strings(&mut self, strings: Vec<String>) {
        self.strings = strings;
    }

    pub fn active(&self) -> bool {
        self.active.is_some()
    }

    pub fn dialog_text(&self) -> Option<&str> {
        self.dialog_text.as_deref()
    }

    /// Fire the trigger on this tile, if any. Returns
    /// the first script event (dialog, level-complete
    /// for a lone-`*` script, or none for `@` triggers).
    pub fn check_trigger(&mut self, tile_x: i32, tile_y: i32) -> ScriptEvent {
        if self.active.is_some() {
            return ScriptEvent::None;
        }
        let Some(t) = self
            .triggers
            .iter()
            .find(|t| t.tile_x == tile_x && t.tile_y == tile_y)
            .copied()
        else {
            return ScriptEvent::None;
        };
        // Fire once: drop every trigger sharing cn,
        // like the original's c(cl, cm, 128) loop.
        let start = t.start;
        self.triggers.retain(|o| o.start != start);
        let first = self.strings.get(start).cloned().unwrap_or_default();
        if first.starts_with('@') {
            // Boss-bar latch: consumed, no dialog.
            return ScriptEvent::None;
        }
        self.active = Some(ActiveScript {
            current: start,
            end: start + t.len,
        });
        self.show_current()
    }

    /// Advance one script step (Select key): `C()`.
    pub fn advance(&mut self) -> ScriptEvent {
        let Some(a) = &mut self.active else {
            return ScriptEvent::None;
        };
        a.current += 1;
        self.show_current()
    }

    /// Cancel the active script (Exit key): back to
    /// gameplay, trigger already consumed.
    pub fn cancel(&mut self) {
        self.active = None;
        self.dialog_text = None;
    }

    /// Show the current string, auto-advancing past
    /// `!` cutscene steps, or finish the script.
    fn show_current(&mut self) -> ScriptEvent {
        loop {
            let Some(a) = &self.active else {
                return ScriptEvent::None;
            };
            if a.current >= a.end {
                self.active = None;
                self.dialog_text = None;
                return ScriptEvent::Ended;
            }
            let text = self
                .strings
                .get(a.current)
                .cloned()
                .unwrap_or_default();
            if text == "*" {
                self.active = None;
                self.dialog_text = None;
                return ScriptEvent::LevelComplete;
            }
            if text.starts_with('!') {
                // Cutscene step: no actions ported,
                // auto-advance without input.
                self.active.as_mut().unwrap().current += 1;
                continue;
            }
            if text.starts_with('^') {
                // Weapon/magic switch + dialog (slice
                // 17, p()): the 2nd char is a digit d;
                // d > 3 switches mode to d - 4 (4
                // blades, 5 Artemis, 6/7 magic), and
                // the dialog shows the text minus the
                // '^' and its digit (char-safe strip).
                let mut chars = text.chars();
                chars.next();
                if let Some(d) = chars.next().and_then(|c| c.to_digit(10)) {
                    if d > 3 {
                        self.pending_weapon = Some(d as i32 - 4);
                    }
                }
                let shown: String = text.chars().skip(2).collect();
                self.dialog_text = Some(shown.clone());
                return ScriptEvent::Dialog(shown);
            }
            if let Some(stripped) = text.strip_prefix('*') {
                // '*' with text: original shows a dialog
                // after a camera move; port shows the
                // text minus the marker.
                let shown = stripped.to_string();
                self.dialog_text = Some(shown.clone());
                return ScriptEvent::Dialog(shown);
            }
            self.dialog_text = Some(text.clone());
            return ScriptEvent::Dialog(text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine(triggers: Vec<ScriptTrigger>, strings: Vec<&str>) -> ScriptEngine {
        ScriptEngine::new(
            triggers,
            strings.into_iter().map(|s| s.to_string()).collect(),
        )
    }

    #[test]
    fn caret_digit_switches_weapon_mode() {
        // `^` digits above 3 switch weapons (slice
        // 17): 4 blades, 5 Artemis, 6 magic; 3 and
        // below are plain dialog markers.
        let mut e = engine(
            vec![ScriptTrigger {
                tile_x: 1,
                tile_y: 2,
                start: 0,
                len: 4,
            }],
            vec![
                "^5Artemis!",
                "^4Blades!",
                "^6Magic!",
                "^3Marker only",
            ],
        );
        assert_eq!(
            e.check_trigger(1, 2),
            ScriptEvent::Dialog("Artemis!".to_string())
        );
        assert_eq!(e.take_weapon_switch(), Some(1));
        assert_eq!(
            e.advance(),
            ScriptEvent::Dialog("Blades!".to_string())
        );
        assert_eq!(e.take_weapon_switch(), Some(0));
        assert_eq!(
            e.advance(),
            ScriptEvent::Dialog("Magic!".to_string())
        );
        assert_eq!(e.take_weapon_switch(), Some(2));
        assert_eq!(
            e.advance(),
            ScriptEvent::Dialog("Marker only".to_string())
        );
        assert_eq!(e.take_weapon_switch(), None);
    }

    #[test]
    fn caret_strips_prefix_and_shows_dialog() {
        let mut e = engine(
            vec![ScriptTrigger {
                tile_x: 1,
                tile_y: 2,
                start: 0,
                len: 1,
            }],
            vec!["^1The enemies were crushed."],
        );
        assert_eq!(
            e.check_trigger(1, 2),
            ScriptEvent::Dialog("The enemies were crushed.".to_string())
        );
        assert!(e.active());
    }

    #[test]
    fn bang_auto_advances_and_star_completes() {
        // Level-1 end script shape: [^dialog, !, *].
        let mut e = engine(
            vec![ScriptTrigger {
                tile_x: 214,
                tile_y: 30,
                start: 0,
                len: 3,
            }],
            vec!["^1Temple crushed.", "!", "*"],
        );
        assert_eq!(
            e.check_trigger(214, 30),
            ScriptEvent::Dialog("Temple crushed.".to_string())
        );
        // One Select skips the '!' and hits '*'.
        assert_eq!(e.advance(), ScriptEvent::LevelComplete);
        assert!(!e.active());
    }

    #[test]
    fn script_end_without_star_resumes_gameplay() {
        let mut e = engine(
            vec![ScriptTrigger {
                tile_x: 0,
                tile_y: 0,
                start: 0,
                len: 2,
            }],
            vec!["First.", "Second."],
        );
        assert_eq!(
            e.check_trigger(0, 0),
            ScriptEvent::Dialog("First.".to_string())
        );
        assert_eq!(
            e.advance(),
            ScriptEvent::Dialog("Second.".to_string())
        );
        assert_eq!(e.advance(), ScriptEvent::Ended);
    }

    #[test]
    fn at_trigger_consumes_without_dialog() {
        let mut e = engine(
            vec![ScriptTrigger {
                tile_x: 3,
                tile_y: 4,
                start: 0,
                len: 1,
            }],
            vec!["@boss"],
        );
        assert_eq!(e.check_trigger(3, 4), ScriptEvent::None);
        assert!(!e.active());
        // Consumed: firing again finds nothing.
        assert_eq!(e.check_trigger(3, 4), ScriptEvent::None);
    }

    #[test]
    fn shared_start_triggers_fire_once() {
        let mut e = engine(
            vec![
                ScriptTrigger {
                    tile_x: 214,
                    tile_y: 30,
                    start: 0,
                    len: 1,
                },
                ScriptTrigger {
                    tile_x: 214,
                    tile_y: 31,
                    start: 0,
                    len: 1,
                },
            ],
            vec!["Hello."],
        );
        assert_eq!(
            e.check_trigger(214, 30),
            ScriptEvent::Dialog("Hello.".to_string())
        );
        e.cancel();
        // Same cn on the neighbouring tile is gone.
        assert_eq!(e.check_trigger(214, 31), ScriptEvent::None);
    }
}
