//! Render thread: owns the EGL context, `egui_glow::Painter`, and `ui::Ui`.
//!
//! When mirroring fullscreen with HUD closed (`uf.visible == false`), clears the overlay
//! surface to transparent RGBA(0,0,0,0) once and stops swapping so the hardware video plane
//! underneath runs with zero GPU overhead.

use super::egl::{Core, Window};
use crate::media::MediaEngine;
use crate::ui::{self, Key};
use ndk::native_window::NativeWindow;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub static UI_READY: AtomicBool = AtomicBool::new(false);
const TICK: Duration = Duration::from_millis(250);

pub enum Ev {
    Key(Key),
    Overlay(Option<NativeWindow>, Sender<()>),
    Wake(Duration),
    Ready(Arc<MediaEngine>),
}

struct R {
    core: Option<Core>,
    win: Option<Window>,
    painter: Option<egui_glow::Painter>,
    ui: Option<ui::Ui>,
    pending_keys: Vec<Key>,
    textures: egui::TexturesDelta,
    shown: bool,
    due: Option<Instant>,
}

pub fn spawn(_tx: Sender<Ev>, rx: Receiver<Ev>) {
    std::thread::Builder::new()
        .name("yansit-ui".into())
        .spawn(move || {
            let r = R {
                core: None,
                win: None,
                painter: None,
                ui: None,
                pending_keys: Vec::new(),
                textures: Default::default(),
                shown: false,
                due: None,
            };
            r.run(rx);
        })
        .expect("spawn render thread");
}

fn earliest(a: Option<Instant>, b: Instant) -> Instant {
    a.map_or(b, |a| a.min(b))
}

impl R {
    fn run(mut self, rx: Receiver<Ev>) {
        let mut tick = Instant::now() + TICK;
        loop {
            let until = earliest(self.due, tick);
            match rx.recv_timeout(until.saturating_duration_since(Instant::now())) {
                Ok(ev) => {
                    self.handle(ev);
                    while let Ok(ev) = rx.try_recv() {
                        self.handle(ev);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            let now = Instant::now();
            if now >= tick {
                tick = now + TICK;
                self.wake(Duration::ZERO);
            }
            if self.due.is_some_and(|t| Instant::now() >= t) {
                self.frame();
            }
        }
    }

    fn wake(&mut self, d: Duration) {
        if self.ui.is_some() {
            self.due = Some(earliest(self.due, Instant::now() + d));
        }
    }

    fn handle(&mut self, ev: Ev) {
        match ev {
            Ev::Key(k) => {
                self.pending_keys.push(k);
                self.wake(Duration::ZERO);
            }
            Ev::Overlay(win, ack) => {
                if let (Some(core), Some(w)) = (self.core.as_ref(), self.win.take()) {
                    core.destroy(w);
                }
                self.shown = false;
                if let Some(win) = win {
                    if self.core.is_none() {
                        self.core = Core::new();
                    }
                    self.win = self.core.as_ref().and_then(|c| c.create(win));
                    self.clear_transparent();
                    self.wake(Duration::ZERO);
                }
                let _ = ack.send(());
            }
            Ev::Wake(d) => self.wake(d),
            Ev::Ready(engine) => {
                self.ui = Some(ui::Ui::new(engine));
                UI_READY.store(true, Relaxed);
                self.wake(Duration::ZERO);
            }
        }
    }

    fn clear_transparent(&mut self) {
        let (Some(core), Some(w)) = (self.core.as_ref(), self.win.as_ref()) else {
            return;
        };
        if core.make_current(Some(w)) {
            unsafe { clear(&core.gl) };
            core.swap(w);
        }
    }

    fn frame(&mut self) {
        self.due = None;
        let Some(ui) = self.ui.as_mut() else {
            return;
        };
        let size = match (self.core.as_ref(), self.win.as_ref()) {
            (Some(c), Some(w)) => c.size(w).unwrap_or([1920, 1080]),
            _ => [1920, 1080],
        };
        let keys: Vec<Key> = std::mem::take(&mut self.pending_keys);
        let mut uf = ui.frame(size, &keys);

        if ui.take_disconnect_request() {
            super::disconnect_active_client();
        }
        if ui.take_exit() {
            super::EXIT.store(true, Relaxed);
        }

        self.textures
            .append(std::mem::take(&mut uf.out.textures_delta));
        self.due = Some(earliest(self.due, Instant::now() + uf.repaint_after));

        if !uf.visible && !self.shown {
            return;
        }
        let (Some(core), Some(w)) = (self.core.as_ref(), self.win.as_ref()) else {
            return;
        };
        if !core.make_current(Some(w)) {
            return;
        }
        if self.painter.is_none() {
            match egui_glow::Painter::new(core.gl.clone(), "", None, false) {
                Ok(p) => self.painter = Some(p),
                Err(e) => {
                    log::error!("egui_glow::Painter: {e}");
                    return;
                }
            }
        }
        let painter = self.painter.as_mut().unwrap();
        unsafe {
            use egui_glow::glow::HasContext;
            core.gl.disable(egui_glow::glow::SCISSOR_TEST);
            core.gl.viewport(0, 0, size[0] as i32, size[1] as i32);
            clear(&core.gl);
        }
        if uf.visible {
            let prims = ui
                .ctx()
                .tessellate(std::mem::take(&mut uf.out.shapes), uf.out.pixels_per_point);
            painter.paint_and_update_textures(
                size,
                uf.out.pixels_per_point,
                &prims,
                &mut self.textures,
            );
        }
        core.swap(w);
        self.shown = uf.visible;
    }
}

unsafe fn clear(gl: &egui_glow::glow::Context) {
    use egui_glow::glow::HasContext;
    gl.clear_color(0.0, 0.0, 0.0, 0.0);
    gl.clear(egui_glow::glow::COLOR_BUFFER_BIT);
}
