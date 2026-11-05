//! Historique en mémoire : un point par minute sur la dernière heure.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

pub const POINTS: usize = 60;
const STEP: Duration = Duration::from_secs(60);

#[derive(Default)]
pub struct Ring(VecDeque<f32>);

impl Ring {
    pub fn push(&mut self, v: f32) {
        if self.0.len() >= POINTS {
            self.0.pop_front();
        }
        self.0.push_back(v);
    }

    pub fn values(&self) -> Vec<f32> {
        self.0.iter().copied().collect()
    }
}

#[derive(Default)]
struct Accum {
    cpu: f32,
    mem: f32,
    n: u32,
    temp: f32,
    nt: u32,
}

#[derive(Default)]
pub struct History {
    pub cpu: Ring,
    pub mem: Ring,
    pub temp: Ring,
    acc: Accum,
    last: Option<Instant>,
}

impl History {
    /// Cumule la mesure ; ajoute un point (moyenne) au plus une fois par minute.
    /// Retourne `true` si les courbes ont changé.
    pub fn record(&mut self, now: Instant, cpu: f32, mem: f32, temp: Option<f32>) -> bool {
        self.acc.cpu += cpu;
        self.acc.mem += mem;
        self.acc.n += 1;
        if let Some(t) = temp {
            self.acc.temp += t;
            self.acc.nt += 1;
        }
        if self.last.is_some_and(|l| now.duration_since(l) < STEP) {
            return false;
        }
        let n = self.acc.n.max(1) as f32;
        self.cpu.push(self.acc.cpu / n);
        self.mem.push(self.acc.mem / n);
        if self.acc.nt > 0 {
            self.temp.push(self.acc.temp / self.acc.nt as f32);
        }
        self.acc = Accum::default();
        self.last = Some(now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_keeps_last_hour() {
        let mut r = Ring::default();
        for i in 0..100 {
            r.push(i as f32);
        }
        let v = r.values();
        assert_eq!(v.len(), POINTS);
        assert_eq!(v[0], 40.0);
        assert_eq!(*v.last().unwrap(), 99.0);
    }

    #[test]
    fn one_averaged_point_per_minute() {
        let mut h = History::default();
        let t0 = Instant::now();
        assert!(h.record(t0, 10.0, 50.0, Some(40.0))); // premier point immédiat
        assert!(!h.record(t0 + Duration::from_secs(3), 20.0, 60.0, None));
        assert!(!h.record(t0 + Duration::from_secs(30), 40.0, 70.0, Some(50.0)));
        assert!(h.record(t0 + Duration::from_secs(61), 30.0, 80.0, None));
        assert_eq!(h.cpu.values(), vec![10.0, 30.0]); // (20+40+30)/3
        assert_eq!(h.mem.values(), vec![50.0, 70.0]);
        assert_eq!(h.temp.values(), vec![40.0, 50.0]);
    }
}
