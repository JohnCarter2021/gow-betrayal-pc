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
use crate::window::scale_nearest;

pub fn run(dir: &std::path::Path, factor: usize, save_dir: Option<&str>) -> i32 {
    let mut game = Game::new(dir);
    if let Some(sd) = save_dir {
        game.set_save_dir(sd);
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
        attack_queued: false,
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
    attack_queued: bool,
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
        self.game.tick(40);
        if self.game.state == State::Playing {
            if self.attack_queued {
                self.game.tick_playing_attack();
                self.attack_queued = false;
            } else {
                self.game.tick_playing();
            }
        }
    }

    fn render(&mut self) {
        if self.window.is_none() {
            return;
        }
        let Some(surface) = &mut self.surface else { return };
        let w = 240 * self.factor as u32;
        let h = 320 * self.factor as u32;
        let mut fb = Framebuffer::game();
        self.game.draw(&mut fb);
        let scaled = scale_nearest(&fb, self.factor);
        debug_assert_eq!(scaled.width as u32, w);
        debug_assert_eq!(scaled.height as u32, h);
        let Ok(mut buffer) = surface.buffer_mut() else { return };
        // softbuffer pixels are 0x00RRGGBB; our ARGB pixels just
        // drop the alpha channel (the game never draws alpha 0
        // as a visible colour — alpha 0 pixels are skipped).
        for (dst, &src) in buffer.iter_mut().zip(scaled.pixels.iter()) {
            *dst = src & 0x00FF_FFFF;
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
                if pressed && self.game.state != State::Playing {
                    self.game.update(Some(Input::Left));
                }
            }
            KeyCode::ArrowRight => {
                self.right = pressed;
                if pressed && self.game.state != State::Playing {
                    self.game.update(Some(Input::Right));
                }
            }
            KeyCode::ArrowUp => {
                if pressed {
                    self.game.update(Some(Input::Up));
                }
            }
            KeyCode::ArrowDown => {
                if pressed {
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
