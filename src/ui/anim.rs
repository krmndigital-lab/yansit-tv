//! Tiny tween store driven by egui's input time (so snapshots can simulate time).
//!
//! Every animated value is keyed by an [`egui::Id`]. Changing the target starts a new
//! ease-out-cubic tween from the current value, so rapid D-pad presses retarget smoothly.
#![allow(dead_code)]

use std::collections::HashMap;

use egui::Id;

pub fn ease_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

pub fn ease_in_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

/// Overshooting ease for the logo pop.
pub fn ease_out_back(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let c1 = 1.70158;
    let c3 = c1 + 1.0;
    1.0 + c3 * (t - 1.0).powi(3) + c1 * (t - 1.0).powi(2)
}

/// Progress of `now` within `[start, start + dur]`, 0..1.
pub fn progress(now: f64, start: f64, dur: f32) -> f32 {
    if dur <= 0.0 {
        return 1.0;
    }
    (((now - start) / dur as f64) as f32).clamp(0.0, 1.0)
}

pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[derive(Clone, Copy, Debug)]
struct Tween {
    from: f32,
    to: f32,
    start: f64,
    dur: f32,
    last_frame: u64,
}

impl Tween {
    fn value(&self, now: f64) -> f32 {
        lerp(
            self.from,
            self.to,
            ease_out_cubic(progress(now, self.start, self.dur)),
        )
    }
    fn done(&self, now: f64) -> bool {
        now >= self.start + self.dur as f64
    }
}

#[derive(Default)]
pub struct Anims {
    map: HashMap<Id, Tween>,
    now: f64,
    frame: u64,
    busy: bool,
}

impl Anims {
    pub fn begin_frame(&mut self, now: f64) {
        self.now = now;
        self.frame += 1;
        self.busy = false;
    }

    /// Drops tweens that were not touched for a few frames.
    pub fn end_frame(&mut self) {
        let frame = self.frame;
        if self.map.len() > 256 {
            self.map
                .retain(|_, t| frame.saturating_sub(t.last_frame) < 4);
        }
    }

    pub fn now(&self) -> f64 {
        self.now
    }

    /// True if any tween touched this frame is still running.
    pub fn busy(&self) -> bool {
        self.busy
    }

    /// Marks the frame as animating (for hand-rolled animations).
    pub fn keep_alive(&mut self) {
        self.busy = true;
    }

    /// Animates towards `target`. The first time a key is seen it snaps to `target`.
    pub fn value(&mut self, id: Id, target: f32, dur: f32) -> f32 {
        self.value_from(id, target, target, dur)
    }

    /// Like [`Self::value`], but the first time a key is seen it animates from `initial`.
    pub fn value_from(&mut self, id: Id, initial: f32, target: f32, dur: f32) -> f32 {
        let now = self.now;
        let frame = self.frame;
        let t = self.map.entry(id).or_insert(Tween {
            from: initial,
            to: target,
            start: now,
            dur,
            last_frame: frame,
        });
        // A key that was not shown for a while restarts from its last value (no stale jump).
        if frame.saturating_sub(t.last_frame) > 2 && t.done(now) {
            t.from = t.to;
        }
        t.last_frame = frame;
        if (t.to - target).abs() > 1e-4 {
            let cur = t.value(now);
            *t = Tween {
                from: cur,
                to: target,
                start: now,
                dur,
                last_frame: frame,
            };
        }
        let v = t.value(now);
        if !t.done(now) {
            self.busy = true;
        }
        v
    }

    /// Bool convenience: 0.0 / 1.0 target.
    pub fn toggle(&mut self, id: Id, on: bool, dur: f32) -> f32 {
        self.value(id, if on { 1.0 } else { 0.0 }, dur)
    }

    /// Forces a key to a value immediately (e.g. when a screen is (re)entered).
    pub fn set(&mut self, id: Id, value: f32) {
        let frame = self.frame;
        self.map.insert(
            id,
            Tween {
                from: value,
                to: value,
                start: self.now,
                dur: 0.0,
                last_frame: frame,
            },
        );
    }

    /// Restarts a key from `from` towards `to`.
    pub fn restart(&mut self, id: Id, from: f32, to: f32, dur: f32) {
        let frame = self.frame;
        self.map.insert(
            id,
            Tween {
                from,
                to,
                start: self.now,
                dur,
                last_frame: frame,
            },
        );
        self.busy = true;
    }

    pub fn clear(&mut self) {
        self.map.clear();
    }
}

/// Hash key for tweens / egui ids (stable across frames).
pub fn k(x: impl std::hash::Hash + std::fmt::Debug) -> Id {
    Id::NULL.with(x)
}
