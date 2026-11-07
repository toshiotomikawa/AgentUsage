//! Tiny eased-tween store keyed by string ids.

use std::collections::HashMap;

struct Tween {
    from: f32,
    to: f32,
    start: f64,
}

#[derive(Default)]
pub struct Tweens {
    map: HashMap<String, Tween>,
    pub active: bool,
}

pub fn ease_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

pub fn ease_in_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

impl Tweens {
    /// Value moving toward `target`; starts from `initial` the first time an id is seen.
    pub fn get(
        &mut self,
        id: &str,
        target: f32,
        initial: f32,
        now: f64,
        dur: f32,
        delay: f32,
    ) -> f32 {
        let tw = self.map.entry(id.to_string()).or_insert(Tween {
            from: initial,
            to: target,
            start: now + delay as f64,
        });
        if (tw.to - target).abs() > 1e-4 {
            let current = value(tw, now, dur);
            tw.from = current;
            tw.to = target;
            tw.start = now;
        }
        let v = value(tw, now, dur);
        if now < tw.start + dur as f64 {
            self.active = true;
        }
        v
    }
}

fn value(tw: &Tween, now: f64, dur: f32) -> f32 {
    let t = ((now - tw.start) as f32 / dur).clamp(0.0, 1.0);
    tw.from + (tw.to - tw.from) * ease_out_cubic(t)
}
