//! Live playable window (slice 4a part 2).
//!
//! winit for the OS window + keyboard, softbuffer for presenting the
//! 240x320 game framebuffer at integer scale (default 3x = 720x960).
//! Software rendering only — no GPU, no crates beyond winit/softbuffer,
//! so it runs on a plain Windows install.
//!
//! This module is feature-gated (`--features window`) so the default
//! build stays std-only and `cargo test` never touches a window.
//!
//! Controls (the phone's keys, mapped to a PC keyboard):
//!   Arrow Up/Down    language select / Help scroll
//!   Arrow Left/Right menu carousel, or hold to walk in gameplay
//!   Enter / Space    Select (left softkey); Space in gameplay = Attack
//!   Escape           Exit / Back (right softkey)
//!   Close window     quit
//!
//! Frame loop: one game tick per 40 ms, paced with
//! ControlFlow::WaitUntil (the same FrameTimer idea as window.rs,
//! but driven by the event loop). Menu camera transitions advance
//! through Game::tick(40); gameplay advances through
//! Game::tick_playing() with the held arrow direction, so holding
//! Right walks continuously instead of one step per key press.

use std::num::NonZeroU32;
use std::time::{Duration, Instant};

use softbuffer::{Context, Surface};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowAttributes, WindowId};

use crate::game::{Game, Input, State};
use crate::midp::Framebuffer;
use crate::window::{scale_bilinear, scale_nearest};

pub fn run(dir: &std::path::Path, factor: usize, save_dir: Option<&str>) -> i32 {
    let mut game = Game::new(dir);
    if let Some(sd) = save_dir {
        game.set_save_dir(sd);
    }
    // HD art: if an hd/ folder with AI-upscaled sheets sits next to
    // the RP packs, the game renders at 4x (960x1280) from it.
    if game.res.enable_hd() {
        eprintln!("HD art enabled: rendering at 960x1280 from hd/ sheets");
    }
    if let Err(e) = game.boot() {
        eprintln!("boot failed: {e}");
        return 1;
    }
    let event_loop = match EventLoop::new() {
        Ok(el) => el,
        Err(e) => {
            eprintln!("event loop failed: {e}");
            return 1;
        }
    };
    let mut app = App {
        window: None,
        surface: None,
        context: None,
        game,
        factor,
        left: false,
        right: false,
        down: false,
        attack_queued: false,
        jump_queued: false,
        key_queued: 0,
        next_frame: Instant::now(),
    };
    let _ = event_loop.run_app(&mut app);
    0
}

struct App {
    window: Option<std::rc::Rc<Window>>,
    surface: Option<Surface<std::rc::Rc<Window>, std::rc::Rc<Window>>>,
    context: Option<Context<std::rc::Rc<Window>>>,
    game: Game,
    factor: usize,
    left: bool,
    right: bool,
    down: bool,
    attack_queued: bool,
    jump_queued: bool,
    /// Key code of the most recent discrete press
    /// edge (Phase 8, the grapple mash): Left 2,
    /// Right 5, Down 6; Up (1) and Attack (8) come
    /// through the jump/attack queues. Consumed in
    /// step() and passed to Game.key_code.
    key_queued: i32,
    next_frame: Instant,
}

impl App {
    fn step(&mut self) {
        // Held arrows -> held_dir, before the gameplay tick.
        let dir = if self.left && !self.right {
            -1
        } else if self.right && !self.left {
            1
        } else {
            0
        };
        self.game.held_dir = dir;
        self.game.held_down = self.down;
        // Press edges (Phase 8): the grapple mash
        // reads the most recent discrete press.
        // Attack (8) and Up (1) come through their
        // queues; Left/Right/Down through key_queued.
        if self.attack_queued {
            self.game.key_code = 8;
        } else if self.jump_queued {
            self.game.key_code = 1;
        } else if self.key_queued != 0 {
            self.game.key_code = self.key_queued;
            self.key_queued = 0;
        }
        self.game.tick(40);
        if self.game.state == State::Playing {
            if self.attack_queued {
                self.game.tick_playing_attack();
                self.attack_queued = false;
            } else if self.jump_queued {
                self.game.tick_playing_jump();
                self.jump_queued = false;
            } else {
                self.game.tick_playing();
            }
        }
    }

    fn render(&mut self) {
        let Some(window) = &self.window else { return };
        let Some(surface) = &mut self.surface else { return };
        // Render at the game's scale (1x = 240x320, HD = 960x1280).
        let scale = self.game.res.render_scale();
        let mut fb = Framebuffer::game_scaled(scale);
        self.game.draw(&mut fb);
        // Fit the frame into the window, preserving the 3:4 aspect,
        // centred on black. SD art keeps crisp integer nearest
        // scaling; HD art (and any non-integer fit) uses bilinear.
        let size = window.inner_size();
        let (ww, wh) = (size.width as usize, size.height as usize);
        if ww == 0 || wh == 0 {
            return;
        }
        let fit = ((ww as f64 / fb.width as f64).min(wh as f64 / fb.height as f64))
            .max(0.0);
        let (out_w, out_h) = (
            ((fb.width as f64 * fit).round() as usize).max(1),
            ((fb.height as f64 * fit).round() as usize).max(1),
        );
        let scaled = if fb.scale == 1 && (fit - fit.round()).abs() < 1e-6 && fit >= 1.0 {
            scale_nearest(&fb, fit.round() as usize)
        } else {
            scale_bilinear(&fb, out_w, out_h)
        };
        let Ok(mut buffer) = surface.buffer_mut() else { return };
        // Black letterbox, then the centred frame.
        for dst in buffer.iter_mut() {
            *dst = 0;
        }
        let ox = (ww.saturating_sub(scaled.width)) / 2;
        let oy = (wh.saturating_sub(scaled.height)) / 2;
        for y in 0..scaled.height {
            if oy + y >= wh {
                break;
            }
            for x in 0..scaled.width {
                if ox + x >= ww {
                    break;
                }
                // softbuffer pixels are 0x00RRGGBB; our ARGB pixels just
                // drop the alpha channel (the game never draws alpha 0
                // as a visible colour — alpha 0 pixels are skipped).
                buffer[(oy + y) * ww + (ox + x)] =
                    scaled.pixels[y * scaled.width + x] & 0x00FF_FFFF;
            }
        }
        let _ = buffer.present();
    }

    fn on_key(&mut self, event: &KeyEvent) {
        if event.repeat {
            return;
        }
        let pressed = event.state == ElementState::Pressed;
        let code = match event.physical_key {
            PhysicalKey::Code(c) => c,
            _ => return,
        };
        match code {
            KeyCode::ArrowLeft => {
                self.left = pressed;
                if pressed && self.game.state == State::Playing {
                    // Press edge (Phase 8): the
                    // grapple mash reads it.
                    self.key_queued = 2;
                } else if pressed {
                    self.game.update(Some(Input::Left));
                }
            }
            KeyCode::ArrowRight => {
                self.right = pressed;
                if pressed && self.game.state == State::Playing {
                    self.key_queued = 5;
                } else if pressed {
                    self.game.update(Some(Input::Right));
                }
            }
            KeyCode::ArrowUp => {
                if pressed {
                    if self.game.state == State::Playing {
                        // Queued like Attack: the frame step runs
                        // exactly one jump frame for this press.
                        self.jump_queued = true;
                    } else {
                        self.game.update(Some(Input::Up));
                    }
                }
            }
            KeyCode::ArrowDown => {
                // Hold-to-block (slice 18): like the
                // walk arrows, the held state drives
                // every frame; menus still get the
                // press for scrolling.
                self.down = pressed;
                if pressed && self.game.state == State::Playing {
                    // Press edge (Phase 8): the
                    // grapple mash reads it.
                    self.key_queued = 6;
                } else if pressed {
                    self.game.update(Some(Input::Down));
                }
            }
            KeyCode::Enter => {
                if pressed {
                    self.game.update(Some(Input::Select));
                }
            }
            KeyCode::Space => {
                if pressed {
                    if self.game.state == State::Playing {
                        self.attack_queued = true;
                    } else {
                        // Fire (s == 8) is also Select in menus.
                        self.game.update(Some(Input::Select));
                    }
                }
            }
            KeyCode::Escape => {
                if pressed {
                    self.game.update(Some(Input::Exit));
                }
            }
            KeyCode::F11 => {
                // Fullscreen toggle: borderless on the current monitor,
                // back to windowed on the next press. The Resized event
                // re-fits the surface; render() centres the frame.
                if pressed {
                    if let Some(window) = &self.window {
                        let fs = if window.fullscreen().is_some() {
                            None
                        } else {
                            Some(winit::window::Fullscreen::Borderless(None))
                        };
                        window.set_fullscreen(fs);
                    }
                }
            }
            _ => {}
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let w = 240 * self.factor as u32;
        let h = 320 * self.factor as u32;
        let attrs = WindowAttributes::default()
            .with_title("God of War: Betrayal (PC port)")
            .with_inner_size(winit::dpi::PhysicalSize::new(w, h))
            .with_resizable(false);
        let window = match event_loop.create_window(attrs) {
            Ok(w) => std::rc::Rc::new(w),
            Err(e) => {
                eprintln!("create window failed: {e}");
                event_loop.exit();
                return;
            }
        };
        let context = match Context::new(window.clone()) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("softbuffer context failed: {e}");
                event_loop.exit();
                return;
            }
        };
        let mut surface = match Surface::new(&context, window.clone()) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("softbuffer surface failed: {e}");
                event_loop.exit();
                return;
            }
        };
        if let Err(e) = surface.resize(
            NonZeroU32::new(w).unwrap(),
            NonZeroU32::new(h).unwrap(),
        ) {
            eprintln!("surface resize failed: {e}");
            event_loop.exit();
            return;
        }
        self.window = Some(window);
        self.surface = Some(surface);
        self.context = Some(context);
        self.next_frame = Instant::now() + Duration::from_millis(40);
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                // Fullscreen toggle / monitor change: re-fit the surface
                // to the new window size; render() centres the frame.
                if let Some(surface) = &mut self.surface {
                    if size.width > 0 && size.height > 0 {
                        let _ = surface.resize(
                            NonZeroU32::new(size.width).unwrap(),
                            NonZeroU32::new(size.height).unwrap(),
                        );
                    }
                }
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                self.on_key(&event);
                if self.game.exited {
                    event_loop.exit();
                }
            }
            WindowEvent::RedrawRequested => {
                self.render();
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            return;
        }
        let now = Instant::now();
        if now >= self.next_frame {
            self.step();
            if self.game.exited {
                event_loop.exit();
                return;
            }
            if let Some(window) = &self.window {
                window.request_redraw();
            }
            // Catch up at most one frame behind, else resync.
            self.next_frame += Duration::from_millis(40);
            if self.next_frame < now {
                self.next_frame = now + Duration::from_millis(40);
            }
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
    }
}
