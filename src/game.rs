//! Game logic, slices 1-2: boot, language select, audio prompt,
//! camera transition, main menu.
//!
//! Translated from `decompiled/e.java` (states in the original are the
//! integer `I`; the names here are the slice's, the numbers are the
//! original's, so later slices can match them up):
//!   0  = Boot (staged loader `n()`, K = step)
//!   78 = LanguageSelect
//!   79 = AudioPrompt ("Audio" / "Lord Kratos, do you wish to enable audio?")
//!   80 = MainMenu (carousel, `w()` / `K()`)
//!   1  = Playing (entered from the menu; gameplay itself is slice 3)
//!
//! Slice 2 adds: the camera transition `a(H,H,35,35)` -> `m()`'s
//! countdown (V=375) -> `a(0,0,35,35)` -> state 80, the menu table
//! `J()` (chunk 1028) with `H()` visibility from save progress, LEFT/RIGHT
//! navigation via `k()`/`l()`, and menu selection actions for types
//! 0/1/5/6/8/11/12. Types 3 (Help) and 4 (About) only set their flags
//! and camera target in `K()`; their screens are not yet traced, so
//! they are recorded as stubs, not drawn. Type 9 (Back) and 10 are
//! handled via the menu stack / save path in `K()` where traced.
//!
//! Input codes (`s` in `e.a()` / `e.k()`): Up=1, Down=6, Left=2,
//! Right=5, Select=27 (left softkey) or 8 (fire), Exit=29 (right softkey).
//! Language select, `e.a()` case 78: Up/Down move `w` (0..=5), Select
//! loads `x[w]` and goes to AudioPrompt, Exit quits (original `M()`:
//! save then destroy — here: save defaults are already loaded, so Exit
//! just sets `exited`).
//!
//! Language data, `e.x` / `e.y` (verified against the packs):
//!   w: 0 English, 1 French, 2 Spanish, 3 German, 4 Italian, 5 Portuguese
//!   x: 10240, 12288, 33792, 29696, 30720, 32768 (RP10/12/33/29/30/32 chunk 0)
//!   y: 23, 24, 21, 20, 22, 19 (own-name string index in the English pack:
//!      23 English, 24 Français, 21 Español, 20 Deutsch, 22 Italian,
//!      19 Português)
//!
//! Save defaults, `e.A(1)` when RecordStore "2" is absent: gN=3, gO=2,
//! palette index bC=bD=0, all progress counters 0, hm=hn=1024.
//!
//! Dialog wrapping follows `e.a(int[], byte[], ...)` (the text-box
//! setup): entries >= 0 are string indices, -1 is a blank line, -3 is
//! the next `hX` slot; leading '&'/'^' drop 2 chars, '*' drops 1; lines
//! wrap at the box width using font widths, breaking at the last space
//! or '.' before the limit. Audio dialog: template `if`/`ih` =
//! [44, -1, -3, -3, -3] / [13, 4, 4, 4, 4], hX = [strings[36], "", ""],
//! box (12, 30, 211, 270).
//!
//! Honest limit of slice 2: Playing is entered (level id, arena flag)
//! but gameplay (state 1's level loading, enemies, combat) is slice 3;
//! the Playing draw is a placeholder screen, not gameplay. The window /
//! input layer is still not built; `tick()` is the headless frame step.

use crate::font::Font;
use crate::menu::Menu;
use crate::midp::{draw_sprite_frame, Framebuffer, Graphics};
use crate::resources::{Resources, SpriteSet};
use crate::rp::RpError;

pub const LANG_IDS: [u32; 6] = [10240, 12288, 33792, 29696, 30720, 32768];
pub const LANG_NAME_INDICES: [usize; 6] = [23, 24, 21, 20, 22, 19];
pub const AUDIO_DIALOG_TEMPLATE: [i32; 5] = [44, -1, -3, -3, -3];
pub const AUDIO_DIALOG_FLAGS: [u8; 5] = [13, 4, 4, 4, 4];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    Up,
    Down,
    Left,
    Right,
    Select,
    Exit,
    Attack,
}

impl Input {
    /// Original `s` code for this input (see module docs).
    pub fn s_code(self) -> i32 {
        match self {
            Input::Up => 1,
            Input::Down => 6,
            Input::Left => 2,
            Input::Right => 5,
            Input::Select => 27,
            Input::Exit => 29,
            Input::Attack => 8,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Boot,
    LanguageSelect,
    AudioPrompt,
    MainMenu,
    Playing,
    Exited,
    /// State 107: Help (ai=false) / About (ai=true) dialog.
    HelpAbout,
}

#[derive(Debug, Clone)]
pub struct DialogLine {
    pub text: String,
    pub flag: u8,
}

pub use crate::save::SaveData as Save;

pub struct Game {
    pub res: Resources,
    pub state: State,
    pub lang_index: usize,
    pub strings: Vec<String>,
    pub save: Save,
    pub save_dir: Option<std::path::PathBuf>,
    pub audio_enabled: Option<bool>,
    pub audio: crate::audio::Audio,
    pub exited: bool,
    // Boot / menu assets (n() steps 0, 1, 3).
    pub font: Option<Font>,
    pub font_height: i32,
    pub font_menu: Option<Font>, // o = b(1032), menu items
    pub font_title: Option<Font>, // p = b(1033), title
    pub font_w_width: i32,      // iJ = width("W") in font q, times 4
    pub banners: Option<SpriteSet>, // r = b(1039)
    pub arrows: Option<SpriteSet>,  // bo = b(1029)
    pub logo: Option<SpriteSet>,    // iS = b(1037), used by boot screen only
    boot_step: u8,
    // Audio dialog, built on entering AudioPrompt.
    pub dialog_lines: Vec<DialogLine>,
    // Camera transition (e.a(int,int,int,int) / e.m()).
    // Fixed-point positions, << 8; speed ac = ad = 35; countdown V = 375.
    pub cam_x: i32,
    pub cam_y: i32,
    pub cam_target_x: i32,
    pub cam_target_y: i32,
    pub cam_moving: bool,
    pub cam_countdown: i32,
    pub audio_transition_started: bool,
    pub saved: bool,
    // Main menu (slice 2).
    pub menu: Option<Menu>,
    pub menu_level: i32,      // bC: current level index
    pub menu_progress: usize, // bD: highest unlocked level
    pub playing_level: Option<i32>, // bA: level id for state 1
    pub playing_arena: bool,  // iN: arena mode
    pub help_or_about: Option<&'static str>,
    pub help_about_text: String,
    pub help_about_scroll: i32,
    pub pending_help_about: bool,
    pub gameplay: Option<crate::gameplay::Gameplay>,
    pub held_dir: i32,
    /// Down held (slice 18): set by the window
    /// loop from the held ArrowDown key (or by a
    /// Down press in scripted modes); drives the
    /// block stance via Gameplay.block_held.
    pub held_down: bool,
}

impl Game {
    pub fn new(dir: impl AsRef<std::path::Path>) -> Game {
        Game {
            res: Resources::new(dir),
            state: State::Boot,
            lang_index: 0,
            strings: Vec::new(),
            save: Save::default(),
            save_dir: None,
            audio_enabled: None,
            audio: crate::audio::Audio::default(),
            exited: false,
            font: None,
            font_height: 0,
            font_menu: None,
            font_title: None,
            font_w_width: 0,
            banners: None,
            arrows: None,
            logo: None,
            boot_step: 0,
            dialog_lines: Vec::new(),
            cam_x: 0,
            cam_y: 0,
            cam_target_x: 0,
            cam_target_y: 0,
            cam_moving: false,
            cam_countdown: 0,
            audio_transition_started: false,
            saved: false,
            menu: None,
            menu_level: 0,
            menu_progress: 0,
            playing_level: None,
            playing_arena: false,
            help_or_about: None,
            help_about_text: String::new(),
            help_about_scroll: 0,
            pending_help_about: false,
            gameplay: None,
            held_dir: 0,
            held_down: false,
        }
    }

    /// `e.a(n, n2, n3, n4)`: set camera target; W = moving, V = 375
    /// when moving forward (either coordinate <= target).
    pub fn set_camera_target(&mut self, x: i32, y: i32) {
        let tx = x << 8;
        let ty = y << 8;
        let moving_y = self.cam_x <= tx;
        let moving_z = self.cam_y <= ty;
        self.cam_moving = moving_z || moving_y;
        if self.cam_moving {
            self.cam_countdown = 375;
        }
        self.cam_target_x = tx;
        self.cam_target_y = ty;
    }

    pub fn string(&self, index: usize) -> &str {
        self.strings.get(index).map(|s| s.as_str()).unwrap_or("")
    }

    /// One boot step (n() steps 0, 1, 3; other steps are slice 2).
    /// Returns true when boot is finished and we are in LanguageSelect.
    pub fn boot_step(&mut self) -> Result<bool, RpError> {
        match self.boot_step {
            0 => {
                // n() case 0: 240x320, English strings, palette table R
                // (chunk 1053, 3 bytes per entry -> ARGB ints), then
                // sprites iS (1037), cp (1035), cq (1030), fu (1038).
                self.strings = self.res.strings(10240)?;
                let _palette = self.res.chunk(1053)?;
                self.logo = Some(self.res.sprite_set(1037, None)?);
                let _cp = self.res.sprite_set(1035, None)?;
                let _cq = self.res.sprite_set(1030, None)?;
                let _fu = self.res.sprite_set(1038, None)?;
                self.boot_step = 1;
            }
            1 => {
                // n() case 1: font q (1031), font height z, half-height A,
                // iJ = width("W") * 4 (used for menu arrows).
                let font_set = self.res.sprite_set(1031, None)?;
                let font = Font::from_sprite_set(font_set);
                self.font_height = font.height();
                self.font_w_width = font.width("W") * 4;
                self.font = Some(font);
                self.boot_step = 3;
            }
            3 => {
                // n() case 3: sprites aL (1036), aU (1040), bo (1029),
                // plus J() menu table (chunk 1028) in the same step's
                // tail (originally called from n() case 3 via J()).
                let _a_l = self.res.sprite_set(1036, None)?;
                let _a_u = self.res.sprite_set(1040, None)?;
                self.arrows = Some(self.res.sprite_set(1029, None)?);
                // b(true) in n() case 0 order: r (1039), o (1032), p (1033).
                self.banners = Some(self.res.sprite_set(1039, None)?);
                let o_set = self.res.sprite_set(1032, None)?;
                self.font_menu = Some(Font::from_sprite_set(o_set));
                let p_set = self.res.sprite_set(1033, None)?;
                self.font_title = Some(Font::from_sprite_set(p_set));
                // Case 0 tail: A(1) save load (defaults if absent),
                // then menu setup uses the loaded progress.
                // Save comes from RecordStore "2" in save_dir
                // when set, else defaults.
                self.load_save();
                let menu_chunk = self.res.chunk(1028)?;
                let mut menu = Menu::from_chunk(&menu_chunk);
                menu.apply_progress(self.save.progress as usize);
                self.menu_progress = self.save.progress as usize;
                self.menu_level = 0;
                self.menu = Some(menu);
                self.state = State::LanguageSelect;
                return Ok(true);
            }
            _ => {
                self.state = State::LanguageSelect;
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn boot(&mut self) -> Result<(), RpError> {
        while !self.boot_step()? {}
        Ok(())
    }

    fn build_audio_dialog(&mut self) {
        let h_x = [
            self.string(36).to_string(),
            String::new(),
            String::new(),
        ];
        self.dialog_lines = build_dialog(
            &self.font,
            &AUDIO_DIALOG_TEMPLATE,
            &AUDIO_DIALOG_FLAGS,
            &h_x,
            &self.strings,
        );
    }

    pub fn update(&mut self, input: Option<Input>) {
        let Some(input) = input else {
            return;
        };
        match self.state {
            State::LanguageSelect => match input {
                Input::Up => {
                    if self.lang_index > 0 {
                        self.lang_index -= 1;
                    }
                }
                Input::Down => {
                    if self.lang_index < 5 {
                        self.lang_index += 1;
                    }
                }
                Input::Select => {
                    // e.a() case 78, s == 27 or 8: load x[w], dialog, state 79.
                    if let Ok(strings) = self.res.strings(LANG_IDS[self.lang_index]) {
                        self.strings = strings;
                    }
                    self.build_audio_dialog();
                    self.state = State::AudioPrompt;
                }
                Input::Exit => {
                    self.state = State::Exited;
                    self.exited = true;
                }
                _ => {}
            },
            State::AudioPrompt => match input {
                Input::Select => {
                    self.audio_enabled = Some(true);
                    self.audio.set_enabled(true);
                    self.start_menu_music(); // y(0): menu music
                    self.start_audio_transition();
                }
                Input::Exit => {
                    self.audio_enabled = Some(false);
                    self.audio.set_enabled(false);
                    self.start_audio_transition();
                }
                _ => {}
            },
            State::MainMenu => {
                // Menu input is gated on the camera being back at (0,0),
                // as in e.a() case 80 (if T == 0: L(); K()).
                if self.cam_x != 0 || self.cam_y != 0 {
                    return;
                }
                self.menu_input(input);
            }
            State::HelpAbout => {
                // State 107 dialog: Up/Down scroll, Exit (Back)
                // returns to MainMenu (camera back to origin).
                match input {
                    Input::Up => {
                        if self.help_about_scroll > 0 {
                            self.help_about_scroll -= 12;
                        }
                    }
                    Input::Down => {
                        self.help_about_scroll += 12;
                    }
                    Input::Exit => {
                        self.state = State::MainMenu;
                        self.help_about_scroll = 0;
                        self.set_camera_target(0, 0);
                    }
                    _ => {}
                }
            }
            State::Playing => {
                // A showing level intro dialog captures
                // input first: Select/Exit dismisses it
                // and gameplay starts; anything else is
                // ignored while it shows.
                if self
                    .gameplay
                    .as_ref()
                    .map(|gp| gp.intro_active())
                    .unwrap_or(false)
                {
                    match input {
                        Input::Select | Input::Exit => {
                            if let Some(gp) = &mut self.gameplay {
                                gp.dismiss_intro();
                            }
                        }
                        _ => {}
                    }
                    return;
                }
                // A showing script dialog captures input:
                // Select advances the script, Exit cancels
                // it; the world stays frozen meanwhile.
                if self
                    .gameplay
                    .as_ref()
                    .map(|gp| gp.script.active())
                    .unwrap_or(false)
                {
                    match input {
                        Input::Select => {
                            if let Some(gp) = &mut self.gameplay {
                                gp.script_advance();
                            }
                            self.handle_level_complete();
                        }
                        Input::Exit => {
                            if let Some(gp) = &mut self.gameplay {
                                gp.script_cancel();
                            }
                        }
                        _ => {}
                    }
                    return;
                }
                // Gameplay keys: Left/Right set the held direction and
                // advance one frame, Attack swings and advances one frame,
                // Up jumps and advances one frame, Select cycles weapons
                // (left softkey, slice 17), Exit returns to the
                // main menu (softkey Menu, not yet a pause screen in
                // the original either - it opens the in-game menu,
                // state 102, which is not ported yet).
                match input {
                    Input::Select => {
                        if let Some(gp) = &mut self.gameplay {
                            gp.cycle_weapon();
                        }
                    }
                    Input::Up => {
                        if let Some(gp) = &mut self.gameplay {
                            gp.jump();
                        }
                        self.gameplay_tick();
                    }
                    Input::Left => {
                        self.held_dir = -1;
                        self.gameplay_tick();
                    }
                    Input::Right => {
                        self.held_dir = 1;
                        self.gameplay_tick();
                    }
                    Input::Down => {
                        // Hold-to-block (slice 18):
                        // sticky in scripted modes
                        // (no key-release there); the
                        // live window overwrites
                        // held_down every frame.
                        self.held_down = true;
                        self.gameplay_tick();
                    }
                    Input::Attack => {
                        self.gameplay_tick_attack();
                    }
                    Input::Exit => {
                        self.state = State::MainMenu;
                        self.gameplay = None;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    /// y(0): start the menu music, slot 0 = resource 1063,
    /// looping. State-only `play(0)` first, so a missing or
    /// unparsable chunk still leaves the slot state correct;
    /// `play_midi` then replaces it with real playback on
    /// Windows (inert elsewhere).
    fn start_menu_music(&mut self) {
        self.audio.play(0);
        if let Ok(bytes) = self.res.chunk(crate::audio::SLOTS[0].1) {
            let _ = self.audio.play_midi(0, &bytes, true);
        }
    }

    /// Audio prompt choice, e.a() case 79: if the camera target is
    /// already centre, target (0,0), else target centre (160,160).
    fn start_audio_transition(&mut self) {
        if self.audio_transition_started {
            return;
        }
        self.audio_transition_started = true;
        if self.cam_target_x == (160 << 8) {
            self.set_camera_target(0, 0);
        } else {
            self.set_camera_target(160, 160);
        }
    }

    /// One frame step: camera movement (e.a() tail, speed 35, L = delta
    /// ms) and the m() transition for state 79. Call this before
    /// update() for the frame's input, or with None, in the window loop.
    pub fn set_save_dir(&mut self, dir: impl AsRef<std::path::Path>) {
        self.save_dir = Some(dir.as_ref().to_path_buf());
    }

    /// Load RecordStore "2" into self.save, or defaults if absent.
    pub fn load_save(&mut self) {
        if let Some(dir) = &self.save_dir {
            let rs = crate::midp::RecordStore::new(dir);
            if let Some(bytes) = rs.read_all("2") {
                if let Some(save) = crate::save::SaveData::from_bytes(&bytes) {
                    self.save = save;
                    return;
                }
            }
        }
        self.save = Save::default();
    }

    /// Write self.save to RecordStore "2" (z(1)). No-op without
    /// a save directory. Sets `saved` on success.
    pub fn persist_save(&mut self) {
        if let Some(dir) = &self.save_dir {
            let rs = crate::midp::RecordStore::new(dir);
            if rs.write_all("2", Some(&self.save.to_bytes())) {
                self.saved = true;
            }
        } else {
            self.saved = true;
        }
    }

    /// Delete RecordStore "2" (Clear Save).
    pub fn clear_save_file(&mut self) {
        if let Some(dir) = &self.save_dir {
            let rs = crate::midp::RecordStore::new(dir);
            rs.write_all("2", None);
        }
        self.save = Save::default();
    }

    /// Load gameplay for the level just entered (menu -> state 1).
    /// Failure keeps the placeholder draw (gameplay stays None).
    pub fn enter_playing(&mut self) {
        if let Some(level_id) = self.playing_level {
            let arena = self.playing_arena;
            self.gameplay = crate::gameplay::Gameplay::load(&mut self.res, level_id as u32, arena).ok();
            // Selected-language strings overwrite the
            // English default loaded by Gameplay::load
            // (script strings and the intro dialog).
            // Combo caps/shifts come from the save
            // (slice 16: gN/gP; slice 17: gO/gQ for
            // Artemis).
            if let Some(gp) = &mut self.gameplay {
                gp.set_strings(self.strings.clone());
                gp.player.combo_max = self.save.g_n;
                gp.player.combo_shift = self.save.g_p;
                gp.player.combo_max_artemis = self.save.g_o;
                gp.player.combo_shift_artemis = self.save.g_q;
            }
        }
    }

    /// Consume a level-complete flag from gameplay:
    /// save progress, then load the next level, or
    /// return to the main menu after the last level.
    fn handle_level_complete(&mut self) {
        let next_idx = match &mut self.gameplay {
            Some(gp) => gp.level_complete.take(),
            None => None,
        };
        let Some(next_idx) = next_idx else {
            return;
        };
        if next_idx < 10 {
            if self.save.progress < next_idx as i32 {
                self.save.progress = next_idx as i32;
            }
            self.persist_save();
            self.menu_progress = self.save.progress as usize;
            self.playing_level = Some(crate::gameplay::LEVEL_IDS[next_idx] as i32);
            self.playing_arena = false;
            self.enter_playing();
        } else {
            if self.save.progress < 10 {
                self.save.progress = 10;
            }
            self.persist_save();
            self.menu_progress = self.save.progress as usize;
            self.gameplay = None;
            self.state = State::MainMenu;
            if let Some(menu) = &mut self.menu {
                menu.apply_progress(self.menu_progress);
            }
        }
    }

    fn gameplay_tick(&mut self) {
        if let Some(gp) = &mut self.gameplay {
            let dir = self.held_dir;
            gp.block_held = self.held_down;
            gp.tick(&mut self.res, dir, false);
        }
        self.handle_level_complete();
    }

    fn gameplay_tick_attack(&mut self) {
        if let Some(gp) = &mut self.gameplay {
            let dir = self.held_dir;
            gp.block_held = self.held_down;
            gp.tick(&mut self.res, dir, true);
        }
        self.handle_level_complete();
    }

    /// Live-window jump frame: start the jump, then advance
    /// one gameplay frame (mirrors the scripted Up handling).
    pub fn tick_playing_jump(&mut self) {
        if self.state == State::Playing {
            if let Some(gp) = &mut self.gameplay {
                gp.jump();
            }
            self.gameplay_tick();
        }
    }

    /// Live-window frame step for Playing: advance one 40 ms
    /// gameplay frame with the current held_dir (set by the window
    /// loop from held arrow keys). Scripted modes keep using
    /// update(), which ticks per key press; this exists so holding
    /// an arrow walks continuously in the real window.
    pub fn tick_playing(&mut self) {
        if self.state == State::Playing {
            self.gameplay_tick();
        }
    }

    /// Same, for a frame where the player attacked (Space pressed).
    pub fn tick_playing_attack(&mut self) {
        if self.state == State::Playing {
            self.gameplay_tick_attack();
        }
    }

    pub fn tick(&mut self, delta_ms: i32) {
        let l = delta_ms;
        // Camera movement towards target.
        if self.cam_x < self.cam_target_x {
            self.cam_x += l * 35;
            if self.cam_x >= self.cam_target_x {
                self.cam_x = self.cam_target_x;
            }
        }
        if self.cam_y < self.cam_target_y {
            self.cam_y += l * 35;
            if self.cam_y >= self.cam_target_y {
                self.cam_y = self.cam_target_y;
            }
        }
        if self.cam_x > self.cam_target_x {
            self.cam_x -= l * 35;
            if self.cam_x <= self.cam_target_x {
                self.cam_x = self.cam_target_x;
            }
        }
        if self.cam_y > self.cam_target_y {
            self.cam_y -= l * 35;
            if self.cam_y <= self.cam_target_y {
                self.cam_y = self.cam_target_y;
            }
        }
        if self.cam_x == self.cam_target_x && self.cam_y == self.cam_target_y {
            self.cam_moving = false;
        }
        // Help/About: camera arrived at (160,160) with a pending
        // Help/About selection -> state 107 dialog (ai flag).
        if self.pending_help_about
            && self.state == State::MainMenu
            && self.cam_x == self.cam_target_x
            && self.cam_y == self.cam_target_y
            && self.cam_target_x == (160 << 8)
        {
            self.pending_help_about = false;
            self.help_about_text = if self.help_or_about == Some("about") {
                self.string(115).replace('%', "1.4.8")
            } else {
                format!("{}\n{}", self.string(114), self.string(292))
            };
            self.help_about_scroll = 0;
            self.state = State::HelpAbout;
        }
        // m() countdown, then the state-79 transition.
        if self.cam_x == self.cam_target_x
            && self.cam_y == self.cam_target_y
            && self.cam_countdown > 0
        {
            self.cam_countdown -= l;
            if self.cam_countdown < 0 {
                self.cam_countdown = 0;
            }
        }
        if self.state == State::AudioPrompt
            && self.audio_transition_started
            && self.cam_countdown == 0
            && self.cam_x == self.cam_target_x
            && self.cam_y == self.cam_target_y
            && self.cam_target_x == (160 << 8)
        {
            // m() for I == 79: a(0,0,35,35), save, state 80.
            self.set_camera_target(0, 0);
            self.persist_save();
            self.state = State::MainMenu;
            self.audio_transition_started = false;
        }
    }

    /// Main menu input, e.K() (called from case 80 via L()/K()).
    /// Left/Right move ix via l()/k() with flag 0x2000000; Select
    /// activates the current item's type; Exit returns to the previous
    /// menu (stack) or, on the root menu, does nothing (the Exit item
    /// itself quits via type 0 -> next 23 -> type 8 -> M()).
    pub fn menu_input(&mut self, input: Input) {
        let Some(menu) = &mut self.menu else {
            return;
        };
        match input {
            Input::Left => {
                menu.ix = menu.l(menu.ix, 0x2000000);
            }
            Input::Right => {
                menu.ix = menu.k(menu.ix, 0x2000000);
            }
            Input::Select => {
                let Some((typ, next, _str_idx)) = menu.entry(menu.ix) else {
                    return;
                };
                match typ {
                    0 => {
                        if next == 9 && self.menu_level == 0 {
                            // Play Game with no progress: start level 9217.
                            self.playing_level = Some(9217);
                            self.playing_arena = false;
                            self.enter_playing();
self.state = State::Playing;
                        } else if next != menu.ix {
                            // Gameroom items link to themselves (external
                            // lobby links, not implemented in slice 2);
                            // do not push a self-loop onto the stack.
                            menu.stack.push(menu.ix);
                            menu.ix = next;
                        }
                    }
                    1 => {
                        // Sound toggle (iM.a(!iM.b())).
                        let enabled = self.audio_enabled.unwrap_or(true);
                        self.audio_enabled = Some(!enabled);
                        self.audio.set_enabled(!enabled);
                        if !enabled {
                            self.start_menu_music();
                        }
                    }
                    3 => {
                        self.help_or_about = Some("help");
                        self.pending_help_about = true;
                        self.set_camera_target(160, 160);
                    }
                    4 => {
                        self.help_or_about = Some("about");
                        self.pending_help_about = true;
                        self.set_camera_target(160, 160);
                    }
                    5 => {
                        // Level select: bC = ix - iG, bA = bB[bC].
                        let start = menu.menu_start();
                        let b_c = menu.ix as i32 - start as i32;
                        let level_ids = [9217, 19456, 20480, 21504, 22528, 23552, 24576, 25600, 26624, 27648];
                        self.menu_level = b_c;
                        self.playing_level = Some(level_ids.get(b_c as usize).copied().unwrap_or(9217));
                        self.playing_arena = false;
                        self.enter_playing();
self.state = State::Playing;
                    }
                    6 => {
                        // Arena: bA = 9216, iN = true.
                        self.playing_level = Some(9216);
                        self.playing_arena = true;
                        self.enter_playing();
self.state = State::Playing;
                    }
                    8 => {
                        // Exit: M() — save then destroy.
                        self.persist_save();
                        self.state = State::Exited;
                        self.exited = true;
                    }
                    11 => {
                        // Clear Save confirm: clear RecordStore "2",
                        // reload defaults, go to next menu (19).
                        self.clear_save_file();
                        self.menu_level = 0;
                        self.menu_progress = 0;
                        if let Some(menu) = &mut self.menu {
                            menu.apply_progress(0);
                            menu.ix = next;
                        }
                    }
                    12 => {
                        // Language cycle: w++, f(x[w]).
                        self.lang_index = if self.lang_index >= 5 { 0 } else { self.lang_index + 1 };
                        if let Ok(strings) = self.res.strings(LANG_IDS[self.lang_index]) {
                            self.strings = strings;
                        }
                    }
                    _ => {
                        // Types 7 (header), 9 (back), 10: no direct action
                        // in slice 2; back is via Exit below.
                    }
                }
            }
            Input::Exit => {
                if let Some(prev) = self.menu.as_mut().and_then(|m| m.stack.pop()) {
                    self.menu.as_mut().unwrap().ix = prev;
                }
            }
            _ => {}
        }
    }

    /// Draw the current state onto a 240x320 framebuffer.
    /// Language select follows `e.a(Graphics)` case 78: black fill,
    /// banners r[0]/r[1] at (120, 0), language names centred at x=120
    /// starting at y = 5*z, stepping z + z/4, arrows bo[2]/bo[3] flanking
    /// the selected row. Audio prompt draws the same banners plus the
    /// dialog lines (centred, starting at y=30, stepping z).
    pub fn draw(&mut self, fb: &mut Framebuffer) {
        fb.clear(0xFF00_0000);
        let mut g = Graphics::default();
        g.set_clip_full(240, 320);
        let Some(font) = &self.font else {
            return;
        };
        let z = self.font_height;
        // State 107 (Help/About) draws only the dialog text, no
        // menu banners — the original's case 107 does not draw r[].
        if self.state != State::HelpAbout {
            if let Some(banners) = &self.banners {
            if let Some(f) = banners.frame(0) {
                draw_sprite_frame(&g, fb, &banners.image, f, 120, 0, 0);
            }
            if let Some(f) = banners.frame(1) {
                draw_sprite_frame(&g, fb, &banners.image, f, 120, 0, 0);
            }
        }
        }
        match self.state {
            State::LanguageSelect => {
                let mut y = (z << 2) + z; // 5*z
                for (i, &name_idx) in LANG_NAME_INDICES.iter().enumerate() {
                    let name = self.string(name_idx);
                    font.draw(&g, fb, name, 120, y, 1);
                    if i == self.lang_index {
                        if let Some(arrows) = &self.arrows {
                            if let Some(f) = arrows.frame(2) {
                                draw_sprite_frame(
                                    &g,
                                    fb,
                                    &arrows.image,
                                    f,
                                    (120 >> 1) - f.w,
                                    y + (z >> 2),
                                    0,
                                );
                            }
                            if let Some(f) = arrows.frame(3) {
                                draw_sprite_frame(
                                    &g,
                                    fb,
                                    &arrows.image,
                                    f,
                                    240 - (120 >> 1),
                                    y + (z >> 2),
                                    0,
                                );
                            }
                        }
                    }
                    y += z + (z >> 2);
                }
                // Bottom bar: strings 0 ("Select") / 1 ("Exit"), drawn
                // left/right at the bottom — the original's a(graphics, 0, 1).
                // Rendered here as centred text at y=306 for the slice-1
                // proof; the full softkey bar is slice 2.
                font.draw(&g, fb, self.string(0), 60, 306, 1);
                font.draw(&g, fb, self.string(1), 180, 306, 1);
            }
            State::AudioPrompt => {
                // Dialog draw, e.a(Graphics, boolean): lines start at
                // hT + 25 for state 79 (hT = 30), i.e. y = 55, below the
                // logo banners. CFR's loop shows no per-line y increment
                // (all lines would overlap at one y, which the shipped
                // game plainly does not do), so that increment is treated
                // as a decompiler artifact and lines step by z here.
                let mut y = 30 + 25;
                for line in &self.dialog_lines {
                    if !line.text.is_empty() {
                        font.draw(&g, fb, &line.text, 120, y, 1);
                    }
                    y += z;
                }
            }
            State::MainMenu => {
                // w(): title with font p at (120, r0.d + r1.d),
                // selected item with font o at (120, iF), iF =
                // r0.d + r1.d + (z << 1), arrows flanking it when there
                // is a previous/next item (ix > iH / ix < n5), bottom
                // decorations r[2..4] at (240, 320) with their anchors.
                let (Some(font_title), Some(font_menu)) = (&self.font_title, &self.font_menu) else {
                    return;
                };
                let banner_h = self
                    .banners
                    .as_ref()
                    .map(|b| {
                        b.frame(0).map(|f| f.h).unwrap_or(0)
                            + b.frame(1).map(|f| f.h).unwrap_or(0)
                    })
                    .unwrap_or(39);
                font_title.draw(&g, fb, self.string(274), 120, banner_h, 1);
                let item_y = banner_h + (z << 1);
                if let Some(menu) = &self.menu {
                    let start = menu.menu_start();
                    let end = menu.menu_end();
                    let last_next = menu.range_last_next(start);
                    if let Some((typ, _next, str_idx)) = menu.entry(menu.ix) {
                        let mut label = if str_idx == 23 {
                            // Language item shows the NEXT language's name.
                            let next_lang = if self.lang_index >= 5 { 0 } else { self.lang_index + 1 };
                            self.string(LANG_NAME_INDICES[next_lang]).to_string()
                        } else {
                            self.string(str_idx).to_string()
                        };
                        if typ == 1 {
                            label = format!("{} {}", label, self.string(if self.audio_enabled.unwrap_or(true) { 234 } else { 235 }));
                        }
                        font_menu.draw(&g, fb, &label, 120, item_y, 1);
                        if let Some(arrows) = &self.arrows {
                            let ij = self.font_w_width;
                            if menu.ix > end {
                                if let Some(f) = arrows.frame(2) {
                                    draw_sprite_frame(&g, fb, &arrows.image, f, 120 - ij - (f.w << 1), item_y + (z >> 3) + 4, 0);
                                }
                            }
                            if menu.ix < last_next {
                                if let Some(f) = arrows.frame(3) {
                                    draw_sprite_frame(&g, fb, &arrows.image, f, 120 + ij + f.w, item_y + (z >> 3) + 4, 0);
                                }
                            }
                        }
                    }
                }
                if let Some(banners) = &self.banners {
                    for i in 2..=4 {
                        if let Some(f) = banners.frame(i) {
                            draw_sprite_frame(&g, fb, &banners.image, f, 240, 320, 0);
                        }
                    }
                }
                font.draw(&g, fb, self.string(0), 60, 306, 1);
                font.draw(&g, fb, self.string(1), 180, 306, 1);
            }
            State::HelpAbout => {
                // State 107: black screen, dialog text (font q,
                // height 12), scrolled by help_about_scroll.
                // Wrapping: split on \n, then wrap long lines at
                // the 240 px screen width using font widths.
                let mut lines: Vec<String> = Vec::new();
                for para in self.help_about_text.split('\n') {
                    if para.is_empty() {
                        lines.push(String::new());
                        continue;
                    }
                    let mut cur = String::new();
                    for word in para.split(' ') {
                        let trial = if cur.is_empty() {
                            word.to_string()
                        } else {
                            format!("{cur} {word}")
                        };
                        if font.width(&trial) > 236 && !cur.is_empty() {
                            lines.push(cur);
                            cur = word.to_string();
                        } else {
                            cur = trial;
                        }
                    }
                    lines.push(cur);
                }
                let mut y = -self.help_about_scroll;
                for line in &lines {
                    if y >= -12 && y < 320 {
                        font.draw(&g, fb, line, 2, y, 4);
                    }
                    y += 12;
                }
                // Bottom bar softkeys: Back (string 2) on the right.
                font.draw(&g, fb, self.string(2), 180, 306, 1);
            }
            State::HelpAbout => {
                // State 107: black screen, dialog text (font q,
                // height 12), scrolled by help_about_scroll.
                let mut lines: Vec<String> = Vec::new();
                for para in self.help_about_text.split('\n') {
                    if para.is_empty() {
                        lines.push(String::new());
                        continue;
                    }
                    let mut cur = String::new();
                    for word in para.split(' ') {
                        let trial = if cur.is_empty() {
                            word.to_string()
                        } else {
                            format!("{cur} {word}")
                        };
                        if font.width(&trial) > 236 && !cur.is_empty() {
                            lines.push(cur);
                            cur = word.to_string();
                        } else {
                            cur = trial;
                        }
                    }
                    lines.push(cur);
                }
                let mut y = -self.help_about_scroll;
                for line in &lines {
                    if y >= -12 && y < 320 {
                        font.draw(&g, fb, line, 2, y, 4);
                    }
                    y += 12;
                }
                font.draw(&g, fb, self.string(2), 180, 306, 1);
            }
            State::Playing => {
                if let Some(gp) = &self.gameplay {
                    // Gameplay draws its own HUD; font is the slice-1
                    // bitmap font (def 1031) already loaded by boot.
                    if let Some(font) = &self.font {
                        gp.draw(fb, &mut self.res, font);
                    }
                } else {
                    // Fallback placeholder if gameplay failed to load.
                    let level_name = match self.playing_level {
                        Some(9216) => "Arena".to_string(),
                        Some(9217) => self.string(54).to_string(),
                        Some(id) => format!("level {id}"),
                        None => String::new(),
                    };
                    font.draw(&g, fb, &level_name, 120, 140, 1);
                    font.draw(&g, fb, "gameplay: slice 3", 120, 160, 1);
                }
            }
            _ => {}
        }
        let _ = &g;
    }
}

/// Dialog line wrapping, from `e.a(int[], byte[], n, n2, n3, n4, n5)`:
/// box width 211 (hU), left margin 12 (hS). Each source entry produces
/// one or more wrapped lines; a line breaks at the last space or '.'
/// before the width limit, or at '\n'. Flag bits: bit 2 (value 4 in the
/// original's byte flags? no — the draw anchor uses (by & 2) != 0 for
/// LEFT, else HCENTER) are carried through for the draw step.
pub fn build_dialog(
    font: &Option<Font>,
    template: &[i32],
    flags: &[u8],
    h_x: &[String],
    strings: &[String],
) -> Vec<DialogLine> {
    let Some(font) = font else {
        return Vec::new();
    };
    let width_limit = 211 - 1 - 4; // n3 - 1 - 4 in the original
    let mut lines = Vec::new();
    let mut h_x_i = 0usize;
    for (entry_i, &entry) in template.iter().enumerate() {
        let flag = flags.get(entry_i).copied().unwrap_or(0);
        let mut text = if entry >= 0 {
            strings.get(entry as usize).cloned().unwrap_or_default()
        } else if entry == -1 {
            String::new()
        } else {
            // -3: next hX slot.
            let s = h_x.get(h_x_i).cloned().unwrap_or_default();
            h_x_i += 1;
            s
        };
        if text.is_empty() {
            lines.push(DialogLine { text, flag });
            continue;
        }
        // Prefix stripping: '&'/'^' drop 2 chars, '*' drops 1.
        if text.starts_with('&') || text.starts_with('^') {
            text = text.chars().skip(2).collect();
        } else if text.starts_with('*') {
            text = text.chars().skip(1).collect();
        }
        // Wrap.
        let mut rest = text.as_str();
        while !rest.is_empty() {
            // Find how many chars fit within width_limit.
            let mut fit = 0usize;
            let mut fit_width = 0i32;
            for (i, ch) in rest.char_indices() {
                let w = font.glyph(ch).map(|g| g.g).unwrap_or(12);
                if fit_width + w > width_limit && fit > 0 {
                    break;
                }
                fit_width += w;
                fit = i + ch.len_utf8();
            }
            if fit == 0 {
                fit = rest.len();
            }
            let mut chunk = &rest[..fit.min(rest.len())];
            let mut consumed = chunk.len();
            if fit < rest.len() {
                // Break at last space or '.' at or before fit.
                let mut brk: Option<usize> = None;
                for (i, ch) in chunk.char_indices() {
                    if ch == ' ' || ch == '.' {
                        brk = Some(i);
                    }
                }
                if let Some(b) = brk {
                    chunk = chunk[..b].trim_end();
                    consumed = b + 1; // skip the space
                }
            } else if let Some(nl) = chunk.find('\n') {
                chunk = &chunk[..nl];
                consumed = nl + 1;
            }
            lines.push(DialogLine {
                text: chunk.trim().to_string(),
                flag,
            });
            rest = rest[consumed..].trim_start();
            if rest.starts_with('\n') {
                rest = &rest[1..];
            }
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn language_select_flow() {
        let mut game = Game::new("../extracted");
        game.boot().unwrap();
        assert_eq!(game.state, State::LanguageSelect);
        assert_eq!(game.font_height, 12);
        assert_eq!(game.string(23), "English");
        assert_eq!(game.string(0), "Select");
        // Down, down -> Spanish (w=2, x=33792), Select -> AudioPrompt.
        game.update(Some(Input::Down));
        game.update(Some(Input::Down));
        assert_eq!(game.lang_index, 2);
        game.update(Some(Input::Select));
        assert_eq!(game.state, State::AudioPrompt);
        assert_eq!(game.string(0), "Elegir"); // Spanish "Select"
        assert_eq!(game.string(36), "Lord Kratos, ¿quieres activar el sonido?");
        // Dialog: "Audio" (Spanish pack index 44) + prompt lines.
        assert!(!game.dialog_lines.is_empty());
        assert_eq!(game.dialog_lines[0].text, "Audio");
        game.update(Some(Input::Select));
        assert_eq!(game.audio_enabled, Some(true));
    }
    #[test]
    fn language_select_bounds() {
        let mut game = Game::new("../extracted");
        game.boot().unwrap();
        game.update(Some(Input::Up));
        assert_eq!(game.lang_index, 0);
        for _ in 0..10 {
            game.update(Some(Input::Down));
        }
        assert_eq!(game.lang_index, 5);
        game.update(Some(Input::Exit));
        assert_eq!(game.state, State::Exited);
    }
    #[test]
    fn audio_transition_to_main_menu() {
        let mut game = Game::new("../extracted");
        game.boot().unwrap();
        game.update(Some(Input::Select)); // English -> AudioPrompt
        assert_eq!(game.state, State::AudioPrompt);
        game.update(Some(Input::Select)); // audio on
        assert_eq!(game.audio_enabled, Some(true));
        for _ in 0..200 {
            if game.state == State::MainMenu && game.cam_x == 0 && game.cam_y == 0 {
                break;
            }
            game.tick(40);
        }
        assert_eq!(game.state, State::MainMenu);
        assert_eq!(game.cam_x, 0);
        assert!(game.saved);
        // Root menu: selected item is Play Game.
        let menu = game.menu.as_ref().unwrap();
        assert_eq!(menu.ix, 0);
        assert_eq!(game.string(menu.entry(0).unwrap().2), "Play Game");
        // Right -> Arena, Left -> back to Play Game.
        game.update(Some(Input::Right));
        assert_eq!(game.menu.as_ref().unwrap().ix, 1);
        game.update(Some(Input::Left));
        assert_eq!(game.menu.as_ref().unwrap().ix, 0);
        // Select Play Game with no progress -> Playing, level 9217.
        game.update(Some(Input::Select));
        assert_eq!(game.state, State::Playing);
        assert_eq!(game.playing_level, Some(9217));
    }
    #[test]
    fn main_menu_options_and_language() {
        let mut game = Game::new("../extracted");
        game.boot().unwrap();
        game.update(Some(Input::Select));
        game.update(Some(Input::Select));
        for _ in 0..200 {
            if game.state == State::MainMenu && game.cam_x == 0 && game.cam_y == 0 {
                break;
            }
            game.tick(40);
        }
        // Right x3: Play Game -> Arena -> Help -> Options, then Select.
        game.update(Some(Input::Right));
        game.update(Some(Input::Right));
        game.update(Some(Input::Right));
        assert_eq!(game.menu.as_ref().unwrap().ix, 3);
        game.update(Some(Input::Select));
        assert_eq!(game.menu.as_ref().unwrap().ix, 19); // Options submenu
        // Right x2: Sound -> Clear Save -> Language, then Select cycles.
        game.update(Some(Input::Right));
        game.update(Some(Input::Right));
        assert_eq!(game.menu.as_ref().unwrap().ix, 21);
        game.update(Some(Input::Select));
        assert_eq!(game.lang_index, 1); // French
        assert_eq!(game.string(0), "Sélect"); // actual French pack string
        // Exit returns to Options (ix 3).
        game.update(Some(Input::Exit));
        assert_eq!(game.menu.as_ref().unwrap().ix, 3);
    }
}

#[cfg(test)]
mod help_about_tests {
    #[test]
    fn help_and_about_enter_state_107() {
        // Help: Right,Right from Play Game, then Select.
        let mut g = super::Game::new("../extracted");
        g.boot().unwrap();
        g.update(Some(super::Input::Select)); // language -> audio
        g.update(Some(super::Input::Select)); // audio yes -> menu (settle)
        for _ in 0..200 {
            if g.state == super::State::MainMenu && !g.cam_moving && g.cam_x == 0 && g.cam_y == 0 {
                break;
            }
            g.tick(40);
        }
        g.update(Some(super::Input::Right));
        g.update(Some(super::Input::Right));
        g.update(Some(super::Input::Select));
        for _ in 0..200 {
            if g.state == super::State::HelpAbout {
                break;
            }
            g.tick(40);
        }
        assert_eq!(g.state, super::State::HelpAbout);
        assert!(g.help_about_text.starts_with("Combat"));
        assert!(g.help_about_text.contains("Support@spe.sony.com"));
        // Back to menu.
        g.update(Some(super::Input::Exit));
        assert_eq!(g.state, super::State::MainMenu);
    }

    #[test]
    fn about_text_has_version() {
        let mut g = super::Game::new("../extracted");
        g.boot().unwrap();
        g.update(Some(super::Input::Select));
        g.update(Some(super::Input::Select));
        for _ in 0..200 {
            if g.state == super::State::MainMenu && !g.cam_moving && g.cam_x == 0 && g.cam_y == 0 {
                break;
            }
            g.tick(40);
        }
        for _ in 0..4 {
            g.update(Some(super::Input::Right));
        }
        g.update(Some(super::Input::Select));
        for _ in 0..200 {
            if g.state == super::State::HelpAbout {
                break;
            }
            g.tick(40);
        }
        assert_eq!(g.state, super::State::HelpAbout);
        assert!(g.help_about_text.starts_with("Version 1.4.8"));
    }
}

#[cfg(test)]
mod audio_tests {
    #[test]
    fn audio_prompt_yes_enables_and_plays_slot_0() {
        let mut g = super::Game::new("../extracted");
        g.boot().unwrap();
        g.update(Some(super::Input::Select)); // language -> audio prompt
        assert_eq!(g.state, super::State::AudioPrompt);
        g.update(Some(super::Input::Select)); // yes
        assert_eq!(g.audio_enabled, Some(true));
        assert!(g.audio.enabled);
        assert_eq!(g.audio.current_slot, Some(0));
        // No: disable
        let mut g2 = super::Game::new("../extracted");
        g2.boot().unwrap();
        g2.update(Some(super::Input::Select));
        g2.update(Some(super::Input::Exit));
        assert_eq!(g2.audio_enabled, Some(false));
        assert!(!g2.audio.enabled);
        assert_eq!(g2.audio.current_slot, None);
    }
}
