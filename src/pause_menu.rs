//! In-game pause menu, state 102 (Phase 5, rest).
//!
//! Trace (from `decompiled/e.java`):
//!
//! * Open (e.java ~740): pressing the menu key
//!   during gameplay (state 100, and the other
//!   gameplay states) sets `I = 102` (and saves the
//!   previous state in `J`); the world freezes.
//! * Pages (e.java ~1284-1318): `ar` is the page,
//!   1-4. Up (`s == 2`) increments `ar` (clamped
//!   to 4); Down (`s == 5`) decrements it (clamped
//!   to 1). `as` is the confirm sub-page.
//! * Select / Attack (`s == 8` or `s == 27`):
//!   - `ar == 4`: toggle sound (`iM.a(!iM.b())`),
//!     `iL = iM.b()`.
//!   - `ar == 2 && as`: quit to main menu
//!     (`cr = -1`, `ao = true`) — save + unload +
//!     state 80.
//!   - `ar == 1 && as`: exit the game (`an = true`)
//!     — save + `M()` (quit the app).
//!   - `ar == 2 || ar == 1` (not `as`): `as = true`
//!     (enter the confirm sub-page).
//!   - `ar == 3`: Help (`aq = true`) — state 107.
//!   - else: `as = false`, `aq = false`.
//! * Exit (`s == 29`): if `as`, back out of the
//!   confirm (`as = false`); else close the menu
//!   and resume (`ap = true`, `I = J`).
//! * Draw (e.java ~2148): the title is the level
//!   name (`e(54 + bC)`) or "Arena" (`e(66)`).
//!   The pages are string 1 "Exit", 50 "Main
//!   menu", 46 "Help", 47 "Sound" + "on"/"off"
//!   (234/235). The confirms are 185 "Are you
//!   sure?" (Exit) and 186 "Return to main menu?"
//!   (Main menu). Left/right arrows (`bo[2]` /
//!   `bo[3]`) mark the pages that have a neighbour.
//!
//! The port's menu is a state in `Gameplay` (the
//! world freezes while it is open). The outcomes
//! are returned to the caller (`Game`), which
//! owns the sound setting, the save, and the
//! screen transitions.

/// The pause-menu pages (`ar`, 1-4).
pub const PAGE_EXIT: u8 = 1;
pub const PAGE_MAIN_MENU: u8 = 2;
pub const PAGE_HELP: u8 = 3;
pub const PAGE_SOUND: u8 = 4;

/// What the menu wants the caller to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuOutcome {
    /// Stay in the menu (no action).
    Stay,
    /// Close the menu and resume the game (`ap`).
    Resume,
    /// Quit to the main menu (`ao`): save + unload.
    MainMenu,
    /// Exit the game (`an`): save + quit the app.
    QuitApp,
    /// Show the Help screen (`aq`): state 107.
    Help,
    /// Toggle the sound setting (`iL`).
    SoundToggle,
}

/// The pause menu state (102).
pub struct PauseMenu {
    /// Current page `ar` (1-4).
    pub page: u8,
    /// In the confirm sub-page (`as`).
    pub confirm: bool,
}

impl PauseMenu {
    pub fn new() -> PauseMenu {
        PauseMenu { page: PAGE_EXIT, confirm: false }
    }

    /// The label for the current page (string
    /// indices in the English table, RP10).
    pub fn page_label(&self) -> &'static str {
        match self.page {
            PAGE_EXIT => "Exit",
            PAGE_MAIN_MENU => "Main menu",
            PAGE_HELP => "Help",
            PAGE_SOUND => "Sound",
            _ => "",
        }
    }

    /// The confirm prompt for the current page,
    /// if it has one.
    pub fn confirm_label(&self) -> Option<&'static str> {
        match self.page {
            PAGE_EXIT => Some("Are you sure?"),
            PAGE_MAIN_MENU => Some("Return to main menu?"),
            _ => None,
        }
    }

    /// Up (`s == 2`): next page, clamped to 4.
    /// Ignored while confirming.
    pub fn up(&mut self) {
        if !self.confirm && self.page < PAGE_SOUND {
            self.page += 1;
        }
    }

    /// Down (`s == 5`): previous page, clamped
    /// to 1. Ignored while confirming.
    pub fn down(&mut self) {
        if !self.confirm && self.page > PAGE_EXIT {
            self.page -= 1;
        }
    }

    /// Select / Attack (`s == 8` or `s == 27`).
    pub fn select(&mut self) -> MenuOutcome {
        match self.page {
            PAGE_SOUND => MenuOutcome::SoundToggle,
            PAGE_HELP => MenuOutcome::Help,
            PAGE_EXIT | PAGE_MAIN_MENU => {
                if self.confirm {
                    self.confirm = false;
                    if self.page == PAGE_EXIT {
                        MenuOutcome::QuitApp
                    } else {
                        MenuOutcome::MainMenu
                    }
                } else {
                    self.confirm = true;
                    MenuOutcome::Stay
                }
            }
            _ => {
                self.confirm = false;
                MenuOutcome::Stay
            }
        }
    }

    /// Exit (`s == 29`): back out of the confirm,
    /// or close the menu and resume.
    pub fn exit(&mut self) -> MenuOutcome {
        if self.confirm {
            self.confirm = false;
            MenuOutcome::Stay
        } else {
            MenuOutcome::Resume
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_clamp_1_to_4() {
        let mut m = PauseMenu::new();
        assert_eq!(m.page, PAGE_EXIT);
        m.down();
        assert_eq!(m.page, PAGE_EXIT, "down clamps at 1");
        m.up();
        assert_eq!(m.page, PAGE_MAIN_MENU);
        m.up();
        assert_eq!(m.page, PAGE_HELP);
        m.up();
        assert_eq!(m.page, PAGE_SOUND);
        m.up();
        assert_eq!(m.page, PAGE_SOUND, "up clamps at 4");
    }

    #[test]
    fn exit_page_confirms_then_quits() {
        let mut m = PauseMenu::new();
        assert_eq!(m.page_label(), "Exit");
        assert_eq!(m.select(), MenuOutcome::Stay);
        assert!(m.confirm);
        assert_eq!(m.confirm_label(), Some("Are you sure?"));
        assert_eq!(m.select(), MenuOutcome::QuitApp);
        assert!(!m.confirm);
    }

    #[test]
    fn main_menu_page_confirms_then_quits_to_menu() {
        let mut m = PauseMenu::new();
        m.up();
        assert_eq!(m.page_label(), "Main menu");
        assert_eq!(m.select(), MenuOutcome::Stay);
        assert_eq!(m.confirm_label(), Some("Return to main menu?"));
        assert_eq!(m.select(), MenuOutcome::MainMenu);
    }

    #[test]
    fn help_and_sound_are_immediate() {
        let mut m = PauseMenu::new();
        m.up();
        m.up();
        assert_eq!(m.select(), MenuOutcome::Help);
        m.up();
        assert_eq!(m.select(), MenuOutcome::SoundToggle);
    }

    #[test]
    fn exit_key_backs_out_then_resumes() {
        let mut m = PauseMenu::new();
        m.select();
        assert!(m.confirm);
        assert_eq!(m.exit(), MenuOutcome::Stay);
        assert!(!m.confirm);
        assert_eq!(m.exit(), MenuOutcome::Resume);
    }

    #[test]
    fn nav_ignored_while_confirming() {
        let mut m = PauseMenu::new();
        m.select();
        m.up();
        assert_eq!(m.page, PAGE_EXIT, "up ignored in confirm");
        m.down();
        assert_eq!(m.page, PAGE_EXIT, "down ignored in confirm");
    }
}
